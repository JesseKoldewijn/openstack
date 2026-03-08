use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use tracing::{debug, warn};

use crate::store::{FilterPolicy, MessageAttribute, Protocol, SnsStore};

pub struct SnsProvider {
    store: Arc<AccountRegionBundle<SnsStore>>,
}

impl SnsProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for SnsProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// XML helpers (SNS uses query protocol — same as SQS)
// ---------------------------------------------------------------------------

fn xml_wrap(action: &str, request_id: &str, inner: &str) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<{action}Response xmlns=\"https://sns.amazonaws.com/doc/2010-03-31/\">\
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
<{action}Response xmlns=\"https://sns.amazonaws.com/doc/2010-03-31/\">\
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

fn sns_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<ErrorResponse xmlns=\"https://sns.amazonaws.com/doc/2010-03-31/\">\
<Error><Type>Sender</Type><Code>{code}</Code><Message>{message}</Message></Error>\
</ErrorResponse>"
    );
    DispatchResponse {
        status_code: status,
        body: Bytes::from(xml.into_bytes()),
        content_type: "text/xml".to_string(),
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
// Query protocol parsing (same approach as SQS provider)
// ---------------------------------------------------------------------------

fn parse_params(ctx: &RequestContext) -> HashMap<String, String> {
    let body_str = std::str::from_utf8(&ctx.raw_body).unwrap_or("");
    let mut params: HashMap<String, String> = body_str
        .split('&')
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?;
            let v = it.next().unwrap_or("");
            if k.is_empty() {
                None
            } else {
                Some((url_decode(k), url_decode(v)))
            }
        })
        .collect();
    for (k, v) in &ctx.query_params {
        params.entry(k.clone()).or_insert_with(|| v.clone());
    }
    params
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

fn extract_message_attributes(
    params: &HashMap<String, String>,
) -> HashMap<String, MessageAttribute> {
    let mut result = HashMap::new();
    let mut i = 1;
    loop {
        let name_key = format!("MessageAttributes.entry.{i}.Name");
        let type_key = format!("MessageAttributes.entry.{i}.Value.DataType");
        let str_key = format!("MessageAttributes.entry.{i}.Value.StringValue");
        match params.get(&name_key) {
            None => break,
            Some(name) => {
                let data_type = params.get(&type_key).cloned().unwrap_or_default();
                let string_value = params.get(&str_key).cloned();
                result.insert(
                    name.clone(),
                    MessageAttribute {
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

// ---------------------------------------------------------------------------
// Operation handlers
// ---------------------------------------------------------------------------

fn handle_create_topic(
    store: &mut SnsStore,
    ctx: &RequestContext,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let name = match params.get("Name") {
        Some(n) => n.clone(),
        None => return sns_error("InvalidParameter", "Name is required", 400),
    };
    let attrs = extract_indexed_kv(params, "Attributes.entry");
    let topic = store.create_topic(&name, &ctx.account_id, &ctx.region, &attrs);
    let arn = topic.topic_arn.clone();
    xml_wrap(
        "CreateTopic",
        &new_request_id(),
        &format!("<TopicArn>{}</TopicArn>", escape_xml(&arn)),
    )
}

fn handle_delete_topic(store: &mut SnsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    if store.delete_topic(&arn) {
        xml_no_result("DeleteTopic", &new_request_id())
    } else {
        sns_error("NotFound", "Topic not found", 404)
    }
}

fn handle_list_topics(store: &SnsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let _ = params.get("NextToken"); // pagination not implemented — return all
    let topics = store.list_topics();
    let mut inner = String::new();
    inner.push_str("<Topics>");
    for t in &topics {
        inner.push_str(&format!(
            "<member><TopicArn>{}</TopicArn></member>",
            escape_xml(&t.topic_arn)
        ));
    }
    inner.push_str("</Topics>");
    xml_wrap("ListTopics", &new_request_id(), &inner)
}

fn handle_get_topic_attributes(
    store: &SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    let topic = match store.get_topic(&arn) {
        Some(t) => t,
        None => return sns_error("NotFound", "Topic not found", 404),
    };

    let sub_count = store.list_subscriptions_by_topic(&arn).len();

    let mut inner = String::from("<Attributes>");
    let attrs = [
        ("TopicArn", topic.topic_arn.clone()),
        ("DisplayName", topic.display_name.clone()),
        ("Owner", topic.owner.clone()),
        ("SubscriptionsConfirmed", sub_count.to_string()),
        ("SubscriptionsPending", "0".to_string()),
        ("SubscriptionsDeleted", "0".to_string()),
        ("FifoTopic", topic.fifo.to_string()),
        (
            "ContentBasedDeduplication",
            topic.content_based_deduplication.to_string(),
        ),
    ];
    for (k, v) in &attrs {
        inner.push_str(&format!(
            "<entry><key>{k}</key><value>{}</value></entry>",
            escape_xml(v)
        ));
    }
    if let Some(policy) = &topic.policy {
        inner.push_str(&format!(
            "<entry><key>Policy</key><value>{}</value></entry>",
            escape_xml(policy)
        ));
    }
    inner.push_str("</Attributes>");
    xml_wrap("GetTopicAttributes", &new_request_id(), &inner)
}

fn handle_set_topic_attributes(
    store: &mut SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    let attr_name = match params.get("AttributeName") {
        Some(n) => n.clone(),
        None => return sns_error("InvalidParameter", "AttributeName is required", 400),
    };
    let attr_value = params.get("AttributeValue").cloned().unwrap_or_default();
    let topic = match store.get_topic_mut(&arn) {
        Some(t) => t,
        None => return sns_error("NotFound", "Topic not found", 404),
    };
    match attr_name.as_str() {
        "DisplayName" => topic.display_name = attr_value,
        "Policy" => topic.policy = Some(attr_value),
        "DeliveryPolicy" => topic.delivery_policy = Some(attr_value),
        "KmsMasterKeyId" => topic.kms_master_key_id = Some(attr_value),
        "ContentBasedDeduplication" => topic.content_based_deduplication = attr_value == "true",
        _ => {}
    }
    xml_no_result("SetTopicAttributes", &new_request_id())
}

fn handle_subscribe(
    store: &mut SnsStore,
    ctx: &RequestContext,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let topic_arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    let protocol_str = match params.get("Protocol") {
        Some(p) => p.clone(),
        None => return sns_error("InvalidParameter", "Protocol is required", 400),
    };
    let endpoint = params.get("Endpoint").cloned().unwrap_or_default();
    let protocol = match Protocol::parse(&protocol_str) {
        Some(p) => p,
        None => return sns_error("InvalidParameter", "Unsupported protocol", 400),
    };

    match store.subscribe(&topic_arn, protocol, &endpoint, &ctx.account_id) {
        None => sns_error("NotFound", "Topic not found", 404),
        Some(sub_arn) => xml_wrap(
            "Subscribe",
            &new_request_id(),
            &format!(
                "<SubscriptionArn>{}</SubscriptionArn>",
                escape_xml(&sub_arn)
            ),
        ),
    }
}

fn handle_unsubscribe(store: &mut SnsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let arn = match params.get("SubscriptionArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "SubscriptionArn is required", 400),
    };
    if store.unsubscribe(&arn) {
        xml_no_result("Unsubscribe", &new_request_id())
    } else {
        sns_error("NotFound", "Subscription not found", 404)
    }
}

fn handle_list_subscriptions(
    store: &SnsStore,
    _params: &HashMap<String, String>,
) -> DispatchResponse {
    let subs = store.list_subscriptions();
    let mut inner = String::from("<Subscriptions>");
    for s in &subs {
        inner.push_str(&format!(
            "<member>\
<SubscriptionArn>{}</SubscriptionArn>\
<Owner>{}</Owner>\
<Protocol>{}</Protocol>\
<Endpoint>{}</Endpoint>\
<TopicArn>{}</TopicArn>\
</member>",
            escape_xml(&s.subscription_arn),
            escape_xml(&s.owner),
            escape_xml(s.protocol.as_str()),
            escape_xml(&s.endpoint),
            escape_xml(&s.topic_arn),
        ));
    }
    inner.push_str("</Subscriptions>");
    xml_wrap("ListSubscriptions", &new_request_id(), &inner)
}

fn handle_list_subscriptions_by_topic(
    store: &SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let topic_arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    if store.get_topic(&topic_arn).is_none() {
        return sns_error("NotFound", "Topic not found", 404);
    }
    let subs = store.list_subscriptions_by_topic(&topic_arn);
    let mut inner = String::from("<Subscriptions>");
    for s in &subs {
        inner.push_str(&format!(
            "<member>\
<SubscriptionArn>{}</SubscriptionArn>\
<Owner>{}</Owner>\
<Protocol>{}</Protocol>\
<Endpoint>{}</Endpoint>\
<TopicArn>{}</TopicArn>\
</member>",
            escape_xml(&s.subscription_arn),
            escape_xml(&s.owner),
            escape_xml(s.protocol.as_str()),
            escape_xml(&s.endpoint),
            escape_xml(&s.topic_arn),
        ));
    }
    inner.push_str("</Subscriptions>");
    xml_wrap("ListSubscriptionsByTopic", &new_request_id(), &inner)
}

fn handle_get_subscription_attributes(
    store: &SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let arn = match params.get("SubscriptionArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "SubscriptionArn is required", 400),
    };
    let sub = match store.get_subscription(&arn) {
        Some(s) => s,
        None => return sns_error("NotFound", "Subscription not found", 404),
    };

    let mut inner = String::from("<Attributes>");
    let attrs = [
        ("SubscriptionArn", sub.subscription_arn.clone()),
        ("TopicArn", sub.topic_arn.clone()),
        ("Protocol", sub.protocol.as_str().to_string()),
        ("Endpoint", sub.endpoint.clone()),
        ("Owner", sub.owner.clone()),
        ("RawMessageDelivery", sub.raw_message_delivery.to_string()),
        ("PendingConfirmation", sub.pending_confirmation.to_string()),
    ];
    for (k, v) in &attrs {
        inner.push_str(&format!(
            "<entry><key>{k}</key><value>{}</value></entry>",
            escape_xml(v)
        ));
    }
    inner.push_str("</Attributes>");
    xml_wrap("GetSubscriptionAttributes", &new_request_id(), &inner)
}

fn handle_set_subscription_attributes(
    store: &mut SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let arn = match params.get("SubscriptionArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "SubscriptionArn is required", 400),
    };
    let attr_name = match params.get("AttributeName") {
        Some(n) => n.clone(),
        None => return sns_error("InvalidParameter", "AttributeName is required", 400),
    };
    let attr_value = params.get("AttributeValue").cloned().unwrap_or_default();
    let sub = match store.get_subscription_mut(&arn) {
        Some(s) => s,
        None => return sns_error("NotFound", "Subscription not found", 404),
    };
    match attr_name.as_str() {
        "RawMessageDelivery" => sub.raw_message_delivery = attr_value == "true",
        "FilterPolicy" => {
            sub.filter_policy = FilterPolicy::from_json(&attr_value);
        }
        _ => {}
    }
    xml_no_result("SetSubscriptionAttributes", &new_request_id())
}

fn handle_publish(store: &mut SnsStore, params: &HashMap<String, String>) -> DispatchResponse {
    let topic_arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    if store.get_topic(&topic_arn).is_none() {
        return sns_error("NotFound", "Topic not found", 404);
    }

    let message = match params.get("Message") {
        Some(m) => m.clone(),
        None => return sns_error("InvalidParameter", "Message is required", 400),
    };
    let subject = params.get("Subject").cloned().unwrap_or_default();
    let message_id = uuid::Uuid::new_v4().to_string();
    let message_attributes = extract_message_attributes(params);

    // Collect subscriptions and delivery info before mutating store
    let delivery_tasks: Vec<(String, String, String, bool, Option<FilterPolicy>)> = store
        .list_subscriptions_by_topic(&topic_arn)
        .into_iter()
        .map(|s| {
            (
                s.subscription_arn.clone(),
                s.protocol.as_str().to_string(),
                s.endpoint.clone(),
                s.raw_message_delivery,
                s.filter_policy.clone(),
            )
        })
        .collect();

    // Deliver to each subscriber
    for (_sub_arn, protocol_str, endpoint, raw_delivery, filter_policy) in &delivery_tasks {
        // Apply filter policy
        if let Some(fp) = filter_policy
            && !fp.matches(&message_attributes)
        {
            continue;
        }

        match protocol_str.as_str() {
            "sqs" => {
                // SQS delivery: format SNS notification envelope and deliver to SQS queue
                // The endpoint is the SQS queue ARN. For in-process delivery we just log it.
                // Real delivery would require access to the SQS store — cross-service
                // delivery is done by the gateway layer. Here we just record it.
                let payload = if *raw_delivery {
                    message.clone()
                } else {
                    format_sns_notification(&topic_arn, &message_id, &message, &subject)
                };
                debug!(
                    protocol = "sqs",
                    endpoint = %endpoint,
                    payload_len = payload.len(),
                    "SNS → SQS delivery (in-process)"
                );
            }
            "http" | "https" => {
                // HTTP delivery: fire-and-forget POST in background
                let payload = format_sns_notification(&topic_arn, &message_id, &message, &subject);
                let endpoint = endpoint.clone();
                tokio::spawn(async move {
                    // Minimal HTTP POST — in production this would use reqwest
                    debug!(
                        protocol = "http",
                        endpoint = %endpoint,
                        payload_len = payload.len(),
                        "SNS → HTTP delivery (stub)"
                    );
                });
            }
            "lambda" => {
                debug!(
                    protocol = "lambda",
                    endpoint = %endpoint,
                    "SNS → Lambda delivery (stub)"
                );
            }
            _ => {
                debug!(
                    protocol = %protocol_str,
                    endpoint = %endpoint,
                    "SNS → unsupported protocol (skipping)"
                );
            }
        }
    }

    xml_wrap(
        "Publish",
        &new_request_id(),
        &format!("<MessageId>{}</MessageId>", escape_xml(&message_id)),
    )
}

fn handle_publish_batch(
    store: &mut SnsStore,
    params: &HashMap<String, String>,
) -> DispatchResponse {
    let topic_arn = match params.get("TopicArn") {
        Some(a) => a.clone(),
        None => return sns_error("InvalidParameter", "TopicArn is required", 400),
    };
    if store.get_topic(&topic_arn).is_none() {
        return sns_error("NotFound", "Topic not found", 404);
    }

    let mut successful = String::new();
    let mut i = 1;
    loop {
        let id_key = format!("PublishBatchRequestEntries.member.{i}.Id");
        let msg_key = format!("PublishBatchRequestEntries.member.{i}.Message");
        match (params.get(&id_key), params.get(&msg_key)) {
            (Some(id), Some(_msg)) => {
                let message_id = uuid::Uuid::new_v4().to_string();
                successful.push_str(&format!(
                    "<member><Id>{}</Id><MessageId>{}</MessageId></member>",
                    escape_xml(id),
                    escape_xml(&message_id),
                ));
                i += 1;
            }
            _ => break,
        }
    }

    let inner = format!("<Successful>{successful}</Successful><Failed></Failed>");
    xml_wrap("PublishBatch", &new_request_id(), &inner)
}

// ---------------------------------------------------------------------------
// SNS notification envelope
// ---------------------------------------------------------------------------

fn format_sns_notification(
    topic_arn: &str,
    message_id: &str,
    message: &str,
    subject: &str,
) -> String {
    serde_json::json!({
        "Type": "Notification",
        "MessageId": message_id,
        "TopicArn": topic_arn,
        "Subject": subject,
        "Message": message,
        "Timestamp": chrono::Utc::now().to_rfc3339(),
        "SignatureVersion": "1",
        "Signature": "EXAMPLE",
        "SigningCertURL": "",
        "UnsubscribeURL": "",
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// ServiceProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for SnsProvider {
    fn service_name(&self) -> &str {
        "sns"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let params = parse_params(ctx);

        let action = match params
            .get("Action")
            .or_else(|| ctx.query_params.get("Action"))
        {
            Some(a) => a.clone(),
            None => {
                warn!(service = "sns", "Missing Action parameter");
                return Err(DispatchError::NotImplemented(
                    "(missing Action)".to_string(),
                ));
            }
        };

        debug!(service = "sns", action = %action, "SNS dispatch");

        let mut store = self.store.get_or_create(&ctx.account_id, &ctx.region);

        let response = match action.as_str() {
            "CreateTopic" => handle_create_topic(&mut store, ctx, &params),
            "DeleteTopic" => handle_delete_topic(&mut store, &params),
            "ListTopics" => handle_list_topics(&store, &params),
            "GetTopicAttributes" => handle_get_topic_attributes(&store, &params),
            "SetTopicAttributes" => handle_set_topic_attributes(&mut store, &params),
            "Subscribe" => handle_subscribe(&mut store, ctx, &params),
            "Unsubscribe" => handle_unsubscribe(&mut store, &params),
            "ListSubscriptions" => handle_list_subscriptions(&store, &params),
            "ListSubscriptionsByTopic" => handle_list_subscriptions_by_topic(&store, &params),
            "GetSubscriptionAttributes" => handle_get_subscription_attributes(&store, &params),
            "SetSubscriptionAttributes" => handle_set_subscription_attributes(&mut store, &params),
            "Publish" => handle_publish(&mut store, &params),
            "PublishBatch" => handle_publish_batch(&mut store, &params),
            _ => {
                warn!(service = "sns", action = %action, "SNS action not implemented");
                return Err(DispatchError::NotImplemented(action));
            }
        };

        Ok(response)
    }
}
