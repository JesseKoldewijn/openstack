use std::collections::HashMap;

use bytes::Bytes;
use openstack_cloudformation::CloudFormationProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, params: HashMap<String, String>) -> RequestContext {
    RequestContext {
        service: "cloudformation".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: json!({}),
        raw_body: Bytes::new(),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: params,
    }
}

#[allow(dead_code)]
fn make_ctx_body(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "cloudformation".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    }
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_stack() {
    let p = CloudFormationProvider::new();
    let template = json!({
        "Resources": {
            "MyBucket": {
                "Type": "AWS::S3::Bucket",
                "Properties": {}
            }
        }
    });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "my-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );

    let resp = p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "CreateStack failed: {}",
        body_str(&resp)
    );
    assert!(body_str(&resp).contains("StackId"), "Missing StackId");
    assert!(body_str(&resp).contains("my-stack"));
}

#[tokio::test]
async fn test_create_duplicate_stack_fails() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": {} });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "dup-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );

    p.dispatch(&make_ctx("CreateStack", params.clone()))
        .await
        .unwrap();
    let resp = p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("AlreadyExistsException"));
}

#[tokio::test]
async fn test_describe_stacks() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": {} });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "stack-desc".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeStacks", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("stack-desc"));
}

#[tokio::test]
async fn test_list_stacks() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": {} });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "list-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let resp = p
        .dispatch(&make_ctx("ListStacks", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("list-stack"));
    assert!(body_str(&resp).contains("CREATE_COMPLETE"));
}

#[tokio::test]
async fn test_delete_stack() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": {} });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "del-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let mut del_params = HashMap::new();
    del_params.insert("StackName".to_string(), "del-stack".to_string());
    let resp = p
        .dispatch(&make_ctx("DeleteStack", del_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    // Should no longer appear
    let resp = p
        .dispatch(&make_ctx("ListStacks", HashMap::new()))
        .await
        .unwrap();
    assert!(!body_str(&resp).contains("del-stack"));
}

#[tokio::test]
async fn test_stack_with_resources() {
    let p = CloudFormationProvider::new();
    let template = json!({
        "Resources": {
            "Queue1": { "Type": "AWS::SQS::Queue", "Properties": {} },
            "Topic1": { "Type": "AWS::SNS::Topic", "Properties": {} },
        }
    });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "resource-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let mut desc_params = HashMap::new();
    desc_params.insert("StackName".to_string(), "resource-stack".to_string());
    let resp = p
        .dispatch(&make_ctx("DescribeStackResources", desc_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("Queue1"));
    assert!(body_str(&resp).contains("Topic1"));
    assert!(body_str(&resp).contains("AWS::SQS::Queue"));
}

#[tokio::test]
async fn test_template_intrinsic_ref() {
    let p = CloudFormationProvider::new();
    let template = json!({
        "Parameters": {
            "BucketName": { "Type": "String" }
        },
        "Resources": {
            "Bucket": { "Type": "AWS::S3::Bucket" }
        },
        "Outputs": {
            "BucketRef": { "Value": { "Ref": "BucketName" } }
        }
    });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "ref-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    params.insert(
        "Parameters.member.1.ParameterKey".to_string(),
        "BucketName".to_string(),
    );
    params.insert(
        "Parameters.member.1.ParameterValue".to_string(),
        "my-bucket-123".to_string(),
    );

    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let mut desc_params = HashMap::new();
    desc_params.insert("StackName".to_string(), "ref-stack".to_string());
    let resp = p
        .dispatch(&make_ctx("DescribeStacks", desc_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("my-bucket-123"));
}

#[tokio::test]
async fn test_validate_template() {
    let p = CloudFormationProvider::new();
    let resp = p
        .dispatch(&make_ctx("ValidateTemplate", HashMap::new()))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("ValidateTemplateResponse"));
}

#[tokio::test]
async fn test_get_template() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": { "R": { "Type": "AWS::S3::Bucket" } } });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "get-tpl-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let mut get_params = HashMap::new();
    get_params.insert("StackName".to_string(), "get-tpl-stack".to_string());
    let resp = p
        .dispatch(&make_ctx("GetTemplate", get_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("Resources"));
}

#[tokio::test]
async fn test_update_stack() {
    let p = CloudFormationProvider::new();
    let template = json!({ "Resources": {} });
    let mut params = HashMap::new();
    params.insert("StackName".to_string(), "upd-stack".to_string());
    params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&template).unwrap(),
    );
    p.dispatch(&make_ctx("CreateStack", params)).await.unwrap();

    let new_template = json!({ "Resources": { "NewRes": { "Type": "AWS::SQS::Queue" } } });
    let mut upd_params = HashMap::new();
    upd_params.insert("StackName".to_string(), "upd-stack".to_string());
    upd_params.insert(
        "TemplateBody".to_string(),
        serde_json::to_string(&new_template).unwrap(),
    );
    let resp = p
        .dispatch(&make_ctx("UpdateStack", upd_params))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("UpdateStackResponse"));
}
