use std::collections::HashMap;

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
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

async fn create_stream(p: &FirehoseProvider, name: &str) -> String {
    let resp = p
        .dispatch(&make_ctx(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": name,
                "S3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::my-test-bucket",
                    "RoleARN": "arn:aws:iam::000000000000:role/firehose-role",
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "CreateDeliveryStream failed: {}",
        body_str(&resp)
    );
    body(&resp)["DeliveryStreamARN"]
        .as_str()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_list_delivery_streams() {
    let p = FirehoseProvider::new();
    create_stream(&p, "my-stream").await;

    let resp = p
        .dispatch(&make_ctx("ListDeliveryStreams", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let names = b["DeliveryStreamNames"].as_array().unwrap();
    assert!(names.iter().any(|n| n.as_str() == Some("my-stream")));
}

#[tokio::test]
async fn test_describe_delivery_stream() {
    let p = FirehoseProvider::new();
    let arn = create_stream(&p, "desc-stream").await;

    let resp = p
        .dispatch(&make_ctx(
            "DescribeDeliveryStream",
            json!({ "DeliveryStreamName": "desc-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let desc = &b["DeliveryStreamDescription"];
    assert_eq!(desc["DeliveryStreamName"], "desc-stream");
    assert_eq!(desc["DeliveryStreamARN"], arn);
    assert_eq!(desc["DeliveryStreamStatus"], "ACTIVE");
}

#[tokio::test]
async fn test_delete_delivery_stream() {
    let p = FirehoseProvider::new();
    create_stream(&p, "del-stream").await;

    let resp = p
        .dispatch(&make_ctx(
            "DeleteDeliveryStream",
            json!({ "DeliveryStreamName": "del-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    // Should now 404
    let resp2 = p
        .dispatch(&make_ctx(
            "DescribeDeliveryStream",
            json!({ "DeliveryStreamName": "del-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status_code, 400);
}

#[tokio::test]
async fn test_put_record() {
    let p = FirehoseProvider::new();
    create_stream(&p, "put-stream").await;

    let data_b64 = B64.encode(b"hello firehose");
    let resp = p
        .dispatch(&make_ctx(
            "PutRecord",
            json!({
                "DeliveryStreamName": "put-stream",
                "Record": { "Data": data_b64 },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let record_id = b["RecordId"].as_str().unwrap();
    assert!(!record_id.is_empty());
}

#[tokio::test]
async fn test_put_record_batch() {
    let p = FirehoseProvider::new();
    create_stream(&p, "batch-stream").await;

    let records = json!([
        { "Data": B64.encode(b"rec1") },
        { "Data": B64.encode(b"rec2") },
        { "Data": B64.encode(b"rec3") },
    ]);
    let resp = p
        .dispatch(&make_ctx(
            "PutRecordBatch",
            json!({
                "DeliveryStreamName": "batch-stream",
                "Records": records,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["FailedPutCount"], 0);
    let responses = b["RequestResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 3);
    for r in responses {
        assert!(r["RecordId"].as_str().is_some());
    }
}

#[tokio::test]
async fn test_duplicate_stream_fails() {
    let p = FirehoseProvider::new();
    create_stream(&p, "dup-stream").await;

    let resp = p
        .dispatch(&make_ctx(
            "CreateDeliveryStream",
            json!({
                "DeliveryStreamName": "dup-stream",
                "S3DestinationConfiguration": {
                    "BucketARN": "arn:aws:s3:::bucket",
                    "RoleARN": "arn:aws:iam::000000000000:role/role",
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    let b = body(&resp);
    assert!(b["__type"].as_str().unwrap().contains("ResourceInUse"));
}
