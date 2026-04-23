use std::collections::HashMap;

use bytes::Bytes;
use openstack_cloudtrail::CloudTrailProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "cloudtrail".to_string(),
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

fn body_json(resp: &openstack_service_framework::traits::DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

// ---------------------------------------------------------------------------
// Trail CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_trail() {
    let p = CloudTrailProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateTrail",
            json!({
                "Name": "my-trail",
                "S3BucketName": "my-log-bucket",
                "IncludeGlobalServiceEvents": true,
                "IsMultiRegionTrail": false,
                "EnableLogFileValidation": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, "application/x-amz-json-1.1");
    let b = body_json(&resp);
    assert_eq!(b["Name"], "my-trail");
    assert_eq!(b["S3BucketName"], "my-log-bucket");
    assert_eq!(b["LogFileValidationEnabled"], true);
    let arn = b["TrailARN"].as_str().unwrap();
    assert!(arn.contains("cloudtrail"));
    assert!(arn.contains("my-trail"));
}

#[tokio::test]
async fn test_create_trail_duplicate_fails() {
    let p = CloudTrailProvider::new();
    let body = json!({ "Name": "dup-trail", "S3BucketName": "bucket" });
    p.dispatch(&make_ctx("CreateTrail", body.clone()))
        .await
        .unwrap();
    let resp = p.dispatch(&make_ctx("CreateTrail", body)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("TrailAlreadyExistsException")
    );
}

#[tokio::test]
async fn test_create_trail_missing_bucket_fails() {
    let p = CloudTrailProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateTrail",
            json!({ "Name": "no-bucket-trail" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("InvalidS3BucketNameException")
    );
}

#[tokio::test]
async fn test_describe_trails() {
    let p = CloudTrailProvider::new();
    for (name, bucket) in [("trail-a", "bucket-a"), ("trail-b", "bucket-b")] {
        p.dispatch(&make_ctx(
            "CreateTrail",
            json!({ "Name": name, "S3BucketName": bucket }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx("DescribeTrails", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    let trails = b["trailList"].as_array().unwrap();
    assert_eq!(trails.len(), 2);
}

#[tokio::test]
async fn test_describe_trails_filtered() {
    let p = CloudTrailProvider::new();
    for (name, bucket) in [("filter-trail-a", "ba"), ("filter-trail-b", "bb")] {
        p.dispatch(&make_ctx(
            "CreateTrail",
            json!({ "Name": name, "S3BucketName": bucket }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "DescribeTrails",
            json!({ "trailNameList": ["filter-trail-a"] }),
        ))
        .await
        .unwrap();
    let trails = body_json(&resp)["trailList"].as_array().unwrap().clone();
    assert_eq!(trails.len(), 1);
    assert_eq!(trails[0]["Name"], "filter-trail-a");
}

#[tokio::test]
async fn test_get_trail() {
    let p = CloudTrailProvider::new();
    p.dispatch(&make_ctx(
        "CreateTrail",
        json!({ "Name": "get-trail", "S3BucketName": "gb" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("GetTrail", json!({ "Name": "get-trail" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["Trail"]["Name"], "get-trail");
}

#[tokio::test]
async fn test_get_trail_not_found() {
    let p = CloudTrailProvider::new();
    let resp = p
        .dispatch(&make_ctx("GetTrail", json!({ "Name": "nonexistent" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 404);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("TrailNotFoundException")
    );
}

#[tokio::test]
async fn test_delete_trail() {
    let p = CloudTrailProvider::new();
    p.dispatch(&make_ctx(
        "CreateTrail",
        json!({ "Name": "del-trail", "S3BucketName": "db" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DeleteTrail", json!({ "Name": "del-trail" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let desc_resp = p
        .dispatch(&make_ctx("DescribeTrails", json!({})))
        .await
        .unwrap();
    let trails = body_json(&desc_resp)["trailList"]
        .as_array()
        .unwrap()
        .clone();
    assert!(!trails.iter().any(|t| t["Name"] == "del-trail"));
}

#[tokio::test]
async fn test_delete_trail_not_found() {
    let p = CloudTrailProvider::new();
    let resp = p
        .dispatch(&make_ctx("DeleteTrail", json!({ "Name": "ghost" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(
        body_json(&resp)["__type"]
            .as_str()
            .unwrap()
            .contains("TrailNotFoundException")
    );
}

#[tokio::test]
async fn test_update_trail() {
    let p = CloudTrailProvider::new();
    p.dispatch(&make_ctx(
        "CreateTrail",
        json!({ "Name": "upd-trail", "S3BucketName": "old-bucket" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "UpdateTrail",
            json!({
                "Name": "upd-trail",
                "S3BucketName": "new-bucket",
                "IsMultiRegionTrail": true,
                "EnableLogFileValidation": true,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert_eq!(b["S3BucketName"], "new-bucket");
    assert_eq!(b["IsMultiRegionTrail"], true);
    assert_eq!(b["LogFileValidationEnabled"], true);
}

// ---------------------------------------------------------------------------
// StartLogging / StopLogging / GetTrailStatus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_start_and_stop_logging() {
    let p = CloudTrailProvider::new();
    p.dispatch(&make_ctx(
        "CreateTrail",
        json!({ "Name": "log-trail", "S3BucketName": "lb" }),
    ))
    .await
    .unwrap();

    // Initially not logging
    let status_resp = p
        .dispatch(&make_ctx("GetTrailStatus", json!({ "Name": "log-trail" })))
        .await
        .unwrap();
    assert_eq!(body_json(&status_resp)["IsLogging"], false);

    // Start logging
    let start_resp = p
        .dispatch(&make_ctx("StartLogging", json!({ "Name": "log-trail" })))
        .await
        .unwrap();
    assert_eq!(start_resp.status_code, 200);

    let status_resp2 = p
        .dispatch(&make_ctx("GetTrailStatus", json!({ "Name": "log-trail" })))
        .await
        .unwrap();
    assert_eq!(body_json(&status_resp2)["IsLogging"], true);

    // Stop logging
    let stop_resp = p
        .dispatch(&make_ctx("StopLogging", json!({ "Name": "log-trail" })))
        .await
        .unwrap();
    assert_eq!(stop_resp.status_code, 200);

    let status_resp3 = p
        .dispatch(&make_ctx("GetTrailStatus", json!({ "Name": "log-trail" })))
        .await
        .unwrap();
    assert_eq!(body_json(&status_resp3)["IsLogging"], false);
}

// ---------------------------------------------------------------------------
// LookupEvents
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lookup_events_empty() {
    let p = CloudTrailProvider::new();
    let resp = p
        .dispatch(&make_ctx("LookupEvents", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body_json(&resp);
    assert!(b["Events"].is_array());
    assert_eq!(b["Events"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_add_and_list_tags() {
    let p = CloudTrailProvider::new();
    let create_resp = p
        .dispatch(&make_ctx(
            "CreateTrail",
            json!({ "Name": "tag-trail", "S3BucketName": "tb" }),
        ))
        .await
        .unwrap();
    let trail_arn = body_json(&create_resp)["TrailARN"]
        .as_str()
        .unwrap()
        .to_string();

    p.dispatch(&make_ctx(
        "AddTags",
        json!({
            "ResourceId": trail_arn,
            "TagsList": [
                {"Key": "env", "Value": "prod"},
                {"Key": "owner", "Value": "team-a"},
            ]
        }),
    ))
    .await
    .unwrap();

    let list_resp = p
        .dispatch(&make_ctx(
            "ListTags",
            json!({ "ResourceIdList": [trail_arn] }),
        ))
        .await
        .unwrap();
    assert_eq!(list_resp.status_code, 200);
    let b = body_json(&list_resp);
    let tag_list = b["ResourceTagList"][0]["TagsList"].as_array().unwrap();
    assert_eq!(tag_list.len(), 2);
    assert!(
        tag_list
            .iter()
            .any(|t| t["Key"] == "env" && t["Value"] == "prod")
    );
}
