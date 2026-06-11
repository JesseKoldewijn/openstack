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
        raw_body: Some(Bytes::from(serde_json::to_vec(&body).unwrap())),
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
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
    assert_eq!(resp.content_type, "application/json");
    let payload = body(&resp);
    assert_eq!(payload["__type"], "ParameterNotFound");
    assert_eq!(payload["message"], "Parameter /del not found.");
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

// ---------------------------------------------------------------------------
// GetParameterHistory
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_parameter_history() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/hist/p", "Value": "v1", "Type": "String" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutParameter",
        json!({ "Name": "/hist/p", "Value": "v2", "Type": "String", "Overwrite": true }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "GetParameterHistory",
            json!({ "Name": "/hist/p" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let params = b["Parameters"].as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["Version"], 1);
    assert_eq!(params[1]["Version"], 2);
}

#[tokio::test]
async fn test_get_parameter_history_not_found() {
    let p = SsmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetParameterHistory",
            json!({ "Name": "/nonexistent" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body(&resp);
    assert!(b["__type"].as_str().unwrap().contains("ParameterNotFound"));
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_describe_document() {
    let p = SsmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateDocument",
            json!({
                "Name": "MyRunbook",
                "Content": "{\"schemaVersion\":\"2.2\"}",
                "DocumentType": "Command",
                "DocumentFormat": "JSON",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["DocumentDescription"]["Name"], "MyRunbook");
    assert_eq!(b["DocumentDescription"]["Status"], "Active");

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeDocument",
            json!({ "Name": "MyRunbook" }),
        ))
        .await
        .unwrap();
    assert_eq!(desc_resp.status_code, 200);
    let db = body(&desc_resp);
    assert_eq!(db["Document"]["Name"], "MyRunbook");
    assert_eq!(db["Document"]["DocumentType"], "Command");
}

#[tokio::test]
async fn test_create_document_duplicate_fails() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "CreateDocument",
        json!({ "Name": "DupDoc", "Content": "{}", "DocumentType": "Command" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateDocument",
            json!({ "Name": "DupDoc", "Content": "{}", "DocumentType": "Command" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("DocumentAlreadyExists")
    );
}

#[tokio::test]
async fn test_get_document() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "CreateDocument",
        json!({
            "Name": "GetMe",
            "Content": "{\"schemaVersion\":\"2.2\",\"mainSteps\":[]}",
            "DocumentType": "Command",
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("GetDocument", json!({ "Name": "GetMe" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Name"], "GetMe");
    assert!(b["Content"].as_str().unwrap().contains("mainSteps"));
}

#[tokio::test]
async fn test_list_documents() {
    let p = SsmProvider::new();
    for name in ["DocA", "DocB"] {
        p.dispatch(&make_ctx(
            "CreateDocument",
            json!({ "Name": name, "Content": "{}", "DocumentType": "Command" }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("ListDocuments", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let docs = b["DocumentIdentifiers"].as_array().unwrap();
    assert_eq!(docs.len(), 2);
}

#[tokio::test]
async fn test_delete_document() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "CreateDocument",
        json!({ "Name": "DelDoc", "Content": "{}", "DocumentType": "Command" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DeleteDocument", json!({ "Name": "DelDoc" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let desc_resp = p
        .dispatch(&make_ctx("DescribeDocument", json!({ "Name": "DelDoc" })))
        .await
        .unwrap();
    assert_eq!(desc_resp.status_code, 400);
}

// ---------------------------------------------------------------------------
// SendCommand / ListCommands / GetCommandInvocation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_command() {
    let p = SsmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "SendCommand",
            json!({
                "DocumentName": "AWS-RunShellScript",
                "InstanceIds": ["i-abc123", "i-def456"],
                "Parameters": { "commands": ["echo hello"] },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(b["Command"]["CommandId"].as_str().is_some());
    assert_eq!(b["Command"]["DocumentName"], "AWS-RunShellScript");
    assert_eq!(b["Command"]["Status"], "Success");
}

#[tokio::test]
async fn test_list_commands() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "SendCommand",
        json!({
            "DocumentName": "AWS-RunShellScript",
            "InstanceIds": ["i-111"],
        }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "SendCommand",
        json!({
            "DocumentName": "AWS-RunPowerShellScript",
            "InstanceIds": ["i-222"],
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("ListCommands", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Commands"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_list_commands_filter_by_instance() {
    let p = SsmProvider::new();
    p.dispatch(&make_ctx(
        "SendCommand",
        json!({ "DocumentName": "Doc1", "InstanceIds": ["i-filter-me"] }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "SendCommand",
        json!({ "DocumentName": "Doc2", "InstanceIds": ["i-other"] }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "ListCommands",
            json!({ "InstanceId": "i-filter-me" }),
        ))
        .await
        .unwrap();
    let b = body(&resp);
    assert_eq!(b["Commands"].as_array().unwrap().len(), 1);
    assert_eq!(b["Commands"][0]["DocumentName"], "Doc1");
}

#[tokio::test]
async fn test_get_command_invocation() {
    let p = SsmProvider::new();
    let send_resp = p
        .dispatch(&make_ctx(
            "SendCommand",
            json!({
                "DocumentName": "AWS-RunShellScript",
                "InstanceIds": ["i-target"],
            }),
        ))
        .await
        .unwrap();
    let command_id = body(&send_resp)["Command"]["CommandId"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "GetCommandInvocation",
            json!({
                "CommandId": command_id,
                "InstanceId": "i-target",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["InstanceId"], "i-target");
    assert_eq!(b["Status"], "Success");
    assert_eq!(b["ResponseCode"], 0);
}

#[tokio::test]
async fn test_get_command_invocation_not_found() {
    let p = SsmProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetCommandInvocation",
            json!({
                "CommandId": "nonexistent-command-id",
                "InstanceId": "i-abc",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("InvocationDoesNotExist")
    );
}
