/// Performance tests for Firehose provider.
///
/// These cover delivery stream creation plus record write and list/describe
/// control-plane paths.
use std::collections::HashMap;
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use openstack_firehose::FirehoseProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "firehose".to_string(),
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

async fn create_stream(p: &FirehoseProvider, name: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": name,
                "S3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::my-test-bucket",
                    "RoleARN": "arn:aws:iam::000000000000:role/firehose-role"
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_delivery_stream_throughput() {
    let p = FirehoseProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_stream(&p, &format!("perf-firehose-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateDeliveryStream x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_delivery_streams_many() {
    let p = FirehoseProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_stream(&p, &format!("perf-list-firehose-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx("ListDeliveryStreams", json!({})))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    assert!(body(&resp)["DeliveryStreamNames"].as_array().unwrap().len() >= n);

    assert!(
        elapsed.as_millis() < 500,
        "ListDeliveryStreams({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_put_record_batch_round_trip() {
    let p = FirehoseProvider::new();
    create_stream(&p, "perf-batch-stream").await;

    let start = Instant::now();
    for i in 0..100usize {
        let resp = p
            .dispatch(&make_ctx(
                "PutRecordBatch",
                json!({
                    "DeliveryStreamName": "perf-batch-stream",
                    "Records": [
                        { "Data": B64.encode(format!("r1-{i}").as_bytes()) },
                        { "Data": B64.encode(format!("r2-{i}").as_bytes()) }
                    ]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
        assert_eq!(body(&resp)["FailedPutCount"], 0);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "PutRecordBatch x100 took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}
