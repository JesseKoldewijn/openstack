use std::collections::HashMap;

use bytes::Bytes;
use openstack_cloudwatch::CloudWatchProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value) -> RequestContext {
    RequestContext {
        service: "cloudwatch".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(&resp.body).expect("valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(&resp.body).to_string()
}

// ---------------------------------------------------------------------------
// CloudWatch Metrics Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_put_metric_data() {
    let p = CloudWatchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "MyApp/Latency",
                "MetricData": [
                    { "MetricName": "RequestLatency", "Value": 42.5, "Unit": "Milliseconds" }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_list_metrics() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "PutMetricData",
        json!({
            "Namespace": "TestNS",
            "MetricData": [
                { "MetricName": "MyMetric", "Value": 1.0, "Unit": "Count" }
            ]
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx("ListMetrics", json!({ "Namespace": "TestNS" })))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let metrics = b["Metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0]["MetricName"], "MyMetric");
}

#[tokio::test]
async fn test_get_metric_statistics() {
    let p = CloudWatchProvider::new();
    for v in [10.0, 20.0, 30.0] {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "Stats/NS",
                "MetricData": [{ "MetricName": "CPU", "Value": v, "Unit": "Percent" }]
            }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "GetMetricStatistics",
            json!({
                "Namespace": "Stats/NS",
                "MetricName": "CPU",
                "Period": 60,
                "Statistics": ["Average", "Sum", "Maximum"],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let dp = &b["Datapoints"].as_array().unwrap()[0];
    assert_eq!(dp["Average"], 20.0);
    assert_eq!(dp["Sum"], 60.0);
    assert_eq!(dp["Maximum"], 30.0);
}

#[tokio::test]
async fn test_put_and_describe_alarm() {
    let p = CloudWatchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "PutMetricAlarm",
            json!({
                "AlarmName": "high-cpu",
                "MetricName": "CPUUtilization",
                "Namespace": "AWS/EC2",
                "Statistic": "Average",
                "Period": 300,
                "EvaluationPeriods": 2,
                "Threshold": 90.0,
                "ComparisonOperator": "GreaterThanThreshold",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx(
            "DescribeAlarms",
            json!({ "AlarmNames": ["high-cpu"] }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let alarms = b["MetricAlarms"].as_array().unwrap();
    assert_eq!(alarms.len(), 1);
    assert_eq!(alarms[0]["AlarmName"], "high-cpu");
    assert_eq!(alarms[0]["Threshold"], 90.0);
    assert_eq!(alarms[0]["StateValue"], "INSUFFICIENT_DATA");
}

#[tokio::test]
async fn test_set_alarm_state() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "PutMetricAlarm",
        json!({ "AlarmName": "my-alarm", "MetricName": "X", "Namespace": "NS", "Statistic": "Average", "Period": 60, "EvaluationPeriods": 1, "Threshold": 1.0, "ComparisonOperator": "GreaterThanThreshold" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "SetAlarmState",
            json!({ "AlarmName": "my-alarm", "StateValue": "ALARM", "StateReason": "Testing" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeAlarms", json!({})))
        .await
        .unwrap();
    let b = body(&resp);
    let alarms = b["MetricAlarms"].as_array().unwrap();
    let alarm = alarms
        .iter()
        .find(|a| a["AlarmName"] == "my-alarm")
        .unwrap();
    assert_eq!(alarm["StateValue"], "ALARM");
}

#[tokio::test]
async fn test_delete_alarms() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "PutMetricAlarm",
        json!({ "AlarmName": "del-alarm", "MetricName": "X", "Namespace": "NS", "Statistic": "Average", "Period": 60, "EvaluationPeriods": 1, "Threshold": 1.0, "ComparisonOperator": "GreaterThanThreshold" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DeleteAlarms",
            json!({ "AlarmNames": ["del-alarm"] }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx(
            "DescribeAlarms",
            json!({ "AlarmNames": ["del-alarm"] }),
        ))
        .await
        .unwrap();
    let b = body(&resp);
    assert_eq!(b["MetricAlarms"].as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// CloudWatch Logs Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_log_group() {
    let p = CloudWatchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "CreateLogGroup",
            json!({ "logGroupName": "/my/app" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
}

#[tokio::test]
async fn test_create_duplicate_log_group_fails() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/dup/group" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "CreateLogGroup",
            json!({ "logGroupName": "/dup/group" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("ResourceAlreadyExistsException"));
}

#[tokio::test]
async fn test_describe_log_groups() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/apps/svc1" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/apps/svc2" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "DescribeLogGroups",
            json!({ "logGroupNamePrefix": "/apps" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert_eq!(b["logGroups"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_create_log_stream_and_put_events() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/my/group" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateLogStream",
        json!({ "logGroupName": "/my/group", "logStreamName": "stream-1" }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "PutLogEvents",
            json!({
                "logGroupName": "/my/group",
                "logStreamName": "stream-1",
                "logEvents": [
                    { "timestamp": 1700000000000_i64, "message": "Hello log" },
                    { "timestamp": 1700000001000_i64, "message": "Second event" },
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("nextSequenceToken"));
}

#[tokio::test]
async fn test_get_log_events() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/get/events" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateLogStream",
        json!({ "logGroupName": "/get/events", "logStreamName": "s1" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutLogEvents",
        json!({
            "logGroupName": "/get/events",
            "logStreamName": "s1",
            "logEvents": [{ "timestamp": 1700000000000_i64, "message": "test-log-line" }]
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "GetLogEvents",
            json!({ "logGroupName": "/get/events", "logStreamName": "s1" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let events = b["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["message"], "test-log-line");
}

#[tokio::test]
async fn test_filter_log_events() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/filter/g" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "CreateLogStream",
        json!({ "logGroupName": "/filter/g", "logStreamName": "s1" }),
    ))
    .await
    .unwrap();
    p.dispatch(&make_ctx(
        "PutLogEvents",
        json!({
            "logGroupName": "/filter/g",
            "logStreamName": "s1",
            "logEvents": [
                { "timestamp": 1_i64, "message": "ERROR: something bad" },
                { "timestamp": 2_i64, "message": "INFO: all good" },
            ]
        }),
    ))
    .await
    .unwrap();

    let resp = p
        .dispatch(&make_ctx(
            "FilterLogEvents",
            json!({ "logGroupName": "/filter/g", "filterPattern": "ERROR" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    let events = b["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0]["message"].as_str().unwrap().contains("ERROR"));
}

#[tokio::test]
async fn test_delete_log_group() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "CreateLogGroup",
        json!({ "logGroupName": "/del/g" }),
    ))
    .await
    .unwrap();
    let resp = p
        .dispatch(&make_ctx(
            "DeleteLogGroup",
            json!({ "logGroupName": "/del/g" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);

    let resp = p
        .dispatch(&make_ctx("DescribeLogGroups", json!({})))
        .await
        .unwrap();
    let b = body(&resp);
    let groups = b["logGroups"].as_array().unwrap();
    assert!(!groups.iter().any(|g| g["logGroupName"] == "/del/g"));
}
