use std::collections::HashMap;

use bytes::Bytes;
use openstack_secretsmanager::SecretsManagerProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "secretsmanager".to_string(),
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
async fn test_create_and_get_secret() {
    let p = SecretsManagerProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateSecret",
            json!({
                "Name": "my-secret",
                "SecretString": "s3cr3t!",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(b["ARN"].as_str().is_some());

    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({
                "SecretId": "my-secret",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["SecretString"], "s3cr3t!");
}

#[tokio::test]
async fn test_create_secret_duplicate() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "dup", "SecretString": "val" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateSecret",
            json!({ "Name": "dup", "SecretString": "val2" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ResourceExistsException"));
}

#[tokio::test]
async fn test_put_secret_value() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "mutable", "SecretString": "v1" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutSecretValue",
            json!({
                "SecretId": "mutable",
                "SecretString": "v2",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({ "SecretId": "mutable" }),
        ))
        .await
        .unwrap();
    assert_eq!(body(&resp)["SecretString"], "v2");
}

#[tokio::test]
async fn test_describe_secret() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "described", "SecretString": "x", "Description": "desc1" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeSecret",
            json!({ "SecretId": "described" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Description"], "desc1");
}

#[tokio::test]
async fn test_list_secrets() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "s1", "SecretString": "a" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "s2", "SecretString": "b" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("ListSecrets", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let list = body(&resp)["SecretList"].as_array().unwrap().clone();
    assert!(list.len() >= 2);
}

#[tokio::test]
async fn test_delete_secret() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "to-delete", "SecretString": "bye" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteSecret",
            json!({
                "SecretId": "to-delete",
                "ForceDeleteWithoutRecovery": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({ "SecretId": "to-delete" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
}

#[tokio::test]
async fn test_update_secret() {
    let p = SecretsManagerProvider::new();
    p.dispatch(&make_ctx(
        "CreateSecret",
        json!({ "Name": "updatable", "SecretString": "old" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateSecret",
            json!({
                "SecretId": "updatable",
                "SecretString": "new",
                "Description": "updated",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({ "SecretId": "updatable" }),
        ))
        .await
        .unwrap();
    assert_eq!(body(&resp)["SecretString"], "new");
}

#[tokio::test]
async fn test_get_secret_not_found() {
    let p = SecretsManagerProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({ "SecretId": "nonexistent" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ResourceNotFoundException"));
}
