/// Performance tests for SQS provider.
///
/// These cover queue creation, message send/receive, and queue attribute reads.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use openstack_sqs::SqsProvider;

fn make_ctx(body: &[u8]) -> RequestContext {
    RequestContext {
        service: "sqs".to_string(),
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
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn body_str(resp: &DispatchResponse) -> String {
    std::str::from_utf8(resp.body.as_bytes())
        .unwrap()
        .to_string()
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

async fn create_queue(provider: &SqsProvider, name: &str) -> String {
    let body = form_body(&[("Action", "CreateQueue"), ("QueueName", name)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    body_str(&resp)
        .split("<QueueUrl>")
        .nth(1)
        .unwrap()
        .split("</QueueUrl>")
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn perf_create_queue_throughput() {
    let provider = SqsProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        let _ = create_queue(&provider, &format!("perf-queue-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "CreateQueue x{n} took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_get_queue_attributes_many() {
    let provider = SqsProvider::new();
    let queue_url = create_queue(&provider, "perf-attr-queue").await;

    let start = Instant::now();
    for _ in 0..100usize {
        let body = form_body(&[
            ("Action", "GetQueueAttributes"),
            ("QueueUrl", &queue_url),
            ("AttributeName.1", "All"),
        ]);
        let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "GetQueueAttributes x100 took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_send_and_receive_round_trip() {
    let provider = SqsProvider::new();
    let queue_url = create_queue(&provider, "perf-msg-queue").await;

    let start = Instant::now();
    for i in 0..100usize {
        let send = form_body(&[
            ("Action", "SendMessage"),
            ("QueueUrl", &queue_url),
            ("MessageBody", &format!("msg-{i}")),
        ]);
        let send_resp = provider.dispatch(&make_ctx(&send)).await.unwrap();
        assert_eq!(send_resp.status_code, 200);

        let receive = form_body(&[
            ("Action", "ReceiveMessage"),
            ("QueueUrl", &queue_url),
            ("MaxNumberOfMessages", "1"),
        ]);
        let receive_resp = provider.dispatch(&make_ctx(&receive)).await.unwrap();
        assert_eq!(receive_resp.status_code, 200);
        let receive_body = body_str(&receive_resp);
        assert!(receive_body.contains(&format!("msg-{i}")));

        let receipt_handle =
            xml_tag(&receive_body, "ReceiptHandle").expect("missing ReceiptHandle");
        let delete = form_body(&[
            ("Action", "DeleteMessage"),
            ("QueueUrl", &queue_url),
            ("ReceiptHandle", &receipt_handle),
        ]);
        let delete_resp = provider.dispatch(&make_ctx(&delete)).await.unwrap();
        assert_eq!(delete_resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2500,
        "SendMessage/ReceiveMessage x100 took {}ms — expected <2500ms",
        elapsed.as_millis()
    );
}
