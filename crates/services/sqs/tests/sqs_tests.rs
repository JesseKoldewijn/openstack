use std::collections::HashMap;

use bytes::Bytes;
use openstack_service_framework::traits::{RequestContext, ServiceProvider};
use openstack_sqs::SqsProvider;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn make_ctx(body: &[u8]) -> RequestContext {
    RequestContext {
        service: "sqs".to_string(),
        operation: String::new(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: serde_json::Value::Null,
        raw_body: Bytes::from(body.to_vec()),
        headers: HashMap::new(),
        path: "/".to_string(),
        method: "POST".to_string(),
        query_params: HashMap::new(),
        spooled_body: None,
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
    std::str::from_utf8(resp.body.as_bytes())
        .unwrap()
        .to_string()
}

// Create a fresh provider and a queue, returning (provider, queue_url)
async fn setup_queue(name: &str) -> (SqsProvider, String) {
    let provider = SqsProvider::new();
    let body = form_body(&[("Action", "CreateQueue"), ("QueueName", name)]);
    let ctx = make_ctx(&body);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    // Extract QueueUrl from XML
    let url = xml
        .split("<QueueUrl>")
        .nth(1)
        .unwrap()
        .split("</QueueUrl>")
        .next()
        .unwrap()
        .to_string();
    (provider, url)
}

// ---------------------------------------------------------------------------
// Queue operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_queue() {
    let provider = SqsProvider::new();
    let body = form_body(&[("Action", "CreateQueue"), ("QueueName", "test-queue")]);
    let ctx = make_ctx(&body);
    let resp = provider.dispatch(&ctx).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("CreateQueueResponse"));
    assert!(xml.contains("QueueUrl"));
    assert!(xml.contains("test-queue"));
}

#[tokio::test]
async fn test_create_queue_idempotent() {
    let provider = SqsProvider::new();
    // Create twice — should return the same URL both times
    let body = form_body(&[("Action", "CreateQueue"), ("QueueName", "idempotent-q")]);
    let resp1 = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let resp2 = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp1.status_code, 200);
    assert_eq!(resp2.status_code, 200);
    // Both responses must contain the same queue URL (RequestId differs — that's fine)
    let extract_url = |xml: String| -> String {
        xml.split("<QueueUrl>")
            .nth(1)
            .unwrap()
            .split("</QueueUrl>")
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(extract_url(body_str(&resp1)), extract_url(body_str(&resp2)));
}

#[tokio::test]
async fn test_create_queue_missing_name() {
    let provider = SqsProvider::new();
    let body = form_body(&[("Action", "CreateQueue")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("MissingParameter"));
}

#[tokio::test]
async fn test_delete_queue() {
    let (provider, url) = setup_queue("del-queue").await;
    let body = form_body(&[("Action", "DeleteQueue"), ("QueueUrl", &url)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    assert!(body_str(&resp).contains("DeleteQueueResponse"));

    // Verify it's gone
    let body2 = form_body(&[("Action", "GetQueueUrl"), ("QueueName", "del-queue")]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 400);
    assert!(body_str(&resp2).contains("NonExistentQueue"));
}

#[tokio::test]
async fn test_delete_nonexistent_queue() {
    let provider = SqsProvider::new();
    let body = form_body(&[
        ("Action", "DeleteQueue"),
        ("QueueUrl", "http://localhost:4566/000000000000/ghost-queue"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 400);
    assert!(body_str(&resp).contains("NonExistentQueue"));
}

#[tokio::test]
async fn test_list_queues() {
    let provider = SqsProvider::new();
    // Create two queues
    for name in &["alpha-q", "beta-q"] {
        let body = form_body(&[("Action", "CreateQueue"), ("QueueName", name)]);
        provider.dispatch(&make_ctx(&body)).await.unwrap();
    }
    let body = form_body(&[("Action", "ListQueues")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("alpha-q"));
    assert!(xml.contains("beta-q"));
}

#[tokio::test]
async fn test_list_queues_with_prefix() {
    let provider = SqsProvider::new();
    for name in &["prod-queue-1", "prod-queue-2", "dev-queue"] {
        let body = form_body(&[("Action", "CreateQueue"), ("QueueName", name)]);
        provider.dispatch(&make_ctx(&body)).await.unwrap();
    }
    let body = form_body(&[("Action", "ListQueues"), ("QueueNamePrefix", "prod-")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let xml = body_str(&resp);
    assert!(xml.contains("prod-queue-1"));
    assert!(xml.contains("prod-queue-2"));
    assert!(!xml.contains("dev-queue"));
}

#[tokio::test]
async fn test_get_queue_url() {
    let (provider, original_url) = setup_queue("url-queue").await;
    let body = form_body(&[("Action", "GetQueueUrl"), ("QueueName", "url-queue")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains(&original_url));
}

#[tokio::test]
async fn test_get_queue_attributes() {
    let (provider, url) = setup_queue("attr-queue").await;
    let body = form_body(&[
        ("Action", "GetQueueAttributes"),
        ("QueueUrl", &url),
        ("AttributeName.1", "All"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("QueueArn"));
    assert!(xml.contains("VisibilityTimeout"));
    assert!(xml.contains("ApproximateNumberOfMessages"));
}

#[tokio::test]
async fn test_set_queue_attributes() {
    let (provider, url) = setup_queue("setattr-queue").await;
    let body = form_body(&[
        ("Action", "SetQueueAttributes"),
        ("QueueUrl", &url),
        ("Attribute.1.Name", "VisibilityTimeout"),
        ("Attribute.1.Value", "60"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // Verify the attribute changed
    let body2 = form_body(&[
        ("Action", "GetQueueAttributes"),
        ("QueueUrl", &url),
        ("AttributeName.1", "VisibilityTimeout"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml = body_str(&resp2);
    assert!(xml.contains("<Value>60</Value>"));
}

// ---------------------------------------------------------------------------
// Message operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_and_receive_message() {
    let (provider, url) = setup_queue("msg-queue").await;

    let body = form_body(&[
        ("Action", "SendMessage"),
        ("QueueUrl", &url),
        ("MessageBody", "Hello, SQS!"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("MessageId"));
    assert!(xml.contains("MD5OfMessageBody"));

    // Receive it
    let body2 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("MaxNumberOfMessages", "1"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert_eq!(resp2.status_code, 200);
    let xml2 = body_str(&resp2);
    assert!(xml2.contains("Hello, SQS!"));
    assert!(xml2.contains("ReceiptHandle"));
}

#[tokio::test]
async fn test_receive_empty_queue() {
    let (provider, url) = setup_queue("empty-queue").await;
    let body = form_body(&[("Action", "ReceiveMessage"), ("QueueUrl", &url)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    // No Message elements
    assert!(!xml.contains("<Message>"));
}

#[tokio::test]
async fn test_delete_message() {
    let (provider, url) = setup_queue("delete-msg-queue").await;

    // Send
    let body = form_body(&[
        ("Action", "SendMessage"),
        ("QueueUrl", &url),
        ("MessageBody", "to-delete"),
    ]);
    provider.dispatch(&make_ctx(&body)).await.unwrap();

    // Receive to get receipt handle
    let body2 = form_body(&[("Action", "ReceiveMessage"), ("QueueUrl", &url)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml2 = body_str(&resp2);
    let rh = xml2
        .split("<ReceiptHandle>")
        .nth(1)
        .unwrap()
        .split("</ReceiptHandle>")
        .next()
        .unwrap()
        .to_string();

    // Delete
    let body3 = form_body(&[
        ("Action", "DeleteMessage"),
        ("QueueUrl", &url),
        ("ReceiptHandle", &rh),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert_eq!(resp3.status_code, 200);

    // Change visibility timeout to 0 to make remaining messages visible
    let body4 = form_body(&[
        ("Action", "ChangeMessageVisibility"),
        ("QueueUrl", &url),
        ("ReceiptHandle", &rh),
        ("VisibilityTimeout", "0"),
    ]);
    provider.dispatch(&make_ctx(&body4)).await.ok();

    // Queue should now be empty (message was deleted)
    let body5 = form_body(&[
        ("Action", "GetQueueAttributes"),
        ("QueueUrl", &url),
        ("AttributeName.1", "ApproximateNumberOfMessages"),
    ]);
    let resp5 = provider.dispatch(&make_ctx(&body5)).await.unwrap();
    let xml5 = body_str(&resp5);
    assert!(xml5.contains("<Value>0</Value>"));
}

#[tokio::test]
async fn test_purge_queue() {
    let (provider, url) = setup_queue("purge-queue").await;

    for i in 0..5 {
        let body = form_body(&[
            ("Action", "SendMessage"),
            ("QueueUrl", &url),
            ("MessageBody", &format!("msg-{i}")),
        ]);
        provider.dispatch(&make_ctx(&body)).await.unwrap();
    }

    let body = form_body(&[("Action", "PurgeQueue"), ("QueueUrl", &url)]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    // Queue should be empty
    let body2 = form_body(&[("Action", "ReceiveMessage"), ("QueueUrl", &url)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert!(!body_str(&resp2).contains("<Message>"));
}

// ---------------------------------------------------------------------------
// Batch operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_send_message_batch() {
    let (provider, url) = setup_queue("batch-send-queue").await;

    let body = form_body(&[
        ("Action", "SendMessageBatch"),
        ("QueueUrl", &url),
        ("SendMessageBatchRequestEntry.1.Id", "msg1"),
        ("SendMessageBatchRequestEntry.1.MessageBody", "body-one"),
        ("SendMessageBatchRequestEntry.2.Id", "msg2"),
        ("SendMessageBatchRequestEntry.2.MessageBody", "body-two"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let xml = body_str(&resp);
    assert!(xml.contains("SendMessageBatchResultEntry"));
    assert!(xml.contains("msg1"));
    assert!(xml.contains("msg2"));

    // Verify two messages exist
    let body2 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("MaxNumberOfMessages", "10"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml2 = body_str(&resp2);
    assert!(xml2.contains("body-one"));
    assert!(xml2.contains("body-two"));
}

#[tokio::test]
async fn test_delete_message_batch() {
    let (provider, url) = setup_queue("batch-del-queue").await;

    // Send 2 messages
    let body = form_body(&[
        ("Action", "SendMessageBatch"),
        ("QueueUrl", &url),
        ("SendMessageBatchRequestEntry.1.Id", "m1"),
        ("SendMessageBatchRequestEntry.1.MessageBody", "b1"),
        ("SendMessageBatchRequestEntry.2.Id", "m2"),
        ("SendMessageBatchRequestEntry.2.MessageBody", "b2"),
    ]);
    provider.dispatch(&make_ctx(&body)).await.unwrap();

    // Receive both
    let body2 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("MaxNumberOfMessages", "10"),
        ("VisibilityTimeout", "0"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml2 = body_str(&resp2);

    // Extract both receipt handles
    let rhs: Vec<&str> = xml2
        .split("<ReceiptHandle>")
        .skip(1)
        .map(|s| s.split("</ReceiptHandle>").next().unwrap())
        .collect();
    assert_eq!(rhs.len(), 2);

    let body3 = form_body(&[
        ("Action", "DeleteMessageBatch"),
        ("QueueUrl", &url),
        ("DeleteMessageBatchRequestEntry.1.Id", "d1"),
        ("DeleteMessageBatchRequestEntry.1.ReceiptHandle", rhs[0]),
        ("DeleteMessageBatchRequestEntry.2.Id", "d2"),
        ("DeleteMessageBatchRequestEntry.2.ReceiptHandle", rhs[1]),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert_eq!(resp3.status_code, 200);
    assert!(body_str(&resp3).contains("DeleteMessageBatchResultEntry"));
}

// ---------------------------------------------------------------------------
// Visibility timeout
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_visibility_timeout() {
    let (provider, url) = setup_queue("vis-queue").await;

    // Send a message
    let body = form_body(&[
        ("Action", "SendMessage"),
        ("QueueUrl", &url),
        ("MessageBody", "invisible"),
    ]);
    provider.dispatch(&make_ctx(&body)).await.unwrap();

    // Receive with long visibility timeout — message becomes invisible
    let body2 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("VisibilityTimeout", "300"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert!(body_str(&resp2).contains("invisible"));

    // Receive again immediately — message should not appear (still invisible)
    let resp3 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert!(!body_str(&resp3).contains("invisible"));

    // Extract receipt handle and change visibility to 0
    let xml2 = body_str(&resp2);
    let rh = xml2
        .split("<ReceiptHandle>")
        .nth(1)
        .unwrap()
        .split("</ReceiptHandle>")
        .next()
        .unwrap();

    let body4 = form_body(&[
        ("Action", "ChangeMessageVisibility"),
        ("QueueUrl", &url),
        ("ReceiptHandle", rh),
        ("VisibilityTimeout", "0"),
    ]);
    provider.dispatch(&make_ctx(&body4)).await.unwrap();

    // Now message should be visible again
    let resp5 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    assert!(body_str(&resp5).contains("invisible"));
}

#[tokio::test]
async fn test_change_message_visibility_batch() {
    let (provider, url) = setup_queue("cvis-batch-queue").await;

    // Send 2 messages
    let body = form_body(&[
        ("Action", "SendMessageBatch"),
        ("QueueUrl", &url),
        ("SendMessageBatchRequestEntry.1.Id", "m1"),
        ("SendMessageBatchRequestEntry.1.MessageBody", "msg1"),
        ("SendMessageBatchRequestEntry.2.Id", "m2"),
        ("SendMessageBatchRequestEntry.2.MessageBody", "msg2"),
    ]);
    provider.dispatch(&make_ctx(&body)).await.unwrap();

    // Receive both
    let body2 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("MaxNumberOfMessages", "10"),
        ("VisibilityTimeout", "300"),
    ]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml2 = body_str(&resp2);
    let rhs: Vec<&str> = xml2
        .split("<ReceiptHandle>")
        .skip(1)
        .map(|s| s.split("</ReceiptHandle>").next().unwrap())
        .collect();
    assert_eq!(rhs.len(), 2);

    // Change both to visible immediately
    let body3 = form_body(&[
        ("Action", "ChangeMessageVisibilityBatch"),
        ("QueueUrl", &url),
        ("ChangeMessageVisibilityBatchRequestEntry.1.Id", "e1"),
        (
            "ChangeMessageVisibilityBatchRequestEntry.1.ReceiptHandle",
            rhs[0],
        ),
        (
            "ChangeMessageVisibilityBatchRequestEntry.1.VisibilityTimeout",
            "0",
        ),
        ("ChangeMessageVisibilityBatchRequestEntry.2.Id", "e2"),
        (
            "ChangeMessageVisibilityBatchRequestEntry.2.ReceiptHandle",
            rhs[1],
        ),
        (
            "ChangeMessageVisibilityBatchRequestEntry.2.VisibilityTimeout",
            "0",
        ),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert_eq!(resp3.status_code, 200);
    assert!(body_str(&resp3).contains("ChangeMessageVisibilityBatchResultEntry"));
}

// ---------------------------------------------------------------------------
// FIFO queue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fifo_queue_deduplication() {
    let provider = SqsProvider::new();
    let body = form_body(&[("Action", "CreateQueue"), ("QueueName", "my-queue.fifo")]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);
    let url = body_str(&resp)
        .split("<QueueUrl>")
        .nth(1)
        .unwrap()
        .split("</QueueUrl>")
        .next()
        .unwrap()
        .to_string();

    // Send same message twice with same dedup ID
    for _ in 0..2 {
        let body2 = form_body(&[
            ("Action", "SendMessage"),
            ("QueueUrl", &url),
            ("MessageBody", "fifo-body"),
            ("MessageGroupId", "group-1"),
            ("MessageDeduplicationId", "dedup-abc"),
        ]);
        let resp = provider.dispatch(&make_ctx(&body2)).await.unwrap();
        assert_eq!(resp.status_code, 200);
    }

    // Only one message should be in the queue
    let body3 = form_body(&[
        ("Action", "ReceiveMessage"),
        ("QueueUrl", &url),
        ("MaxNumberOfMessages", "10"),
    ]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    let xml3 = body_str(&resp3);
    // Count Message elements
    let count = xml3.matches("<Message>").count();
    assert_eq!(
        count, 1,
        "FIFO dedup should result in only 1 message, got {count}"
    );
}

// ---------------------------------------------------------------------------
// Message attributes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_message_attributes() {
    let (provider, url) = setup_queue("attr-msg-queue").await;

    let body = form_body(&[
        ("Action", "SendMessage"),
        ("QueueUrl", &url),
        ("MessageBody", "with-attrs"),
        ("MessageAttribute.1.Name", "Color"),
        ("MessageAttribute.1.Value.DataType", "String"),
        ("MessageAttribute.1.Value.StringValue", "Blue"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    assert_eq!(resp.status_code, 200);

    let body2 = form_body(&[("Action", "ReceiveMessage"), ("QueueUrl", &url)]);
    let resp2 = provider.dispatch(&make_ctx(&body2)).await.unwrap();
    let xml = body_str(&resp2);
    assert!(xml.contains("Color"));
    assert!(xml.contains("Blue"));
    assert!(xml.contains("String"));
}

// ---------------------------------------------------------------------------
// Delay queue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delay_queue_message_not_immediately_visible() {
    let provider = SqsProvider::new();
    let body = form_body(&[
        ("Action", "CreateQueue"),
        ("QueueName", "delay-queue"),
        ("Attribute.1.Name", "DelaySeconds"),
        ("Attribute.1.Value", "300"),
    ]);
    let resp = provider.dispatch(&make_ctx(&body)).await.unwrap();
    let url = body_str(&resp)
        .split("<QueueUrl>")
        .nth(1)
        .unwrap()
        .split("</QueueUrl>")
        .next()
        .unwrap()
        .to_string();

    // Send a message
    let body2 = form_body(&[
        ("Action", "SendMessage"),
        ("QueueUrl", &url),
        ("MessageBody", "delayed"),
    ]);
    provider.dispatch(&make_ctx(&body2)).await.unwrap();

    // Immediately try to receive — should be empty (message is delayed)
    let body3 = form_body(&[("Action", "ReceiveMessage"), ("QueueUrl", &url)]);
    let resp3 = provider.dispatch(&make_ctx(&body3)).await.unwrap();
    assert!(!body_str(&resp3).contains("delayed"));
}

// ---------------------------------------------------------------------------
// Action not found
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_unknown_action() {
    let provider = SqsProvider::new();
    let body = form_body(&[("Action", "BogusAction")]);
    let result = provider.dispatch(&make_ctx(&body)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_missing_action() {
    let provider = SqsProvider::new();
    let body = form_body(&[("QueueName", "no-action")]);
    let result = provider.dispatch(&make_ctx(&body)).await;
    assert!(result.is_err());
}
