/// Performance tests for the CloudWatch provider.
///
/// Run with: `cargo test -p openstack-cloudwatch`
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_cloudwatch::CloudWatchProvider;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
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

// ---------------------------------------------------------------------------
// Perf 1 — PutMetricData × 500 datapoints then GetMetricData must finish
//           within 2 s total
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_put_then_get_metric_data() {
    let p = CloudWatchProvider::new();
    let n = 500usize;

    let start = Instant::now();
    for i in 0..n {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "PerfNS",
                "MetricData": [
                    { "MetricName": "Score", "Value": i as f64, "Unit": "Count" }
                ]
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
                        "Id": "all",
                        "MetricStat": {
                            "Metric": { "Namespace": "PerfNS", "MetricName": "Score" },
                            "Period": 60,
                            "Stat": "Sum",
                        }
                    }
                ]
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    let results = body["MetricDataResults"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    // Expected sum = 0+1+…+499 = 499*500/2 = 124_750
    let sum = results[0]["Values"][0].as_f64().unwrap_or(0.0);
    let expected = (n * (n - 1) / 2) as f64;
    assert!(
        (sum - expected).abs() < 1.0,
        "Sum should be {expected}, got {sum}"
    );

    assert!(
        elapsed.as_millis() < 2000,
        "PutMetricData×{n} + GetMetricData took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Perf 2 — GetMetricData with 10 queries in one call must finish under 500 ms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_get_metric_data_multi_query() {
    let p = CloudWatchProvider::new();

    // Pre-populate 10 distinct metrics
    for i in 0..10usize {
        p.dispatch(&make_ctx(
            "PutMetricData",
            json!({
                "Namespace": "MultiNS",
                "MetricData": [
                    { "MetricName": format!("Metric{i}"), "Value": i as f64 * 10.0, "Unit": "Count" }
                ]
            }),
        ))
        .await
        .unwrap();
    }

    let queries: Vec<Value> = (0..10usize)
        .map(|i| {
            json!({
                "Id": format!("q{i}"),
                "MetricStat": {
                    "Metric": { "Namespace": "MultiNS", "MetricName": format!("Metric{i}") },
                    "Period": 60,
                    "Stat": "Sum",
                }
            })
        })
        .collect();

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "GetMetricData",
            json!({
                "StartTime": "2000-01-01T00:00:00Z",
                "EndTime":   "2099-01-01T00:00:00Z",
                "MetricDataQueries": queries,
            }),
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(resp.status_code, 200);
    let body: Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
    let results = body["MetricDataResults"].as_array().unwrap();
    assert_eq!(results.len(), 10, "expected 10 query results");

    assert!(
        elapsed.as_millis() < 500,
        "GetMetricData(10 queries) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}
