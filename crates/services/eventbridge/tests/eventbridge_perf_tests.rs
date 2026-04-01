/// Performance tests for the EventBridge provider.
///
/// Run with: `cargo test -p openstack-eventbridge`
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_eventbridge::EventBridgeProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "eventbridge".to_string(),
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

// ---------------------------------------------------------------------------
// Perf 1 — PutEvents × 200 single-entry batches in under 2 s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_put_events_throughput() {
    let p = EventBridgeProvider::new();
    let n = 200usize;

    let start = Instant::now();
    for i in 0..n {
        let resp = p
            .dispatch(&make_ctx(
                "PutEvents",
                json!({
                    "Entries": [
                        {
                            "Source": format!("perf.source.{i}"),
                            "DetailType": "PerfEvent",
                            "Detail": format!(r#"{{"i":{i}}}"#),
                        }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200, "PutEvents iter {i} failed");
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "PutEvents×{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — PutEvents large batch (100 entries) in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_put_events_large_batch() {
    let p = EventBridgeProvider::new();
    let n = 100usize;
    let entries: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "Source": "perf.batch",
                "DetailType": "BatchItem",
                "Detail": format!(r#"{{"seq":{i}}}"#),
            })
        })
        .collect();

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("PutEvents", json!({ "Entries": entries })))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let b: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    assert_eq!(b["FailedEntryCount"], 0);
    assert_eq!(b["Entries"].as_array().unwrap().len(), n);

    assert!(
        elapsed.as_millis() < 500,
        "PutEvents batch({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
