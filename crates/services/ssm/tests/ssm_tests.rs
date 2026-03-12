use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_ssm::SsmProvider;
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "ssm".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

#[tokio::test]
async fn test_put_and_get_parameter() {
    let p = SsmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "PutParameter",
            json!({
                "Name": "/app/db/url",
                "Value": "postgres://localhost/mydb",
                "Type": "String",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Version"], 1);

    let resp = p
        .dispatch(&make_ctx(
            "GetParameter",
            json!({
                "Name": "/app/db/url",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(
        body(&resp)["Parameter"]["Value"],
        "postgres://localhost/mydb"
    );
}

#[tokio::test]
async fn test_put_parameter_no_overwrite() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({
            "Name": "/app/key",
            "Value": "v1",
            "Type": "String",
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutParameter",
            json!({
                "Name": "/app/key",
                "Value": "v2",
                "Type": "String",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ParameterAlreadyExists"));
}

#[tokio::test]
async fn test_put_parameter_overwrite() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({
            "Name": "/overwrite/key",
            "Value": "v1",
            "Type": "String",
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutParameter",
            json!({
                "Name": "/overwrite/key",
                "Value": "v2",
                "Type": "String",
                "Overwrite": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Version"], 2);

    let resp = p
        .dispatch(&make_ctx(
            "GetParameter",
            json!({ "Name": "/overwrite/key" }),
        ))
        .await
        .unwrap();
    assert_eq!(body(&resp)["Parameter"]["Value"], "v2");
}

#[tokio::test]
async fn test_get_parameters() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/a", "Value": "1", "Type": "String" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/b", "Value": "2", "Type": "String" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "GetParameters",
            json!({
                "Names": ["/a", "/b", "/nonexistent"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["Parameters"].as_array().unwrap().len(), 2);
    assert_eq!(b["InvalidParameters"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_get_parameters_by_path() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/myapp/db/host", "Value": "localhost", "Type": "String" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/myapp/db/port", "Value": "5432", "Type": "String" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/myapp/api/key", "Value": "secret", "Type": "String" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "GetParametersByPath",
            json!({
                "Path": "/myapp/db",
                "Recursive": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Parameters"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_delete_parameter() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/del", "Value": "bye", "Type": "String" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DeleteParameter", json!({ "Name": "/del" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("GetParameter", json!({ "Name": "/del" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ParameterNotFound"));
}

#[tokio::test]
async fn test_describe_parameters() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/desc/p1", "Value": "x", "Type": "String" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeParameters", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let params = body(&resp)["Parameters"].as_array().unwrap().clone();
    assert!(!params.is_empty());
}

#[tokio::test]
async fn test_delete_parameters_batch() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/batch/a", "Value": "1", "Type": "String" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/batch/b", "Value": "2", "Type": "String" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteParameters",
            json!({
                "Names": ["/batch/a", "/batch/b", "/batch/nonexistent"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["DeletedParameters"].as_array().unwrap().len(), 2);
    assert_eq!(b["InvalidParameters"].as_array().unwrap().len(), 1);
}
