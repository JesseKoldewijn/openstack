/// Performance tests for SNS provider.
///
/// These cover the common topic creation, publish, and list paths so we can
/// catch obvious regressions in basic topic and message fan-out bookkeeping.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_sns::SnsProvider;

fn make_ctx(body: &[u8]) -> RequestContext {
    RequestContext {
        service: "sns".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Some(Bytes::from(body.to_vec())),
        headers: Default::default(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn form_body(params: &[(&str, &str)]) -> Vec<u8> {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", k, url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

async fn create_topic(provider: &SnsProvider, name: &str) -> String {
    let body = form_body(&[("Action", "CreateTopic"), ("Name", name)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = String::from_utf8_lossy(resp.body.as_bytes()).to_string();
    xml.split("<TopicArn>")
        .nth(1)
        .unwrap()
        .split("</TopicArn>")
        .next()
        .unwrap()
        .to_string()
}

async fn delete_topic(provider: &SnsProvider, topic_arn: &str) {
    let body = form_body(&[("Action", "DeleteTopic"), ("TopicArn", topic_arn)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_topic_throughput() {
    let provider = SnsProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let body = form_body(&[
            ("Action", "CreateTopic"),
            ("Name", &format!("perf-topic-{i:03}")),
        ]);
        let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1500,
        "CreateTopic x{n} took {}ms — expected <1500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_publish_throughput() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "perf-publish-topic").await;
    let n = 200usize;

    let start = Instant::now();
    for i in 0..n {
        let body = form_body(&[
            ("Action", "Publish"),
            ("TopicArn", &topic_arn),
            ("Message", &format!("message-{i}")),
        ]);
        let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "Publish x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_topics_many() {
    let provider = SnsProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_topic(&provider, &format!("perf-list-topic-{i:03}")).await;
    }

    let body = form_body(&[("Action", "ListTopics")]);
    let start = Instant::now();
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let xml = String::from_utf8_lossy(resp.body.as_bytes()).to_string();
    assert!(xml.contains("perf-list-topic-000"));
    assert!(xml.contains("perf-list-topic-099"));

    assert!(
        elapsed.as_millis() < 500,
        "ListTopics({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_create_and_delete_topic_round_trip() {
    let provider = SnsProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let topic_arn = create_topic(&provider, &format!("perf-delete-topic-{i:03}")).await;
        delete_topic(&provider, &topic_arn).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateTopic/DeleteTopic x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}
