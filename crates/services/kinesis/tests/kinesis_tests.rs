use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use bytes::Bytes;
use openstack_kinesis::KinesisProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
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

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("response body is valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

async fn create_stream(p: &KinesisProvider, name: &str, shard_count: u32) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateStream",
            json!({ "StreamName": name, "ShardCount": shard_count }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        200,
        "CreateStream failed: {}",
        body_str(&resp)
    );
}

// ---------------------------------------------------------------------------
// Tag operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_add_tags_to_stream() {
    let p = KinesisProvider::new();
    create_stream(&p, "tag-stream", 1).await;

    let resp = p
        .dispatch(&make_ctx(
            "AddTagsToStream",
            json!({
                "StreamName": "tag-stream",
                "Tags": { "env": "prod", "team": "backend" },
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_list_tags_for_stream() {
    let p = KinesisProvider::new();
    create_stream(&p, "list-tag-stream", 1).await;

    p.dispatch(&make_ctx(
        "AddTagsToStream",
        json!({ "StreamName": "list-tag-stream", "Tags": { "k1": "v1", "k2": "v2" } }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "list-tag-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let tags = b["Tags"].as_array().unwrap();
    assert_eq!(tags.len(), 2);
    let found_k1 = tags.iter().any(|t| t["Key"] == "k1" && t["Value"] == "v1");
    let found_k2 = tags.iter().any(|t| t["Key"] == "k2" && t["Value"] == "v2");
    assert!(found_k1, "k1=v1 tag not found");
    assert!(found_k2, "k2=v2 tag not found");
    assert_eq!(b["HasMoreTags"], false);
}

#[tokio::test]
async fn test_remove_tags_from_stream() {
    let p = KinesisProvider::new();
    create_stream(&p, "rm-tag-stream", 1).await;

    p.dispatch(&make_ctx(
        "AddTagsToStream",
        json!({ "StreamName": "rm-tag-stream", "Tags": { "keep": "yes", "remove": "me" } }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "RemoveTagsFromStream",
            json!({ "StreamName": "rm-tag-stream", "TagKeys": ["remove"] }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "rm-tag-stream" }),
        ))
        .await
        .unwrap();
    let tags = body(&list_resp)["Tags"].as_array().unwrap().clone();
    assert_eq!(tags.len(), 1, "should have 1 tag left, got: {tags:?}");
    assert_eq!(tags[0]["Key"], "keep");
}

#[tokio::test]
async fn test_add_tags_stream_not_found() {
    let p = KinesisProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "AddTagsToStream",
            json!({ "StreamName": "nonexistent", "Tags": { "k": "v" } }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["__type"], "ResourceNotFoundException");
}

#[tokio::test]
async fn test_add_tags_overwrites_existing_key() {
    let p = KinesisProvider::new();
    create_stream(&p, "overwrite-tag-stream", 1).await;

    p.dispatch(&make_ctx(
        "AddTagsToStream",
        json!({ "StreamName": "overwrite-tag-stream", "Tags": { "color": "red" } }),
    ))
    .await
    .unwrap();

    p.dispatch(&make_ctx(
        "AddTagsToStream",
        json!({ "StreamName": "overwrite-tag-stream", "Tags": { "color": "blue" } }),
    ))
    .await
    .unwrap();

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "overwrite-tag-stream" }),
        ))
        .await
        .unwrap();
    let tags = body(&list_resp)["Tags"].as_array().unwrap().clone();
    assert_eq!(tags.len(), 1, "should have exactly 1 tag after overwrite");
    assert_eq!(tags[0]["Value"], "blue");
}

#[tokio::test]
async fn test_create_and_list_streams() {
    let p = KinesisProvider::new();
    create_stream(&p, "my-stream", 2).await;

    let resp = p
        .dispatch(&make_ctx("ListStreams", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let names = b["StreamNames"].as_array().unwrap();
    assert!(names.iter().any(|n| n.as_str() == Some("my-stream")));
}

#[tokio::test]
async fn test_describe_stream() {
    let p = KinesisProvider::new();
    create_stream(&p, "test-stream", 3).await;

    let resp = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "test-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let desc = &b["StreamDescription"];
    assert_eq!(desc["StreamName"], "test-stream");
    assert_eq!(desc["StreamStatus"], "ACTIVE");
    let shards = desc["Shards"].as_array().unwrap();
    assert_eq!(shards.len(), 3);
}

#[tokio::test]
async fn test_delete_stream() {
    let p = KinesisProvider::new();
    create_stream(&p, "del-stream", 1).await;

    let resp = p
        .dispatch(&make_ctx(
            "DeleteStream",
            json!({ "StreamName": "del-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    // Should now be missing
    let resp2 = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "del-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status_code, 400);
}

#[tokio::test]
async fn test_put_record_missing_stream_matches_localstack_shape() {
    let p = KinesisProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "PutRecord",
            json!({
                "StreamName": "missing-stream",
                "PartitionKey": "pk",
                "Data": B64.encode(b"bench"),
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400, "{}", body_str(&resp));
    assert_eq!(resp.content_type, "application/json");
    let b = body(&resp);
    assert_eq!(b["__type"], "ResourceNotFoundException");
    assert_eq!(
        b["message"],
        "Stream arn arn:aws:kinesis:us-east-1:000000000000:stream/missing-stream not found"
    );
}

#[tokio::test]
async fn test_put_and_get_records() {
    let p = KinesisProvider::new();
    create_stream(&p, "data-stream", 1).await;

    // Get shard id
    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "data-stream" }),
        ))
        .await
        .unwrap();
    let desc_body = body(&desc_resp);
    let shard_id = desc_body["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .unwrap()
        .to_string();

    let data_b64 = B64.encode(b"hello kinesis");

    // PutRecord
    let put_resp = p
        .dispatch(&make_ctx(
            "PutRecord",
            json!({
                "StreamName": "data-stream",
                "PartitionKey": "pk1",
                "Data": data_b64,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(put_resp.status_code, 200, "{}", body_str(&put_resp));
    let put_body = body(&put_resp);
    let seq = put_body["SequenceNumber"].as_str().unwrap().to_string();
    assert!(!seq.is_empty());

    // GetShardIterator (TRIM_HORIZON)
    let iter_resp = p
        .dispatch(&make_ctx(
            "GetShardIterator",
            json!({
                "StreamName": "data-stream",
                "ShardId": shard_id,
                "ShardIteratorType": "TRIM_HORIZON",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(iter_resp.status_code, 200, "{}", body_str(&iter_resp));
    let iter_token = body(&iter_resp)["ShardIterator"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!iter_token.is_empty());

    // GetRecords
    let rec_resp = p
        .dispatch(&make_ctx(
            "GetRecords",
            json!({ "ShardIterator": iter_token }),
        ))
        .await
        .unwrap();
    assert_eq!(rec_resp.status_code, 200, "{}", body_str(&rec_resp));
    let rec_body = body(&rec_resp);
    let records = rec_body["Records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["Data"], data_b64);
    assert_eq!(records[0]["PartitionKey"], "pk1");
}

#[tokio::test]
async fn test_put_records_batch() {
    let p = KinesisProvider::new();
    create_stream(&p, "batch-stream", 2).await;

    let records = json!([
        { "PartitionKey": "k1", "Data": B64.encode(b"rec1") },
        { "PartitionKey": "k2", "Data": B64.encode(b"rec2") },
        { "PartitionKey": "k3", "Data": B64.encode(b"rec3") },
    ]);
    let resp = p
        .dispatch(&make_ctx(
            "PutRecords",
            json!({ "StreamName": "batch-stream", "Records": records }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(b["FailedRecordCount"], 0);
    let results = b["Records"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    for r in results {
        assert!(r["SequenceNumber"].as_str().is_some());
    }
}

#[tokio::test]
async fn test_shard_iterator_latest() {
    let p = KinesisProvider::new();
    create_stream(&p, "latest-stream", 1).await;

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "latest-stream" }),
        ))
        .await
        .unwrap();
    let shard_id = body(&desc_resp)["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .unwrap()
        .to_string();

    // Put a record first
    p.dispatch(&make_ctx(
        "PutRecord",
        json!({ "StreamName": "latest-stream", "PartitionKey": "p", "Data": B64.encode(b"old") }),
    ))
    .await
    .unwrap();

    // LATEST iterator should not return the old record
    let iter_resp = p
        .dispatch(&make_ctx(
            "GetShardIterator",
            json!({
                "StreamName": "latest-stream",
                "ShardId": shard_id,
                "ShardIteratorType": "LATEST",
            }),
        ))
        .await
        .unwrap();
    let iter_token = body(&iter_resp)["ShardIterator"]
        .as_str()
        .unwrap()
        .to_string();

    let rec_resp = p
        .dispatch(&make_ctx(
            "GetRecords",
            json!({ "ShardIterator": iter_token }),
        ))
        .await
        .unwrap();
    let records = body(&rec_resp)["Records"].as_array().unwrap().clone();
    assert_eq!(
        records.len(),
        0,
        "LATEST should return no pre-existing records"
    );
}

#[tokio::test]
async fn test_split_shard() {
    let p = KinesisProvider::new();
    create_stream(&p, "split-stream", 1).await;

    let desc_resp = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "split-stream" }),
        ))
        .await
        .unwrap();
    let shard_id = body(&desc_resp)["StreamDescription"]["Shards"][0]["ShardId"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = p
        .dispatch(&make_ctx(
            "SplitShard",
            json!({
                "StreamName": "split-stream",
                "ShardToSplit": shard_id,
                "NewStartingHashKey": "170141183460469231731687303715884105728",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    // After split there should be 2 open shards and 1 closed
    let desc2 = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "split-stream" }),
        ))
        .await
        .unwrap();
    let shards = body(&desc2)["StreamDescription"]["Shards"]
        .as_array()
        .unwrap()
        .clone();
    // 3 total shards (1 closed + 2 new open)
    assert_eq!(shards.len(), 3);
}

#[tokio::test]
async fn test_retention_period() {
    let p = KinesisProvider::new();
    create_stream(&p, "ret-stream", 1).await;

    let resp = p
        .dispatch(&make_ctx(
            "IncreaseStreamRetentionPeriod",
            json!({ "StreamName": "ret-stream", "RetentionPeriodHours": 168 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));

    let desc = p
        .dispatch(&make_ctx(
            "DescribeStream",
            json!({ "StreamName": "ret-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(
        body(&desc)["StreamDescription"]["RetentionPeriodHours"],
        168
    );
}

#[tokio::test]
async fn test_list_shards() {
    let p = KinesisProvider::new();
    create_stream(&p, "ls-stream", 4).await;

    let resp = p
        .dispatch(&make_ctx(
            "ListShards",
            json!({ "StreamName": "ls-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let shards = body(&resp)["Shards"].as_array().unwrap().clone();
    assert_eq!(shards.len(), 4);
}

#[tokio::test]
async fn test_describe_stream_summary() {
    let p = KinesisProvider::new();
    create_stream(&p, "summary-stream", 2).await;

    let resp = p
        .dispatch(&make_ctx(
            "DescribeStreamSummary",
            json!({ "StreamName": "summary-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    assert_eq!(
        b["StreamDescriptionSummary"]["StreamName"],
        "summary-stream"
    );
    assert_eq!(b["StreamDescriptionSummary"]["OpenShardCount"], 2);
}

#[tokio::test]
async fn test_list_tags_stream_not_found() {
    let p = KinesisProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "ListTagsForStream",
            json!({ "StreamName": "nonexistent-stream" }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        400,
        "expected 400 for missing stream, got {}: {}",
        resp.status_code,
        body_str(&resp)
    );
    let b = body(&resp);
    assert_eq!(b["__type"], "ResourceNotFoundException");
}

#[tokio::test]
async fn test_remove_tags_stream_not_found() {
    let p = KinesisProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "RemoveTagsFromStream",
            json!({ "StreamName": "nonexistent-stream", "TagKeys": ["k1"] }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status_code,
        400,
        "expected 400 for missing stream, got {}: {}",
        resp.status_code,
        body_str(&resp)
    );
    let b = body(&resp);
    assert_eq!(b["__type"], "ResourceNotFoundException");
}
