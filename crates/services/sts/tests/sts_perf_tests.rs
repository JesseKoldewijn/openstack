/// Performance tests for the STS provider.
///
/// Run with: `cargo test -p openstack-sts`
use std::collections::HashMap;
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_sts::StsProvider;

fn make_ctx(operation: &str, params: &[(&str, &str)]) -> RequestContext {
    let mut qp: HashMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    qp.insert("Action".to_string(), operation.to_string());
    RequestContext {
        service: "sts".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::json!({}),
        raw_body: None,
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: qp,
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

// ---------------------------------------------------------------------------
// Perf 1 — GetCallerIdentity throughput (50 calls in under 500 ms)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_get_caller_identity_throughput() {
    let p = StsProvider::new();
    let n = 50usize;

    let start = Instant::now();
    for _ in 0..n {
        let resp = p
            .dispatch(&make_ctx("GetCallerIdentity", &[]))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "{n} GetCallerIdentity calls took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — DecodeAuthorizationMessage with large payloads (1 KiB message)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_decode_authorization_message_large_payload() {
    let p = StsProvider::new();
    // Build a 1 KiB JSON-like string and base64-encode it.
    let payload = format!(r#"{{"version":"1.0","reason":"{}"}}"#, "x".repeat(1000));
    let encoded = B64.encode(payload.as_bytes());

    let start = Instant::now();
    for _ in 0..20 {
        let resp = p
            .dispatch(&make_ctx(
                "DecodeAuthorizationMessage",
                &[("EncodedMessage", encoded.as_str())],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "20 × DecodeAuthorizationMessage (1 KiB) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
