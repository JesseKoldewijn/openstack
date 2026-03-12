use std::collections::HashMap;

use bytes::Bytes;
use openstack_eventbridge::EventBridgeProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "eventbridge".to_string(),
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
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

#[allow(dead_code)]
fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_and_list_event_buses() {
    let p = EventBridgeProvider::new();
    let resp = p
        .dispatch(&make_ctx("CreateEventBus", json!({ "Name": "my-bus" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(b["EventBusArn"].as_str().unwrap().contains("my-bus"));

    let resp = p
        .dispatch(&make_ctx("ListEventBuses", json!({})))
        .await
        .unwrap();
    let b = body(&resp);
    let buses = b["EventBuses"].as_array().unwrap();
    assert!(buses.iter().any(|bus| bus["Name"] == "my-bus"));
    // default bus is always included
    assert!(buses.iter().any(|bus| bus["Name"] == "default"));
}

#[tokio::test]
async fn test_delete_event_bus() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx("CreateEventBus", json!({ "Name": "del-bus" })))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("DeleteEventBus", json!({ "Name": "del-bus" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_delete_default_bus_fails() {
    let p = EventBridgeProvider::new();
    let resp = p
        .dispatch(&make_ctx("DeleteEventBus", json!({ "Name": "default" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
}

#[tokio::test]
async fn test_put_and_list_rules() {
    let p = EventBridgeProvider::new();
    let pattern = json!({ "source": ["myapp.backend"] });
    let resp = p
        .dispatch(&make_ctx(
            "PutRule",
            json!({
                "Name": "my-rule",
                "EventPattern": serde_json::to_string(&pattern).unwrap(),
                "State": "ENABLED",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(b["RuleArn"].as_str().unwrap().contains("my-rule"));

    let resp = p
        .dispatch(&make_ctx("ListRules", json!({ "EventBusName": "default" })))
        .await
        .unwrap();
    let b = body(&resp);
    let rules = b["Rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["Name"], "my-rule");
}

#[tokio::test]
async fn test_describe_rule() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx(
        "PutRule",
        json!({ "Name": "desc-rule", "ScheduleExpression": "rate(5 minutes)", "State": "ENABLED" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("DescribeRule", json!({ "Name": "desc-rule" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["Name"], "desc-rule");
    assert_eq!(b["ScheduleExpression"], "rate(5 minutes)");
}

#[tokio::test]
async fn test_put_and_list_targets() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx(
        "PutRule",
        json!({ "Name": "target-rule", "ScheduleExpression": "rate(1 hour)", "State": "ENABLED" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutTargets",
            json!({
                "Rule": "target-rule",
                "Targets": [
                    { "Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:my-queue" },
                    { "Id": "t2", "Arn": "arn:aws:lambda:us-east-1:000000000000:function:my-fn" },
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["FailedEntryCount"], 0);

    let resp = p
        .dispatch(&make_ctx(
            "ListTargetsByRule",
            json!({ "Rule": "target-rule" }),
        ))
        .await
        .unwrap();
    let b = body(&resp);
    let targets = b["Targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
}

#[tokio::test]
async fn test_remove_targets() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx(
        "PutRule",
        json!({ "Name": "rm-rule", "ScheduleExpression": "rate(1 hour)", "State": "ENABLED" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutTargets",
        json!({ "Rule": "rm-rule", "Targets": [{ "Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:q1" }] }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "RemoveTargets",
            json!({ "Rule": "rm-rule", "Ids": ["t1"] }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("ListTargetsByRule", json!({ "Rule": "rm-rule" })))
        .await
        .unwrap();
    let b = body(&resp);
    assert_eq!(b["Targets"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_put_events() {
    let p = EventBridgeProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "PutEvents",
            json!({
                "Entries": [
                    {
                        "Source": "myapp",
                        "DetailType": "UserSignedUp",
                        "Detail": serde_json::to_string(&json!({ "userId": "123" })).unwrap(),
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["FailedEntryCount"], 0);
    assert_eq!(b["Entries"].as_array().unwrap().len(), 1);
    assert!(b["Entries"][0]["EventId"].as_str().is_some());
}

#[tokio::test]
async fn test_enable_disable_rule() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx(
        "PutRule",
        json!({ "Name": "toggle-rule", "ScheduleExpression": "rate(1 hour)", "State": "ENABLED" }),
    ))
    .await
    .unwrap();

    p.dispatch(&make_ctx("DisableRule", json!({ "Name": "toggle-rule" })))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("DescribeRule", json!({ "Name": "toggle-rule" })))
        .await
        .unwrap();
    assert_eq!(body(&resp)["State"], "DISABLED");

    p.dispatch(&make_ctx("EnableRule", json!({ "Name": "toggle-rule" })))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("DescribeRule", json!({ "Name": "toggle-rule" })))
        .await
        .unwrap();
    assert_eq!(body(&resp)["State"], "ENABLED");
}

#[tokio::test]
async fn test_delete_rule() {
    let p = EventBridgeProvider::new();
    p.dispatch(&make_ctx(
        "PutRule",
        json!({ "Name": "del-rule", "ScheduleExpression": "rate(1 hour)", "State": "ENABLED" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx("DeleteRule", json!({ "Name": "del-rule" })))
        .await
        .unwrap();
    let resp = p
        .dispatch(&make_ctx("ListRules", json!({ "EventBusName": "default" })))
        .await
        .unwrap();
    let b = body(&resp);
    assert!(
        !b["Rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["Name"] == "del-rule")
    );
}
