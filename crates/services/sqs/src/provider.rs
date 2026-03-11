use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use tracing::{debug, warn};

use crate::store::{MessageAttributeValue, SqsStore, apply_queue_attributes};

pub struct SqsProvider {
    store: Arc<AccountRegionBundle<SqsStore>>,
}

impl SqsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SqsProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XML response helpers
// ---------------------------------------------------------------------------

fn xml_wrap(action: &str, request_id: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"http://queue.amazonaws.com/doc/2012-11-05/\">\
<{action}Result>{inner}</{action}Result>\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>",
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn xml_no_result(action: &str, request_id: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"http://queue.amazonaws.com/doc/2012-11-05/\">\
<ResponseMetadata><RequestId>{request_id}</RequestId></ResponseMetadata>\
</{action}Response>",
    );
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn sqs_error(code: &str, message: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"http://queue.amazonaws.com/doc/2012-11-05/\">\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
    );
    DispatchResponse {
        status_code: 400,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
        headers: Vec::new(),
    }
}

fn sqs_json_response(value: serde_json::Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: Bytes::from(value.to_string().into_bytes()),
        content_type: "application/x-amz-json-1.0".to_string(),
        headers: Vec::new(),
    }
}

fn sqs_json_error(code: &str, message: &str, status_code: u16) -> DispatchResponse {
    let body = serde_json::json!({
        "__type": code,
        "message": message,
    });
    DispatchResponse {
        status_code,
        body: Bytes::from(body.to_string().into_bytes()),
        content_type: "application/x-amz-json-1.0".to_string(),
        headers: Vec::new(),
    }
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Parameter parsing (SQS uses query protocol — form-urlencoded body)
// ---------------------------------------------------------------------------

fn parse_params(ctx: &RequestContext) -> HashMap<String, String> {
    let mut params: HashMap<String, String> = HashMap::new();

    // Query protocol form body
    let body_str = std::str::from_utf8(&ctx.raw_body).unwrap_or("");
    for (k, v) in body_str.split('&').filter_map(|kv| {
        let mut it = kv.splitn(2, '=');
        let k = it.next()?;
        let v = it.next().unwrap_or("");
        if k.is_empty() {
            None
        } else {
            Some((url_decode(k), url_decode(v)))
        }
    }) {
        params.insert(k, v);
    }

    // AWS CLI v2 JSON mode for SQS may send operation in x-amz-target and JSON body
    if !params.contains_key("Action") {
        if let Some(target) = ctx.headers.get("x-amz-target")
            && target.starts_with("AmazonSQS.")
            && let Some(op) = target.split('.').nth(1)
        {
            params.insert("Action".to_string(), op.to_string());
        }

        if let Ok(serde_json::Value::Object(map)) =
            serde_json::from_slice::<serde_json::Value>(&ctx.raw_body)
        {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    params.insert(k, s.to_string());
                } else if let Some(obj) = v.as_object() {
                    // Flatten common map objects (Attributes, Tags)
                    for (mk, mv) in obj {
                        if let Some(ms) = mv.as_str() {
                            params.insert(format!("{k}.{mk}"), ms.to_string());
                        }
                    }
                }
            }
        }
    }

    // Also absorb query string params (some clients put Action in query)
    for (k, v) in &ctx.query_params {
        params.entry(k.clone()).or_insert_with(|| v.clone());
    }

    params
}

fn normalize_queue_url_for_endpoint(url: &str, endpoint: &str) -> String {
    if let Some(path_start) = url.find("/000000000000/") {
        let path = &url[path_start..];
        return format!("{}{}", endpoint.trim_end_matches('/'), path);
    }
    if let Some(scheme_pos) = url.find("://") {
        let rest = &url[(scheme_pos + 3)..];
        if let Some(first_slash) = rest.find('/') {
            let path = &rest[first_slash..];
            return format!("{}{}", endpoint.trim_end_matches('/'), path);
        }
    }
    url.to_string()
}

fn apply_transport_compat(ctx: &RequestContext, params: &mut HashMap<String, String>) {
    // AWS CLI v2 SQS returns hostname URLs (sqs.us-east-1.localhost.localstack.cloud)
    // in GetQueueUrl, but follow-up calls still target endpoint-url host.
    if let Some(endpoint) = ctx.headers.get("host") {
        let scheme = if ctx.headers.get("x-forwarded-proto").map(|v| v.as_str()) == Some("https") {
            "https"
        } else {
            "http"
        };
        let endpoint_url = format!("{scheme}://{endpoint}");
        if let Some(queue_url) = params.get_mut("QueueUrl") {
            *queue_url = normalize_queue_url_for_endpoint(queue_url, &endpoint_url);
        }
    }
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3])
                && let Ok(byte) = u8::from_str_radix(hex_str, 16)
            {
                result.push(byte as char);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Extract indexed attributes from SQS query params.
/// e.g. `Attribute.1.Name=VisibilityTimeout&Attribute.1.Value=30`
fn extract_indexed_kv(params: &HashMap<String, String>, prefix: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut i = 1;
    loop {
        let name_key = format!("{prefix}.{i}.Name");
        let value_key = format!("{prefix}.{i}.Value");
        match (params.get(&name_key), params.get(&value_key)) {
            (Some(name), Some(value)) => {
                result.insert(name.clone(), value.clone());
                i += 1;
            }
            _ => break,
        }
    }
    result
}

/// Extract message attributes: MessageAttribute.1.Name, MessageAttribute.1.Value.DataType, etc.
fn extract_message_attributes(
    params: &HashMap<String, String>,
) -> HashMap<String, MessageAttributeValue> {
    let mut result = HashMap::new();
    let mut i = 1;
    loop {
        let name_key = format!("MessageAttribute.{i}.Name");
        let type_key = format!("MessageAttribute.{i}.Value.DataType");
        let str_key = format!("MessageAttribute.{i}.Value.StringValue");
        match params.get(&name_key) {
            None => break,
            Some(name) => {
                let data_type = params.get(&type_key).cloned().unwrap_or_default();
                let string_value = params.get(&str_key).cloned();
                result.insert(
                    name.clone(),
                    MessageAttributeValue {
                        data_type,
                        string_value,
                        binary_value: None,
                    },
                );
                i += 1;
            }
        }
    }
    result
}

fn queue_name_from_url(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("").to_string()
}

fn base_queue_url(_ctx: &RequestContext) -> String {
    if _ctx.headers.contains_key("host") {
        let region = _ctx
            .headers
            .get("x-amz-region")
            .cloned()
            .unwrap_or_else(|| _ctx.region.clone());
        return format!("http://sqs.{region}.localhost.localstack.cloud:4566");
    }
    "http://localhost:4566".to_string()
}

// ---------------------------------------------------------------------------
// Operation handlers
// ---------------------------------------------------------------------------

fn handle_create_queue(
    store: &mut SqsStore,
    ctx: &RequestContext,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let name = match params.get("QueueName") {
        Some(n) => n.clone(),
        None => return sqs_error("MissingParameter", "QueueName is required"),
    };

    let attributes = extract_indexed_kv(params, "Attribute");
    let base = base_queue_url(ctx);
    let q = store.create_queue(&name, &base, &ctx.account_id, &ctx.region, &attributes);
    let url = q.url.clone();
    let rid = new_request_id();

    xml_wrap(
        "CreateQueue",
        &rid,
        &format!("<QueueUrl>{}</QueueUrl>", escape_xml(&url)),
    )
}

fn handle_delete_queue(store: &mut SqsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);
    if !store.delete_queue(&name) {
        return sqs_error(
            "AWS.SimpleQueueService.NonExistentQueue",
            "Queue does not exist",
        );
    }
    xml_no_result("DeleteQueue", &new_request_id())
}

fn handle_list_queues(store: &SqsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let prefix = params.get("QueueNamePrefix").cloned().unwrap_or_default();
    let queues = store.list_queues(&prefix);

    let mut inner = String::new();
    for q in queues {
        inner.push_str(&format!("<QueueUrl>{}</QueueUrl>", escape_xml(&q.url)));
    }

    xml_wrap("ListQueues", &new_request_id(), &inner)
}

fn handle_get_queue_url(store: &SqsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let name = match params.get("QueueName") {
        Some(n) => n.clone(),
        None => return sqs_error("MissingParameter", "QueueName is required"),
    };
    match store.get_queue_by_name(&name) {
        None => sqs_error(
            "AWS.SimpleQueueService.NonExistentQueue",
            "Queue does not exist",
        ),
        Some(q) => xml_wrap(
            "GetQueueUrl",
            &new_request_id(),
            &format!("<QueueUrl>{}</QueueUrl>", escape_xml(&q.url)),
        ),
    }
}

fn handle_get_queue_attributes(
    store: &SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);
    let q = match store.get_queue_by_name(&name) {
        None => {
            return sqs_error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "Queue does not exist",
            );
        }
        Some(q) => q,
    };

    // Which attributes were requested?
    let requested: Vec<String> = {
        let mut i = 1;
        let mut r = Vec::new();
        loop {
            let k = format!("AttributeName.{i}");
            match params.get(&k) {
                Some(v) => {
                    r.push(v.clone());
                    i += 1;
                }
                None => break,
            }
        }
        r
    };
    let all = requested.is_empty() || requested.iter().any(|r| r == "All");

    let mut inner = String::new();
    let attrs: Vec<(&str, String)> = vec![
        ("QueueArn", q.arn.clone()),
        ("VisibilityTimeout", q.visibility_timeout.to_string()),
        (
            "MessageRetentionPeriod",
            q.message_retention_period.to_string(),
        ),
        ("MaximumMessageSize", q.maximum_message_size.to_string()),
        ("DelaySeconds", q.delay_seconds.to_string()),
        (
            "ReceiveMessageWaitTimeSeconds",
            q.receive_message_wait_time_seconds.to_string(),
        ),
        (
            "ApproximateNumberOfMessages",
            q.approximate_number_of_messages().to_string(),
        ),
        (
            "ApproximateNumberOfMessagesNotVisible",
            q.approximate_number_of_messages_not_visible().to_string(),
        ),
        ("CreatedTimestamp", q.created.timestamp().to_string()),
        (
            "LastModifiedTimestamp",
            q.last_modified.timestamp().to_string(),
        ),
        ("FifoQueue", q.fifo.to_string()),
        (
            "ContentBasedDeduplication",
            q.content_based_deduplication.to_string(),
        ),
    ];

    for (k, v) in &attrs {
        if all || requested.iter().any(|r| r == k) {
            inner.push_str(&format!(
                "<Attribute><Name>{k}</Name><Value>{}</Value></Attribute>",
                escape_xml(v)
            ));
        }
    }

    xml_wrap("GetQueueAttributes", &new_request_id(), &inner)
}

fn handle_set_queue_attributes(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);
    let q = match store.get_queue_by_name_mut(&name) {
        None => {
            return sqs_error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "Queue does not exist",
            );
        }
        Some(q) => q,
    };
    let attributes = extract_indexed_kv(params, "Attribute");
    apply_queue_attributes(q, &attributes);
    xml_no_result("SetQueueAttributes", &new_request_id())
}

fn handle_purge_queue(store: &mut SqsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);
    match store.get_queue_by_name_mut(&name) {
        None => sqs_error(
            "AWS.SimpleQueueService.NonExistentQueue",
            "Queue does not exist",
        ),
        Some(q) => {
            q.purge();
            xml_no_result("PurgeQueue", &new_request_id())
        }
    }
}

fn handle_send_message(store: &mut SqsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let body = match params.get("MessageBody") {
        Some(b) => b.clone(),
        None => return sqs_error("MissingParameter", "MessageBody is required"),
    };
    let name = queue_name_from_url(&url);
    let delay = params.get("DelaySeconds").and_then(|v| v.parse().ok());
    let message_group_id = params.get("MessageGroupId").cloned();
    let dedup_id = params.get("MessageDeduplicationId").cloned();
    let msg_attrs = extract_message_attributes(params);

    let q = match store.get_queue_by_name_mut(&name) {
        None => {
            return sqs_error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "Queue does not exist",
            );
        }
        Some(q) => q,
    };

    match q.send_message(body, delay, msg_attrs, message_group_id, dedup_id) {
        None => {
            // Deduplicated — return success with placeholder
            xml_wrap(
                "SendMessage",
                &new_request_id(),
                "<MessageId>duplicate</MessageId><MD5OfMessageBody></MD5OfMessageBody><SequenceNumber>0</SequenceNumber>",
            )
        }
        Some(msg) => xml_wrap(
            "SendMessage",
            &new_request_id(),
            &format!(
                "<MessageId>{}</MessageId>\
<MD5OfMessageBody>{}</MD5OfMessageBody>\
<SequenceNumber>{}</SequenceNumber>",
                escape_xml(&msg.message_id),
                escape_xml(&msg.md5_of_body),
                msg.sequence_number
            ),
        ),
    }
}

fn handle_send_message_batch(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);

    let mut successful = String::new();
    let mut i = 1;

    loop {
        let id_key = format!("SendMessageBatchRequestEntry.{i}.Id");
        let body_key = format!("SendMessageBatchRequestEntry.{i}.MessageBody");
        match (params.get(&id_key), params.get(&body_key)) {
            (Some(id), Some(body)) => {
                let delay = params
                    .get(&format!("SendMessageBatchRequestEntry.{i}.DelaySeconds"))
                    .and_then(|v| v.parse().ok());
                let group_id = params
                    .get(&format!("SendMessageBatchRequestEntry.{i}.MessageGroupId"))
                    .cloned();
                let dedup_id = params
                    .get(&format!(
                        "SendMessageBatchRequestEntry.{i}.MessageDeduplicationId"
                    ))
                    .cloned();

                let q = match store.get_queue_by_name_mut(&name) {
                    None => break,
                    Some(q) => q,
                };

                match q.send_message(body.clone(), delay, HashMap::new(), group_id, dedup_id) {
                    None => {
                        successful.push_str(&format!(
                            "<SendMessageBatchResultEntry>\
<Id>{}</Id><MessageId>dup</MessageId><MD5OfMessageBody></MD5OfMessageBody>\
</SendMessageBatchResultEntry>",
                            escape_xml(id)
                        ));
                    }
                    Some(msg) => {
                        successful.push_str(&format!(
                            "<SendMessageBatchResultEntry>\
<Id>{}</Id>\
<MessageId>{}</MessageId>\
<MD5OfMessageBody>{}</MD5OfMessageBody>\
</SendMessageBatchResultEntry>",
                            escape_xml(id),
                            escape_xml(&msg.message_id),
                            escape_xml(&msg.md5_of_body)
                        ));
                    }
                }
                i += 1;
            }
            _ => break,
        }
    }

    xml_wrap("SendMessageBatch", &new_request_id(), &successful)
}

fn handle_receive_message(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let max: usize = params
        .get("MaxNumberOfMessages")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .min(10);
    let vt: Option<u32> = params.get("VisibilityTimeout").and_then(|v| v.parse().ok());

    let name = queue_name_from_url(&url);
    let q = match store.get_queue_by_name_mut(&name) {
        None => {
            return sqs_error(
                "AWS.SimpleQueueService.NonExistentQueue",
                "Queue does not exist",
            );
        }
        Some(q) => q,
    };

    let messages = q.receive_messages(max, vt);

    // DLQ redrive on receive
    let dlq_msgs = q.messages_for_dlq();
    if !dlq_msgs.is_empty() {
        q.remove_dlq_candidates();
    }

    let mut inner = String::new();
    for msg in &messages {
        let mut attrs_xml = String::new();
        for (k, v) in &msg.attributes {
            attrs_xml.push_str(&format!(
                "<Attribute><Name>{}</Name><Value>{}</Value></Attribute>",
                escape_xml(k),
                escape_xml(v)
            ));
        }
        let mut msg_attrs_xml = String::new();
        for (k, v) in &msg.message_attributes {
            msg_attrs_xml.push_str(&format!(
                "<MessageAttribute><Name>{}</Name><Value>\
<DataType>{}</DataType>\
<StringValue>{}</StringValue>\
</Value></MessageAttribute>",
                escape_xml(k),
                escape_xml(&v.data_type),
                escape_xml(v.string_value.as_deref().unwrap_or(""))
            ));
        }

        inner.push_str(&format!(
            "<Message>\
<MessageId>{msg_id}</MessageId>\
<ReceiptHandle>{rh}</ReceiptHandle>\
<MD5OfBody>{md5}</MD5OfBody>\
<Body>{body}</Body>\
{attrs}\
{msg_attrs}\
</Message>",
            msg_id = escape_xml(&msg.message_id),
            rh = escape_xml(&msg.receipt_handle),
            md5 = escape_xml(&msg.md5_of_body),
            body = escape_xml(&msg.body),
            attrs = attrs_xml,
            msg_attrs = msg_attrs_xml,
        ));
    }

    xml_wrap("ReceiveMessage", &new_request_id(), &inner)
}

fn handle_delete_message(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let rh = match params.get("ReceiptHandle") {
        Some(r) => r.clone(),
        None => return sqs_error("MissingParameter", "ReceiptHandle is required"),
    };
    let name = queue_name_from_url(&url);
    if let Some(q) = store.get_queue_by_name_mut(&name) {
        q.delete_message(&rh);
    }
    xml_no_result("DeleteMessage", &new_request_id())
}

fn handle_delete_message_batch(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);

    let mut inner = String::new();
    let mut i = 1;
    loop {
        let id_key = format!("DeleteMessageBatchRequestEntry.{i}.Id");
        let rh_key = format!("DeleteMessageBatchRequestEntry.{i}.ReceiptHandle");
        match (params.get(&id_key), params.get(&rh_key)) {
            (Some(id), Some(rh)) => {
                if let Some(q) = store.get_queue_by_name_mut(&name) {
                    q.delete_message(rh);
                }
                inner.push_str(&format!(
                    "<DeleteMessageBatchResultEntry><Id>{}</Id></DeleteMessageBatchResultEntry>",
                    escape_xml(id)
                ));
                i += 1;
            }
            _ => break,
        }
    }
    xml_wrap("DeleteMessageBatch", &new_request_id(), &inner)
}

fn handle_change_message_visibility(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let rh = match params.get("ReceiptHandle") {
        Some(r) => r.clone(),
        None => return sqs_error("MissingParameter", "ReceiptHandle is required"),
    };
    let vt: u32 = params
        .get("VisibilityTimeout")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let name = queue_name_from_url(&url);
    if let Some(q) = store.get_queue_by_name_mut(&name) {
        q.change_visibility(&rh, vt);
    }
    xml_no_result("ChangeMessageVisibility", &new_request_id())
}

fn handle_change_message_visibility_batch(
    store: &mut SqsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let url = match params.get("QueueUrl") {
        Some(u) => u.clone(),
        None => return sqs_error("MissingParameter", "QueueUrl is required"),
    };
    let name = queue_name_from_url(&url);

    let mut inner = String::new();
    let mut i = 1;
    loop {
        let id_key = format!("ChangeMessageVisibilityBatchRequestEntry.{i}.Id");
        let rh_key = format!("ChangeMessageVisibilityBatchRequestEntry.{i}.ReceiptHandle");
        let vt_key = format!("ChangeMessageVisibilityBatchRequestEntry.{i}.VisibilityTimeout");
        match (params.get(&id_key), params.get(&rh_key)) {
            (Some(id), Some(rh)) => {
                let vt: u32 = params
                    .get(&vt_key)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30);
                if let Some(q) = store.get_queue_by_name_mut(&name) {
                    q.change_visibility(rh, vt);
                }
                inner.push_str(&format!(
                    "<ChangeMessageVisibilityBatchResultEntry><Id>{}</Id></ChangeMessageVisibilityBatchResultEntry>",
                    escape_xml(id)
                ));
                i += 1;
            }
            _ => break,
        }
    }
    xml_wrap("ChangeMessageVisibilityBatch", &new_request_id(), &inner)
}

// ---------------------------------------------------------------------------
// ServiceProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SqsProvider {
    fn service_name(&self) -> &str {
        "sqs"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let op_start = std::time::Instant::now();
        let mut params = parse_params(ctx);
        apply_transport_compat(ctx, &mut params);
        let query_mode_json = ctx
            .headers
            .get("x-amzn-query-mode")
            .map(|v| v == "true")
            .unwrap_or(false);

        let action = match params
            .get("Action")
            .or_else(|| ctx.query_params.get("Action"))
        {
            Some(a) => a.clone(),
            None => {
                warn!(service = "sqs", "Missing Action parameter");
                return Err(DispatchError::NotImplemented(
                    "(missing Action)".to_string(),
                ));
            }
        };

        debug!(service = "sqs", action = %action, "SQS dispatch");

        let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);

        let response = if query_mode_json {
            match action.as_str() {
                "CreateQueue" => {
                    let name = match params.get("QueueName") {
                        Some(n) => n.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "QueueName is required",
                                400,
                            ));
                        }
                    };
                    let attributes = extract_indexed_kv(&params, "Attribute");
                    let base = base_queue_url(ctx);
                    let q =
                        store.create_queue(&name, &base, &ctx.account_id, &ctx.region, &attributes);
                    sqs_json_response(serde_json::json!({ "QueueUrl": q.url.clone() }))
                }
                "GetQueueUrl" => {
                    let name = match params.get("QueueName") {
                        Some(n) => n.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "QueueName is required",
                                400,
                            ));
                        }
                    };
                    match store.get_queue_by_name(&name) {
                        Some(q) => {
                            sqs_json_response(serde_json::json!({ "QueueUrl": q.url.clone() }))
                        }
                        None => sqs_json_error(
                            "AWS.SimpleQueueService.NonExistentQueue",
                            "The specified queue does not exist.",
                            400,
                        ),
                    }
                }
                "SendMessage" => {
                    let url = match params.get("QueueUrl") {
                        Some(u) => u.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "QueueUrl is required",
                                400,
                            ));
                        }
                    };
                    let body = match params.get("MessageBody") {
                        Some(b) => b.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "MessageBody is required",
                                400,
                            ));
                        }
                    };
                    let name = queue_name_from_url(&url);
                    let delay = params.get("DelaySeconds").and_then(|v| v.parse().ok());
                    let message_group_id = params.get("MessageGroupId").cloned();
                    let dedup_id = params.get("MessageDeduplicationId").cloned();
                    let msg_attrs = extract_message_attributes(&params);
                    let q = match store.get_queue_by_name_mut(&name) {
                        Some(q) => q,
                        None => {
                            return Ok(sqs_json_error(
                                "AWS.SimpleQueueService.NonExistentQueue",
                                "The specified queue does not exist.",
                                400,
                            ));
                        }
                    };
                    let msg =
                        match q.send_message(body, delay, msg_attrs, message_group_id, dedup_id) {
                            Some(m) => m,
                            None => {
                                return Ok(sqs_json_response(serde_json::json!({
                                    "MessageId": "duplicate",
                                    "MD5OfMessageBody": "",
                                })));
                            }
                        };
                    sqs_json_response(serde_json::json!({
                        "MessageId": msg.message_id,
                        "MD5OfMessageBody": msg.md5_of_body,
                    }))
                }
                "ReceiveMessage" => {
                    let url = match params.get("QueueUrl") {
                        Some(u) => u.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "QueueUrl is required",
                                400,
                            ));
                        }
                    };
                    let max: usize = params
                        .get("MaxNumberOfMessages")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                        .min(10);
                    let vt: Option<u32> =
                        params.get("VisibilityTimeout").and_then(|v| v.parse().ok());
                    let name = queue_name_from_url(&url);
                    let q = match store.get_queue_by_name_mut(&name) {
                        Some(q) => q,
                        None => {
                            return Ok(sqs_json_error(
                                "AWS.SimpleQueueService.NonExistentQueue",
                                "The specified queue does not exist.",
                                400,
                            ));
                        }
                    };
                    let messages = q.receive_messages(max, vt);
                    let payload = messages
                        .iter()
                        .map(|msg| {
                            serde_json::json!({
                                "MessageId": msg.message_id,
                                "ReceiptHandle": msg.receipt_handle,
                                "MD5OfBody": msg.md5_of_body,
                                "Body": msg.body,
                            })
                        })
                        .collect::<Vec<_>>();
                    sqs_json_response(serde_json::json!({ "Messages": payload }))
                }
                "DeleteQueue" => {
                    let url = match params.get("QueueUrl") {
                        Some(u) => u.clone(),
                        None => {
                            return Ok(sqs_json_error(
                                "MissingParameter",
                                "QueueUrl is required",
                                400,
                            ));
                        }
                    };
                    let name = queue_name_from_url(&url);
                    if store.delete_queue(&name) {
                        sqs_json_response(serde_json::json!({}))
                    } else {
                        sqs_json_error(
                            "AWS.SimpleQueueService.NonExistentQueue",
                            "The specified queue does not exist.",
                            400,
                        )
                    }
                }
                _ => {
                    warn!(service = "sqs", action = %action, "SQS action not implemented");
                    return Err(DispatchError::NotImplemented(action));
                }
            }
        } else {
            match action.as_str() {
                "CreateQueue" => handle_create_queue(&mut store, ctx, &params),
                "DeleteQueue" => handle_delete_queue(&mut store, &params),
                "ListQueues" => handle_list_queues(&store, &params),
                "GetQueueUrl" => handle_get_queue_url(&store, &params),
                "GetQueueAttributes" => handle_get_queue_attributes(&store, &params),
                "SetQueueAttributes" => handle_set_queue_attributes(&mut store, &params),
                "PurgeQueue" => handle_purge_queue(&mut store, &params),
                "SendMessage" => handle_send_message(&mut store, &params),
                "SendMessageBatch" => handle_send_message_batch(&mut store, &params),
                "ReceiveMessage" => handle_receive_message(&mut store, &params),
                "DeleteMessage" => handle_delete_message(&mut store, &params),
                "DeleteMessageBatch" => handle_delete_message_batch(&mut store, &params),
                "ChangeMessageVisibility" => handle_change_message_visibility(&mut store, &params),
                "ChangeMessageVisibilityBatch" => {
                    handle_change_message_visibility_batch(&mut store, &params)
                }
                _ => {
                    warn!(service = "sqs", action = %action, "SQS action not implemented");
                    return Err(DispatchError::NotImplemented(action));
                }
            }
        };

        debug!(
            service = "sqs",
            action = %action,
            op_latency_us = op_start.elapsed().as_micros(),
            "SQS operation complete"
        );

        Ok(response)
    }
}
