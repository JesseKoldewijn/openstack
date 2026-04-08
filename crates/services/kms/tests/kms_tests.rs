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
async fn test_delete_alias() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    p.dispatch(&make_ctx(
        "CreateAlias",
        json!({
            "AliasName": "alias/delete-me",
            "TargetKeyId": key_id,
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteAlias",
            json!({ "AliasName": "alias/delete-me" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("ListAliases", json!({})))
        .await
        .unwrap();
    let aliases = body(&resp)["Aliases"].as_array().unwrap().clone();
    assert!(!aliases.iter().any(|a| a["AliasName"] == "alias/delete-me"));
}

#[tokio::test]
async fn test_tag_list_and_untag_resource() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let resp = p
        .dispatch(&make_ctx(
            "TagResource",
            json!({
                "KeyId": key_id,
                "Tags": [
                    { "TagKey": "env", "TagValue": "prod" },
                    { "TagKey": "team", "TagValue": "platform" }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("ListResourceTags", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let tags = body(&resp)["Tags"].as_array().unwrap().clone();
    assert_eq!(tags.len(), 2);
    assert!(
        tags.iter()
            .any(|t| t["TagKey"] == "env" && t["TagValue"] == "prod")
    );

    let resp = p
        .dispatch(&make_ctx(
            "UntagResource",
            json!({
                "KeyId": key_id,
                "TagKeys": ["env"]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("ListResourceTags", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    let tags = body(&resp)["Tags"].as_array().unwrap().clone();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["TagKey"], "team");
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
async fn test_cancel_key_deletion_restores_enabled_state() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    p.dispatch(&make_ctx(
        "ScheduleKeyDeletion",
        json!({
            "KeyId": key_id,
            "PendingWindowInDays": 7,
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("CancelKeyDeletion", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_id })))
        .await
        .unwrap();
    assert_eq!(body(&resp)["KeyMetadata"]["KeyState"], "Enabled");
}

#[tokio::test]
async fn test_describe_key_not_found() {
    let p = KmsProvider::new();
    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": "nonexistent" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert_eq!(resp.content_type, "application/json");
    let payload = body(&resp);
    assert_eq!(payload["__type"], "NotFoundException");
    assert_eq!(
        payload["message"],
        "Key 'arn:aws:kms:us-east-1:000000000000:key/nonexistent' does not exist"
    );
}

#[tokio::test]
async fn test_describe_key_with_full_arn_not_found_message_not_double_wrapped() {
    let p = KmsProvider::new();
    let key_arn = "arn:aws:kms:us-east-1:000000000000:key/nonexistent";
    let resp = p
        .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_arn })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let payload = body(&resp);
    assert_eq!(payload["__type"], "NotFoundException");
    assert_eq!(
        payload["message"],
        "Key 'arn:aws:kms:us-east-1:000000000000:key/nonexistent' does not exist"
    );
}

// ---------------------------------------------------------------------------
// Sign / Verify (HMAC-SHA256 based)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sign_returns_signature() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let message = B64.encode(b"hello world");
    let resp = p
        .dispatch(&make_ctx(
            "Sign",
            json!({
                "KeyId": key_id,
                "Message": message,
                "MessageType": "RAW",
                "SigningAlgorithm": "HMAC_SHA256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert!(b["Signature"].as_str().is_some(), "Signature field missing");
    // KeyId in response is the full ARN
    let key_id_str = b["KeyId"].as_str().unwrap();
    assert!(
        key_id_str.contains(&key_id),
        "Expected KeyId ARN to contain {key_id}, got {key_id_str}"
    );
    assert_eq!(b["SigningAlgorithm"], "HMAC_SHA_256");
}

#[tokio::test]
async fn test_verify_valid_signature() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let message = B64.encode(b"sign me please");

    // Sign first
    let sign_resp = p
        .dispatch(&make_ctx(
            "Sign",
            json!({
                "KeyId": key_id,
                "Message": message,
                "MessageType": "RAW",
                "SigningAlgorithm": "HMAC_SHA256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(sign_resp.status_code, 200, "{}", body_str(&sign_resp));
    let signature = body(&sign_resp)["Signature"].as_str().unwrap().to_string();

    // Verify with correct signature
    let resp = p
        .dispatch(&make_ctx(
            "Verify",
            json!({
                "KeyId": key_id,
                "Message": message,
                "Signature": signature,
                "MessageType": "RAW",
                "SigningAlgorithm": "HMAC_SHA256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["SignatureValid"], true);
    // KeyId in response is the full ARN
    let key_id_str = b["KeyId"].as_str().unwrap();
    assert!(
        key_id_str.contains(&key_id),
        "Expected KeyId ARN to contain {key_id}, got {key_id_str}"
    );
}

#[tokio::test]
async fn test_verify_invalid_signature() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let message = B64.encode(b"some message");
    let bad_signature = B64.encode(b"definitely-not-the-real-signature");

    let resp = p
        .dispatch(&make_ctx(
            "Verify",
            json!({
                "KeyId": key_id,
                "Message": message,
                "Signature": bad_signature,
                "MessageType": "RAW",
                "SigningAlgorithm": "HMAC_SHA256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["__type"], "KMSInvalidSignatureException");
}

#[tokio::test]
async fn test_sign_key_not_found() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let p = KmsProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "Sign",
            json!({
                "KeyId": "nonexistent-key",
                "Message": B64.encode(b"msg"),
                "MessageType": "RAW",
                "SigningAlgorithm": "HMAC_SHA256",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert_eq!(body(&resp)["__type"], "NotFoundException");
}

#[tokio::test]
async fn test_sign_verify_deterministic() {
    // Signing the same message with the same key must produce the same signature.
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;
    let message = B64.encode(b"deterministic");

    let sig1 = {
        let r = p
            .dispatch(&make_ctx(
                "Sign",
                json!({
                    "KeyId": key_id,
                    "Message": message,
                    "MessageType": "RAW",
                    "SigningAlgorithm": "HMAC_SHA256",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(r.status_code, 200, "Sign failed: {}", body_str(&r));
        body(&r)["Signature"].as_str().unwrap().to_string()
    };
    let sig2 = {
        let r = p
            .dispatch(&make_ctx(
                "Sign",
                json!({
                    "KeyId": key_id,
                    "Message": message,
                    "MessageType": "RAW",
                    "SigningAlgorithm": "HMAC_SHA256",
                }),
            ))
            .await
            .unwrap();
        assert_eq!(r.status_code, 200, "Sign failed: {}", body_str(&r));
        body(&r)["Signature"].as_str().unwrap().to_string()
    };
    assert_eq!(sig1, sig2, "HMAC-based Sign must be deterministic");
}
