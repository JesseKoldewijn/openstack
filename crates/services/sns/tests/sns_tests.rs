use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_sns::SnsProvider;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_ctx(body: &[u8]) -> RequestContext {
    RequestContext {
        service: "sns".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Bytes::from(body.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
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
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn body_str(resp: &openstack_service_framework::traits::DispatchResponse) -> String {
    std::str::from_utf8(&resp.body).unwrap().to_string()
}

async fn create_topic(provider: &SnsProvider, name: &str) -> String {
    let body = form_body(&[("Action", "CreateTopic"), ("Name", name)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    xml.split("<TopicArn>")
        .nth(1)
        .unwrap()
        .split("</TopicArn>")
        .next()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------
// Topic operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_topic() {
    let provider = SnsProvider::new();
    let body = form_body(&[("Action", "CreateTopic"), ("Name", "my-topic")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("CreateTopicResponse"));
    assert!(xml.contains("TopicArn"));
    assert!(xml.contains("my-topic"));
}

#[tokio::test]
async fn test_create_topic_idempotent() {
    let provider = SnsProvider::new();
    let body = form_body(&[("Action", "CreateTopic"), ("Name", "idempotent-topic")]);
    let resp1 = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let resp2 = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp1.status_code, 200);
    assert_eq!(resp2.status_code, 200);
    let arn1 = body_str(&resp1)
        .split("<TopicArn>")
        .nth(1)
        .unwrap()
        .split("</TopicArn>")
        .next()
        .unwrap()
        .to_string();
    let arn2 = body_str(&resp2)
        .split("<TopicArn>")
        .nth(1)
        .unwrap()
        .split("</TopicArn>")
        .next()
        .unwrap()
        .to_string();
    assert_eq!(arn1, arn2);
}

#[tokio::test]
async fn test_delete_topic() {
    let provider = SnsProvider::new();
    let arn = create_topic(&provider, "delete-me").await;

    let body = form_body(&[("Action", "DeleteTopic"), ("TopicArn", &arn)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // Verify gone
    let body2 = form_body(&[("Action", "GetTopicAttributes"), ("TopicArn", &arn)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 404);
}

#[tokio::test]
async fn test_list_topics() {
    let provider = SnsProvider::new();
    create_topic(&provider, "topic-alpha").await;
    create_topic(&provider, "topic-beta").await;

    let body = form_body(&[("Action", "ListTopics")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("topic-alpha"));
    assert!(xml.contains("topic-beta"));
}

#[tokio::test]
async fn test_get_topic_attributes() {
    let provider = SnsProvider::new();
    let arn = create_topic(&provider, "attr-topic").await;

    let body = form_body(&[("Action", "GetTopicAttributes"), ("TopicArn", &arn)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("TopicArn"));
    assert!(xml.contains("Owner"));
    assert!(xml.contains("SubscriptionsConfirmed"));
}

#[tokio::test]
async fn test_set_topic_attributes() {
    let provider = SnsProvider::new();
    let arn = create_topic(&provider, "settable-topic").await;

    let body = form_body(&[
        ("Action", "SetTopicAttributes"),
        ("TopicArn", &arn),
        ("AttributeName", "DisplayName"),
        ("AttributeValue", "My Display Name"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // Verify
    let body2 = form_body(&[("Action", "GetTopicAttributes"), ("TopicArn", &arn)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert!(body_str(&resp2).contains("My Display Name"));
}

// ---------------------------------------------------------------------------
// Subscription operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_subscribe() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "sub-topic").await;

    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &topic_arn),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:my-queue"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("SubscriptionArn"));
    assert!(xml.contains("sub-topic"));
}

#[tokio::test]
async fn test_subscribe_nonexistent_topic() {
    let provider = SnsProvider::new();
    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", "arn:aws:sns:us-east-1:000000000000:ghost"),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:q"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_unsubscribe() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "unsub-topic").await;

    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &topic_arn),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:q"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let sub_arn = body_str(&resp)
        .split("<SubscriptionArn>")
        .nth(1)
        .unwrap()
        .split("</SubscriptionArn>")
        .next()
        .unwrap()
        .to_string();

    let body2 = form_body(&[("Action", "Unsubscribe"), ("SubscriptionArn", &sub_arn)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 200);

    // Verify gone
    let body3 = form_body(&[
        ("Action", "GetSubscriptionAttributes"),
        ("SubscriptionArn", &sub_arn),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert_eq!(resp3.status_code, 404);
}

#[tokio::test]
async fn test_list_subscriptions() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "listsub-topic").await;

    for i in 0..3 {
        let body = form_body(&[
            ("Action", "Subscribe"),
            ("TopicArn", &topic_arn),
            ("Protocol", "sqs"),
            (
                "Endpoint",
                &format!("arn:aws:sqs:us-east-1:000000000000:q{i}"),
            ),
        ]);
        provider.dispatch(&make_ctx(&body)).await.unwrap();
    }

    let body = form_body(&[("Action", "ListSubscriptions")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("q0"));
    assert!(xml.contains("q1"));
    assert!(xml.contains("q2"));
}

#[tokio::test]
async fn test_list_subscriptions_by_topic() {
    let provider = SnsProvider::new();
    let t1 = create_topic(&provider, "topic-x").await;
    let t2 = create_topic(&provider, "topic-y").await;

    // Subscribe 2 to t1, 1 to t2
    for q in &["q-a", "q-b"] {
        let body = form_body(&[
            ("Action", "Subscribe"),
            ("TopicArn", &t1),
            ("Protocol", "sqs"),
            (
                "Endpoint",
                &format!("arn:aws:sqs:us-east-1:000000000000:{q}"),
            ),
        ]);
        provider.dispatch(&make_ctx(&body)).await.unwrap();
    }
    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &t2),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:q-c"),
    ]);
    provider.dispatch(&make_ctx(&body)).await.unwrap();

    let body = form_body(&[("Action", "ListSubscriptionsByTopic"), ("TopicArn", &t1)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let xml = body_str(&resp);
    assert!(xml.contains("q-a"));
    assert!(xml.contains("q-b"));
    assert!(!xml.contains("q-c"));
}

#[tokio::test]
async fn test_get_subscription_attributes() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "subattr-topic").await;

    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &topic_arn),
        ("Protocol", "https"),
        ("Endpoint", "https://example.com/notify"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let sub_arn = body_str(&resp)
        .split("<SubscriptionArn>")
        .nth(1)
        .unwrap()
        .split("</SubscriptionArn>")
        .next()
        .unwrap()
        .to_string();

    let body2 = form_body(&[
        ("Action", "GetSubscriptionAttributes"),
        ("SubscriptionArn", &sub_arn),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 200);
    let xml = body_str(&resp2);
    assert!(xml.contains("https://example.com/notify"));
    assert!(xml.contains("https"));
}

#[tokio::test]
async fn test_set_subscription_attributes() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "setsub-topic").await;

    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &topic_arn),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:my-q"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let sub_arn = body_str(&resp)
        .split("<SubscriptionArn>")
        .nth(1)
        .unwrap()
        .split("</SubscriptionArn>")
        .next()
        .unwrap()
        .to_string();

    let body2 = form_body(&[
        ("Action", "SetSubscriptionAttributes"),
        ("SubscriptionArn", &sub_arn),
        ("AttributeName", "RawMessageDelivery"),
        ("AttributeValue", "true"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 200);

    // Verify
    let body3 = form_body(&[
        ("Action", "GetSubscriptionAttributes"),
        ("SubscriptionArn", &sub_arn),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert!(body_str(&resp3).contains("true"));
}

// ---------------------------------------------------------------------------
// Publish
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_publish() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "pub-topic").await;

    let body = form_body(&[
        ("Action", "Publish"),
        ("TopicArn", &topic_arn),
        ("Message", "Hello from SNS!"),
        ("Subject", "Test Subject"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("MessageId"));
}

#[tokio::test]
async fn test_publish_nonexistent_topic() {
    let provider = SnsProvider::new();
    let body = form_body(&[
        ("Action", "Publish"),
        ("TopicArn", "arn:aws:sns:us-east-1:000000000000:ghost"),
        ("Message", "hello"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 404);
}

#[tokio::test]
async fn test_publish_batch() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "batch-pub-topic").await;

    let body = form_body(&[
        ("Action", "PublishBatch"),
        ("TopicArn", &topic_arn),
        ("PublishBatchRequestEntries.member.1.Id", "msg1"),
        ("PublishBatchRequestEntries.member.1.Message", "body-one"),
        ("PublishBatchRequestEntries.member.2.Id", "msg2"),
        ("PublishBatchRequestEntries.member.2.Message", "body-two"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("msg1"));
    assert!(xml.contains("msg2"));
    assert!(xml.contains("MessageId"));
}

// ---------------------------------------------------------------------------
// Subscription delivery: topic deleted removes subscriptions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_topic_removes_subscriptions() {
    let provider = SnsProvider::new();
    let topic_arn = create_topic(&provider, "del-with-subs").await;

    let body = form_body(&[
        ("Action", "Subscribe"),
        ("TopicArn", &topic_arn),
        ("Protocol", "sqs"),
        ("Endpoint", "arn:aws:sqs:us-east-1:000000000000:q"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let sub_arn = body_str(&resp)
        .split("<SubscriptionArn>")
        .nth(1)
        .unwrap()
        .split("</SubscriptionArn>")
        .next()
        .unwrap()
        .to_string();

    // Delete topic
    let body2 = form_body(&[("Action", "DeleteTopic"), ("TopicArn", &topic_arn)]);
    provider.dispatch(&make_ctx(&body2)).await.unwrap();

    // Subscription should be gone
    let body3 = form_body(&[
        ("Action", "GetSubscriptionAttributes"),
        ("SubscriptionArn", &sub_arn),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert_eq!(resp3.status_code, 404);

    // ListSubscriptions should be empty
    let body4 = form_body(&[("Action", "ListSubscriptions")]);
    let resp4 = provider.dispatch(&make_ctx(&body4)).await.unwrap();
    assert!(!body_str(&resp4).contains(&sub_arn));
}

// ---------------------------------------------------------------------------
// FIFO topic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fifo_topic() {
    let provider = SnsProvider::new();
    let arn = create_topic(&provider, "fifo.fifo").await;
    assert!(arn.contains("fifo.fifo"));

    let body = form_body(&[("Action", "GetTopicAttributes"), ("TopicArn", &arn)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert!(body_str(&resp).contains("true")); // FifoTopic = true
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_unknown_action() {
    let provider = SnsProvider::new();
    let body = form_body(&[("Action", "BogusAction")]);
    let result = provider.dispatch(&make_ctx(&body)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_missing_action() {
    let provider = SnsProvider::new();
    let body = form_body(&[("Name", "no-action")]);
    let result = provider.dispatch(&make_ctx(&body)).await;
    assert!(result.is_err());
}
