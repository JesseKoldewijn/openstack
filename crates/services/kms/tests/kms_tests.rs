use std::collections::HashMap;

use bytes::Bytes;
use openstack_kms::KmsProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "kms".to_string(),
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

fn body(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(&resp.body).expect("response body is valid JSON")
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

async fn create_key(p: &KmsProvider) -> String {
    let resp = p
        .dispatch(&make_ctx("CreateKey", json!({ "Description": "test-key" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    body(&resp)["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_create_key() {
    let p = KmsProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateKey", json!({ "Description": "my-key" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(b["KeyMetadata"]["KeyId"].as_str().is_some());
    assert_eq!(b["KeyMetadata"]["KeyState"], "Enabled");
}

#[tokio::test]
async fn test_describe_key() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body(&resp)["KeyMetadata"]["KeyId"], key_id);
}

#[tokio::test]
async fn test_list_keys() {
    let p = KmsProvider::new();
    create_key(&p).await;
    create_key(&p).await;

    let resp = p.dispatch(&make_ctx("ListKeys", json!({}))).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body(&resp)["Keys"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn test_enable_disable_key() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx("DisableKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(body(&resp)["KeyMetadata"]["KeyState"], "Disabled");

    let resp = p
        .dispatch(&make_ctx("EnableKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(body(&resp)["KeyMetadata"]["KeyState"], "Enabled");
}

#[tokio::test]
async fn test_encrypt_decrypt() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let plaintext = B64.encode("hello world");

    let resp = p
        .dispatch(&make_ctx(
            "Encrypt",
            json!({
                "KeyId": key_id,
                "Plaintext": plaintext,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let ciphertext = body(&resp)["CiphertextBlob"].as_str().unwrap().to_string();

    let resp = p
        .dispatch(&make_ctx(
            "Decrypt",
            json!({
                "CiphertextBlob": ciphertext,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert_eq!(body(&resp)["Plaintext"], plaintext);
}

#[tokio::test]
async fn test_generate_data_key() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx(
            "GenerateDataKey",
            json!({
                "KeyId": key_id,
                "KeySpec": "AES_256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(b["Plaintext"].as_str().is_some());
    assert!(b["CiphertextBlob"].as_str().is_some());
}

#[tokio::test]
async fn test_create_alias() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx(
            "CreateAlias",
            json!({
                "AliasName": "alias/my-alias",
                "TargetKeyId": key_id,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("ListAliases", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let aliases = body(&resp)["Aliases"].as_array().unwrap().clone();
    assert!(aliases.iter().any(|a| a["AliasName"] == "alias/my-alias"));
}

#[tokio::test]
async fn test_schedule_key_deletion() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx(
            "ScheduleKeyDeletion",
            json!({
                "KeyId": key_id,
                "PendingWindowInDays": 7,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    assert!(body(&resp)["DeletionDate"].as_i64().is_some());
}

#[tokio::test]
async fn test_describe_key_not_found() {
    let p = KmsProvider::new();
    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": "nonexistent" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    assert!(body_str(&resp).contains("NotFoundException"));
}
