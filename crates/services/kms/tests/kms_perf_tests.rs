/// Performance tests for the KMS provider.
///
/// Run with: `cargo test -p openstack-kms`
use std::collections::HashMap;
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
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

async fn create_key(p: &KmsProvider) -> String {
    let resp = p
        .dispatch(&make_ctx("CreateKey", json!({ "Description": "perf-key" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    b["KeyMetadata"]["KeyId"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Perf 1 — Sign × 100 in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_sign_throughput() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;
    let message = B64.encode(b"perf test message");

    let n = 100usize;
    let start = Instant::now();
    for _ in 0..n {
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
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "Sign×{n} took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — Sign + Verify round-trip × 50 in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_sign_verify_round_trip() {
    let p = KmsProvider::new();
    let key_id = create_key(&p).await;

    let n = 50usize;
    let start = Instant::now();
    for i in 0..n {
        let message = B64.encode(format!("message-{i}").as_bytes());

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
        assert_eq!(sign_resp.status_code, 200);
        let sig: Value = serde_json::from_slice(sign_resp.body.as_bytes()).unwrap();
        let signature = sig["Signature"].as_str().unwrap().to_string();

        let verify_resp = p
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
        assert_eq!(verify_resp.status_code, 200);
        let vb: Value = serde_json::from_slice(verify_resp.body.as_bytes()).unwrap();
        assert_eq!(
            vb["SignatureValid"], true,
            "iteration {i}: signature not valid"
        );
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "Sign+Verify round-trip×{n} took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 3 — CreateKey + DescribeKey round-trip × 100 in under 1500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_create_and_describe_key_round_trip() {
    let p = KmsProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "CreateKey",
                json!({ "Description": format!("perf-desc-key-{i}") }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
        let b: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
        let key_id = b["KeyMetadata"]["KeyId"].as_str().unwrap().to_string();

        let desc = p
            .dispatch(&make_ctx("DescribeKey", json!({ "KeyId": key_id })))
            .await
            .unwrap();
        assert_eq!(desc.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "CreateKey+DescribeKey round-trip×{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}
