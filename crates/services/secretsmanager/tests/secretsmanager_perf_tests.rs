/// Performance tests for Secrets Manager provider.
///
/// These cover secret creation, reads at scale, and update round-trips.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_secretsmanager::SecretsManagerProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "secretsmanager".to_string(),
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

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

async fn create_secret(p: &SecretsManagerProvider, name: &str, value: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateSecret",
            json!({ "Name": name, "SecretString": value }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_secret_throughput() {
    let p = SecretsManagerProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_secret(&p, &format!("perf-secret-{i:03}"), "value").await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateSecret x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_secret_value_many() {
    let p = SecretsManagerProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_secret(&p, &format!("perf-get-secret-{i:03}"), "value").await;
    }

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "GetSecretValue",
                json!({ "SecretId": format!("perf-get-secret-{i:03}") }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "GetSecretValue x{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_update_secret_round_trip() {
    let p = SecretsManagerProvider::new();
    create_secret(&p, "perf-update-secret", "v1").await;

    let start = Instant::now();
    for i in 0..100usize {
        let resp = p
            .dispatch(&make_ctx(
                "UpdateSecret",
                json!({
                    "SecretId": "perf-update-secret",
                    "SecretString": format!("v{i}"),
                    "Description": format!("desc-{i}")
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    let resp = p
        .dispatch(&make_ctx(
            "GetSecretValue",
            json!({ "SecretId": "perf-update-secret" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(body(&resp)["SecretString"], "v99");

    assert!(
        elapsed.as_millis() < 1500,
        "UpdateSecret x100 took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}
