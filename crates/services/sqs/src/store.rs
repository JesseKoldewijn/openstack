use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Message attribute value
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttributeValue {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// SqsMessage — a single message in a queue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqsMessage {
    pub message_id: String,
    pub receipt_handle: String,
    pub body: String,
    pub md5_of_body: String,
    pub attributes: HashMap<String, String>, // system attributes
    pub message_attributes: HashMap<String, MessageAttributeValue>,
    pub sent_at: DateTime<Utc>,
    /// When this message becomes visible again (None = immediately visible)
    pub visible_after: Option<DateTime<Utc>>,
    /// How many times this message has been received
    pub receive_count: u32,
    /// Delay before the message becomes initially visible (seconds since sent_at)
    pub delay_seconds: u32,
    /// For FIFO: message group
    pub message_group_id: Option<String>,
    /// For FIFO: deduplication id
    pub message_deduplication_id: Option<String>,
    /// Sequence number (used for FIFO ordering)
    pub sequence_number: u64,
}

impl SqsMessage {
    pub fn new(
        body: impl Into<String>,
        delay_seconds: u32,
        message_attributes: HashMap<String, MessageAttributeValue>,
        message_group_id: Option<String>,
        dedup_id: Option<String>,
        sequence_number: u64,
    ) -> Self {
        let body = body.into();
        let md5 = md5_hex(body.as_bytes());
        let message_id = Uuid::new_v4().to_string();
        let receipt_handle = Uuid::new_v4().to_string();
        let visible_after = if delay_seconds > 0 {
            Some(Utc::now() + chrono::Duration::seconds(delay_seconds as i64))
        } else {
            None
        };

        let mut attributes = HashMap::new();
        attributes.insert(
            "ApproximateFirstReceiveTimestamp".to_string(),
            "0".to_string(),
        );
        attributes.insert("ApproximateReceiveCount".to_string(), "0".to_string());
        attributes.insert(
            "SentTimestamp".to_string(),
            Utc::now().timestamp_millis().to_string(),
        );

        Self {
            message_id,
            receipt_handle,
            body,
            md5_of_body: md5,
            attributes,
            message_attributes,
            sent_at: Utc::now(),
            visible_after,
            receive_count: 0,
            delay_seconds,
            message_group_id,
            message_deduplication_id: dedup_id,
            sequence_number,
        }
    }

    pub fn is_visible(&self) -> bool {
        match self.visible_after {
            None => true,
            Some(t) => Utc::now() >= t,
        }
    }

    /// Make a new receipt handle for this receive attempt and set visibility timeout.
    pub fn begin_receive(&mut self, visibility_timeout_secs: u32) {
        self.receipt_handle = Uuid::new_v4().to_string();
        self.receive_count += 1;
        self.attributes.insert(
            "ApproximateReceiveCount".to_string(),
            self.receive_count.to_string(),
        );
        if visibility_timeout_secs > 0 {
            self.visible_after =
                Some(Utc::now() + chrono::Duration::seconds(visibility_timeout_secs as i64));
        } else {
            self.visible_after = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Redrive policy (DLQ)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedrivePolicy {
    pub dead_letter_target_arn: String,
    pub max_receive_count: u32,
}

// ---------------------------------------------------------------------------
// SqsQueue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqsQueue {
    pub name: String,
    pub url: String,
    pub arn: String,
    pub created: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
    /// Default visibility timeout in seconds
    pub visibility_timeout: u32,
    /// Default message retention period in seconds
    pub message_retention_period: u32,
    /// Maximum message size in bytes
    pub maximum_message_size: u32,
    /// Default delay seconds
    pub delay_seconds: u32,
    /// Receive message wait time seconds (long poll)
    pub receive_message_wait_time_seconds: u32,
    /// FIFO queue?
    pub fifo: bool,
    /// Content-based deduplication (for FIFO)
    pub content_based_deduplication: bool,
    /// Redrive / DLQ policy
    pub redrive_policy: Option<RedrivePolicy>,
    /// Queue policy JSON
    pub policy: Option<String>,
    /// The message queue — order matters for FIFO
    pub messages: VecDeque<SqsMessage>,
    /// Counter for sequence numbers
    pub sequence_counter: u64,
    /// Set of deduplication IDs seen (FIFO only), expires in 5 min but we keep forever for simplicity
    pub dedup_ids: HashMap<String, String>, // dedup_id → message_id
}

impl SqsQueue {
    pub fn new(name: impl Into<String>, base_url: &str, account_id: &str, region: &str) -> Self {
        let name = name.into();
        let is_fifo = name.ends_with(".fifo");
        let url = format!("{base_url}/{account_id}/{name}");
        let arn = format!("arn:aws:sqs:{region}:{account_id}:{name}");
        Self {
            name: name.clone(),
            url,
            arn,
            created: Utc::now(),
            last_modified: Utc::now(),
            visibility_timeout: 30,
            message_retention_period: 345600, // 4 days
            maximum_message_size: 262144,     // 256 KiB
            delay_seconds: 0,
            receive_message_wait_time_seconds: 0,
            fifo: is_fifo,
            content_based_deduplication: false,
            redrive_policy: None,
            policy: None,
            messages: VecDeque::new(),
            sequence_counter: 0,
            dedup_ids: HashMap::new(),
        }
    }

    /// Send a message to the queue. Returns the new message if accepted (may be deduplicated).
    pub fn send_message(
        &mut self,
        body: impl Into<String>,
        delay_override: Option<u32>,
        message_attributes: HashMap<String, MessageAttributeValue>,
        message_group_id: Option<String>,
        dedup_id: Option<String>,
    ) -> Option<SqsMessage> {
        let delay = delay_override.unwrap_or(self.delay_seconds);

        // FIFO deduplication
        if self.fifo
            && let Some(ref did) = dedup_id
            && self.dedup_ids.contains_key(did.as_str())
        {
            return None; // duplicate
        }

        self.sequence_counter += 1;
        let seq = self.sequence_counter;
        let msg = SqsMessage::new(
            body,
            delay,
            message_attributes,
            message_group_id,
            dedup_id.clone(),
            seq,
        );

        if let Some(did) = dedup_id {
            self.dedup_ids.insert(did, msg.message_id.clone());
        }

        self.messages.push_back(msg.clone());
        Some(msg)
    }

    /// Receive up to `max_number` visible messages, applying visibility timeout.
    pub fn receive_messages(
        &mut self,
        max_number: usize,
        visibility_timeout: Option<u32>,
    ) -> Vec<SqsMessage> {
        let vt = visibility_timeout.unwrap_or(self.visibility_timeout);
        let mut received = Vec::new();

        for msg in self.messages.iter_mut() {
            if received.len() >= max_number {
                break;
            }
            if msg.is_visible() {
                msg.begin_receive(vt);
                received.push(msg.clone());
            }
        }
        received
    }

    /// Delete a message by receipt handle. Returns true if found.
    pub fn delete_message(&mut self, receipt_handle: &str) -> bool {
        let before = self.messages.len();
        self.messages.retain(|m| m.receipt_handle != receipt_handle);
        self.messages.len() < before
    }

    /// Change visibility timeout for a message by receipt handle.
    pub fn change_visibility(&mut self, receipt_handle: &str, visibility_timeout: u32) -> bool {
        for msg in self.messages.iter_mut() {
            if msg.receipt_handle == receipt_handle {
                if visibility_timeout == 0 {
                    msg.visible_after = None;
                } else {
                    msg.visible_after =
                        Some(Utc::now() + chrono::Duration::seconds(visibility_timeout as i64));
                }
                return true;
            }
        }
        false
    }

    /// Purge all messages.
    pub fn purge(&mut self) {
        self.messages.clear();
    }

    /// Approximate number of visible messages.
    pub fn approximate_number_of_messages(&self) -> usize {
        self.messages.iter().filter(|m| m.is_visible()).count()
    }

    pub fn approximate_number_of_messages_not_visible(&self) -> usize {
        self.messages.iter().filter(|m| !m.is_visible()).count()
    }

    /// Check DLQ redrive: move messages that exceed max receive count.
    /// Returns a list of (receipt_handle, body) that should be sent to DLQ.
    pub fn messages_for_dlq(&self) -> Vec<SqsMessage> {
        if let Some(ref rp) = self.redrive_policy {
            self.messages
                .iter()
                .filter(|m| m.receive_count >= rp.max_receive_count)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn remove_dlq_candidates(&mut self) {
        if let Some(ref rp) = self.redrive_policy {
            let max = rp.max_receive_count;
            self.messages.retain(|m| m.receive_count < max);
        }
    }
}

// ---------------------------------------------------------------------------
// SqsStore
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SqsStore {
    /// queue_url (or queue_name) → SqsQueue
    pub queues: HashMap<String, SqsQueue>,
}

impl SqsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_queue(
        &mut self,
        name: impl Into<String>,
        base_url: &str,
        account_id: &str,
        region: &str,
        attributes: &HashMap<String, String>,
    ) -> &SqsQueue {
        let name = name.into();
        if !self.queues.contains_key(&name) {
            let mut q = SqsQueue::new(&name, base_url, account_id, region);
            apply_queue_attributes(&mut q, attributes);
            self.queues.insert(name.clone(), q);
        }
        self.queues.get(&name).unwrap()
    }

    pub fn get_queue_by_name(&self, name: &str) -> Option<&SqsQueue> {
        self.queues.get(name)
    }

    pub fn get_queue_by_name_mut(&mut self, name: &str) -> Option<&mut SqsQueue> {
        self.queues.get_mut(name)
    }

    pub fn get_queue_by_url(&self, url: &str) -> Option<&SqsQueue> {
        self.queues.values().find(|q| q.url == url)
    }

    pub fn get_queue_by_url_mut(&mut self, url: &str) -> Option<&mut SqsQueue> {
        self.queues.values_mut().find(|q| q.url == url)
    }

    pub fn queue_name_from_url(&self, url: &str) -> Option<String> {
        // URL format: http://..../account_id/queue_name
        url.rsplit('/').next().map(|s| s.to_string())
    }

    pub fn delete_queue(&mut self, name: &str) -> bool {
        self.queues.remove(name).is_some()
    }

    pub fn list_queues(&self, prefix: &str) -> Vec<&SqsQueue> {
        self.queues
            .values()
            .filter(|q| q.name.starts_with(prefix))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn apply_queue_attributes(q: &mut SqsQueue, attrs: &HashMap<String, String>) {
    if let Some(v) = attrs.get("VisibilityTimeout")
        && let Ok(n) = v.parse()
    {
        q.visibility_timeout = n;
    }
    if let Some(v) = attrs.get("MessageRetentionPeriod")
        && let Ok(n) = v.parse()
    {
        q.message_retention_period = n;
    }
    if let Some(v) = attrs.get("MaximumMessageSize")
        && let Ok(n) = v.parse()
    {
        q.maximum_message_size = n;
    }
    if let Some(v) = attrs.get("DelaySeconds")
        && let Ok(n) = v.parse()
    {
        q.delay_seconds = n;
    }
    if let Some(v) = attrs.get("ReceiveMessageWaitTimeSeconds")
        && let Ok(n) = v.parse()
    {
        q.receive_message_wait_time_seconds = n;
    }
    if let Some(v) = attrs.get("ContentBasedDeduplication") {
        q.content_based_deduplication = v == "true";
    }
    if let Some(v) = attrs.get("RedrivePolicy")
        && let Ok(rp) = serde_json::from_str::<serde_json::Value>(v)
    {
        let arn = rp["deadLetterTargetArn"].as_str().unwrap_or("").to_string();
        let max: u32 = rp["maxReceiveCount"].as_u64().unwrap_or(3) as u32;
        q.redrive_policy = Some(RedrivePolicy {
            dead_letter_target_arn: arn,
            max_receive_count: max,
        });
    }
    q.last_modified = Utc::now();
}

pub fn md5_hex(data: &[u8]) -> String {
    use sha2::Digest;
    // Use SHA-256 truncated to 16 bytes as a stand-in for MD5 (no md-5 crate in workspace)
    let digest = sha2::Sha256::digest(data);
    hex::encode(&digest[..16])
}
