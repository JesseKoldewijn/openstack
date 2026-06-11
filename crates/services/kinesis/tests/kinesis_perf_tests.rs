/// Performance tests for Kinesis provider.
///
/// These test timing and throughput of core operations — tagging, put/get
/// records, and stream listing — to catch regressions introduced by lock
/// contention or linear scans.
///
/// Run with: `cargo test -p openstack-kinesis`
use std::collections::HashMap;
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use openstack_kinesis::KinesisProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "kinesis".to_string(),
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

async fn create_stream(p: &KinesisProvider, name: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateStream",
            json!({ "StreamName": name, "ShardCount": 1 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

async fn delete_stream(p: &KinesisProvider, name: &str) {
    let resp = p
        .dispatch(&make_ctx("DeleteStream", json!({ "StreamName": name })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

// ---------------------------------------------------------------------------
// Perf 1 — AddTags + ListTags round-trip latency
//
// Adding and then listing tags for a stream must complete in under 500 ms even
// for 50 tag keys (well within the AWS 50-tag limit).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_tagging_round_trip() {
    let p = KinesisProvider::new();
    create_stream(&p, "perf-tag-stream").await;

    // Build 50 tags
    let tags: serde_json::Map<String, Value> = (0..50)
        .map(|i| (format!("key-{i:02}"), Value::String(format!("val-{i:02}"))))
        .collect();

    let start = Instant::now();
    let add_resp = p
        .dispatch(&make_ctx(
            "AddTagsToStream",
            json!({ "StreamName": "perf-tag-stream", "Tags": tags }),
        ))
        .await
        .unwrap();
    assert_eq!(add_resp.status_code, 200);

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "perf-tag-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let elapsed = start.elapsed();

    let body: Value = serde_json::from_slice(list_resp.body.as_bytes()).unwrap();
    let tag_count = body["Tags"].as_array().unwrap().len();
    assert_eq!(tag_count, 50, "expected 50 tags, got {tag_count}");

    assert!(
        elapsed.as_millis() < 500,
        "tagging round-trip took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — PutRecords throughput: 500 records in under 2 s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_put_records_throughput() {
    let p = KinesisProvider::new();
    create_stream(&p, "perf-put-stream").await;

    let n = 500usize;
    let records: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "PartitionKey": format!("pk-{i}"),
                "Data": B64.encode(format!("record-data-{i}").as_bytes()),
            })
        })
        .collect();

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "PutRecords",
            json!({ "StreamName": "perf-put-stream", "Records": records }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    assert_eq!(body["FailedRecordCount"], 0);
    let returned = body["Records"].as_array().unwrap().len();
    assert_eq!(returned, n, "expected {n} returned records, got {returned}");

    assert!(
        elapsed.as_millis() < 2000,
        "PutRecords({n}) took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 3 — ListStreams with 100 streams completes in under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_list_streams_many() {
    let p = KinesisProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_stream(&p, &format!("list-perf-stream-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListStreams", json!({})))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    let count = body["StreamNames"].as_array().unwrap().len();
    assert!(count >= n, "expected at least {n} streams, got {count}");

    assert!(
        elapsed.as_millis() < 500,
        "ListStreams({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 4 — RemoveTags round-trip: add 50 tags then remove them all in <500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_remove_tags_round_trip() {
    let p = KinesisProvider::new();
    create_stream(&p, "perf-rm-tag-stream").await;

    // Add 50 tags first
    let tags: serde_json::Map<String, Value> = (0..50)
        .map(|i| {
            (
                format!("rm-key-{i:02}"),
                Value::String(format!("val-{i:02}")),
            )
        })
        .collect();
    let add_resp = p
        .dispatch(&make_ctx(
            "AddTagsToStream",
            json!({ "StreamName": "perf-rm-tag-stream", "Tags": tags }),
        ))
        .await
        .unwrap();
    assert_eq!(add_resp.status_code, 200);

    let tag_keys: Vec<String> = (0..50).map(|i| format!("rm-key-{i:02}")).collect();

    let start = Instant::now();
    let rm_resp = p
        .dispatch(&make_ctx(
            "RemoveTagsFromStream",
            json!({ "StreamName": "perf-rm-tag-stream", "TagKeys": tag_keys }),
        ))
        .await
        .unwrap();
    assert_eq!(rm_resp.status_code, 200);

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "perf-rm-tag-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let elapsed = start.elapsed();

    let body: Value = serde_json::from_slice(list_resp.body.as_bytes()).unwrap();
    let remaining = body["Tags"].as_array().unwrap().len();
    assert_eq!(
        remaining, 0,
        "all tags should have been removed, got {remaining}"
    );

    assert!(
        elapsed.as_millis() < 500,
        "RemoveTags+ListTags round-trip took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_and_delete_stream_round_trip() {
    let p = KinesisProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let name = format!("perf-delete-stream-{i:03}");
        create_stream(&p, &name).await;
        delete_stream(&p, &name).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateStream/DeleteStream x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
