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

fn make_query_ctx(operation: &str, action: &str, form_body: &str) -> RequestContext {
    let mut ctx = make_ctx(operation, json!({}));
    let mut query_params = HashMap::new();
    query_params.insert("Action".to_string(), action.to_string());
    ctx.raw_body = Some(Bytes::from(form_body.as_bytes().to_vec()));
    ctx.query_params = query_params;
    ctx
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

fn body_str(resp: &DispatchResponse) -> String {
    String::from_utf8_lossy(resp.body.as_bytes()).to_string()
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
async fn test_list_metrics_query_protocol_includes_dimensions() {
    let p = CloudWatchProvider::new();
    p.dispatch(&make_ctx(
        "PutMetricData",
        json!({
            "Namespace": "TestNS",
            "MetricData": [
                {
                    "MetricName": "MyMetric",
                    "Value": 1.0,
                    "Unit": "Count",
                    "Dimensions": [
                        { "Name": "Service", "Value": "api" },
                        { "Name": "Env", "Value": "dev" }
                    ]
                }
            ]
        }),
    ))
    .await
    .unwrap();

    let mut ctx = make_query_ctx(
        "ListMetrics",
        "ListMetrics",
        "Action=ListMetrics&Version=2010-08-01&Namespace=TestNS",
    );
    ctx.request_body = json!({ "Namespace": "TestNS" });

    let resp = p.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert_eq!(&*resp.content_type, "text/xml");
    let xml = body_str(&resp);
    assert!(xml.contains("<Dimensions>"));
    assert!(xml.contains("<Name>Service</Name>"));
    assert!(xml.contains("<Value>api</Value>"));
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
async fn test_get_metric_statistics_empty_query_protocol_returns_xml() {
    let p = CloudWatchProvider::new();
    let mut ctx = make_query_ctx(
        "GetMetricStatistics",
        "GetMetricStatistics",
        "Action=GetMetricStatistics&Version=2010-08-01&Namespace=AWS%2FLogs&MetricName=IncomingLogEvents&StartTime=2024-01-01T00%3A00%3A00Z&EndTime=2024-01-01T01%3A00%3A00Z&Period=60&Statistics.member.1=Sum",
    );
    ctx.request_body = json!({
        "Namespace": "AWS/Logs",
        "MetricName": "IncomingLogEvents",
        "StartTime": "2024-01-01T00:00:00Z",
        "EndTime": "2024-01-01T01:00:00Z",
        "Period": 60,
        "Statistics": ["Sum"],
    });

    let resp = p.dispatch(&ctx).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(&*resp.content_type, "text/xml");
    let xml = body_str(&resp);
    assert!(xml.contains("<GetMetricStatisticsResponse"));
    assert!(xml.contains("<GetMetricStatisticsResult>"));
    assert!(xml.contains("<Datapoints />"));
    assert!(xml.contains("<Label>IncomingLogEvents</Label>"));
}

#[tokio::test]
async fn test_get_metric_statistics_empty_json_protocol_returns_json() {
    let p = CloudWatchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetMetricStatistics",
            json!({
                "Namespace": "AWS/Logs",
                "MetricName": "IncomingLogEvents",
                "Period": 60,
                "Statistics": ["Sum"],
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(&*resp.content_type, "application/x-amz-json-1.1");
    let b = body(&resp);
    assert_eq!(b["Label"], "IncomingLogEvents");
    assert_eq!(b["Datapoints"], json!([]));
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

// ---------------------------------------------------------------------------
// GetMetricData — MetricStat queries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_metric_data_metric_stat_sum() {
    let p = CloudWatchProvider::new();

    for v in [10.0f64, 20.0, 30.0] {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "MyApp",
                "MetricData": [{ "MetricName": "Requests", "Value": v, "Unit": "Count" }]
            }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "2000-01-01T00:00:00Z",
                "EndTime":   "2099-01-01T00:00:00Z",
                "MetricDataQueries": [
                    {
                        "Id": "q1",
                        "Label": "Total Requests",
                        "MetricStat": {
                            "Metric": { "Namespace": "MyApp", "MetricName": "Requests" },
                            "Period": 60,
                            "Stat": "Sum",
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let results = b["MetricDataResults"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["Id"], "q1");
    assert_eq!(results[0]["Label"], "Total Requests");
    assert_eq!(results[0]["StatusCode"], "Complete");
    let values = results[0]["Values"].as_array().unwrap();
    assert_eq!(values.len(), 1, "expected 1 aggregated value");
    let sum: f64 = values[0].as_f64().unwrap();
    assert!((sum - 60.0).abs() < 0.001, "Sum should be 60.0, got {sum}");
}

#[tokio::test]
async fn test_get_metric_data_metric_stat_average() {
    let p = CloudWatchProvider::new();

    for v in [100.0f64, 200.0, 300.0] {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "SvcNS",
                "MetricData": [{ "MetricName": "Latency", "Value": v, "Unit": "Milliseconds" }]
            }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "2000-01-01T00:00:00Z",
                "EndTime":   "2099-01-01T00:00:00Z",
                "MetricDataQueries": [
                    {
                        "Id": "avg_q",
                        "MetricStat": {
                            "Metric": { "Namespace": "SvcNS", "MetricName": "Latency" },
                            "Period": 60,
                            "Stat": "Average",
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let results = b["MetricDataResults"].as_array().unwrap();
    let v: f64 = results[0]["Values"][0].as_f64().unwrap();
    assert!(
        (v - 200.0).abs() < 0.001,
        "Average should be 200.0, got {v}"
    );
}

#[tokio::test]
async fn test_get_metric_data_no_data_returns_empty() {
    let p = CloudWatchProvider::new();

    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "2000-01-01T00:00:00Z",
                "EndTime":   "2099-01-01T00:00:00Z",
                "MetricDataQueries": [
                    {
                        "Id": "empty",
                        "MetricStat": {
                            "Metric": { "Namespace": "NonExistent", "MetricName": "Ghost" },
                            "Period": 60,
                            "Stat": "Sum",
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let results = b["MetricDataResults"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["Values"].as_array().unwrap().len(), 0);
    assert_eq!(results[0]["Timestamps"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_metric_data_expression_sum_aggregates_metric_results() {
    let p = CloudWatchProvider::new();

    for v in [5.0f64, 15.0] {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "ExprNS",
                "MetricData": [{ "MetricName": "Clicks", "Value": v, "Unit": "Count" }]
            }),
        ))
        .await
        .unwrap();
    }

    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "2000-01-01T00:00:00Z",
                "EndTime":   "2099-01-01T00:00:00Z",
                "MetricDataQueries": [
                    {
                        "Id": "m1",
                        "MetricStat": {
                            "Metric": { "Namespace": "ExprNS", "MetricName": "Clicks" },
                            "Period": 60,
                            "Stat": "Sum",
                        }
                    },
                    {
                        "Id": "total",
                        "Expression": "SUM(METRICS())",
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200, "{}", body_str(&resp));
    let b = body(&resp);
    let results = b["MetricDataResults"].as_array().unwrap();
    assert_eq!(results.len(), 2, "expected 2 result entries");
    // Find the expression result
    let expr_result = results
        .iter()
        .find(|r| r["Id"] == "total")
        .expect("expression result 'total' not found");
    let vals = expr_result["Values"].as_array().unwrap();
    assert!(!vals.is_empty(), "expression SUM should produce a value");
    let sum: f64 = vals[0].as_f64().unwrap();
    // m1 = 5+15 = 20, expression sums over m1 → 20
    assert!(
        (sum - 20.0).abs() < 0.001,
        "SUM expression value should be 20.0, got {sum}"
    );
}

#[tokio::test]
async fn test_get_metric_data_invalid_time_format_returns_graceful_response() {
    // AWS behavior: malformed time strings should not panic the provider.
    // The provider should return 200 with an empty (or best-effort) MetricDataResults array.
    let p = CloudWatchProvider::new();
    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "not-a-date",
                "EndTime": "also-not-a-date",
                "MetricDataQueries": [
                    {
                        "Id": "m1",
                        "MetricStat": {
                            "Metric": {
                                "Namespace": "TestNS",
                                "MetricName": "TestMetric"
                            },
                            "Period": 60,
                            "Stat": "Sum"
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(
        resp.status_code, 200,
        "invalid time format should return 200, not panic; body={}",
        body_str(&resp)
    );
    let b = body(&resp);
    assert!(
        b.get("MetricDataResults").is_some(),
        "response should contain MetricDataResults key; body={}",
        body_str(&resp)
    );
}
