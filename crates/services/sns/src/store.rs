use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Subscription filter policy
// ---------------------------------------------------------------------------

/// A subscription filter policy: map of attribute name -> list of allowed values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterPolicy(pub HashMap<String, Vec<serde_json::Value>>);

impl FilterPolicy {
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str::<HashMap<String, Vec<serde_json::Value>>>(json)
            .ok()
            .map(FilterPolicy)
    }

    /// Returns true if the given message attributes pass this filter policy.
    pub fn matches(&self, attributes: &HashMap<String, MessageAttribute>) -> bool {
        for (attr_name, allowed_values) in &self.0 {
            let attr = match attributes.get(attr_name) {
                None => return false,
                Some(a) => a,
            };
            let attr_val_str = attr.string_value.as_deref().unwrap_or("");
            let passes = allowed_values.iter().any(|v| {
                if let Some(s) = v.as_str() {
                    s == attr_val_str
                } else {
                    false
                }
            });
            if !passes {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Message attribute
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttribute {
    pub data_type: String,
    pub string_value: Option<String>,
    pub binary_value: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Subscription
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Sqs,
    Http,
    Https,
    Lambda,
    Email,
    EmailJson,
    Sms,
    Application,
    Firehose,
}

impl Protocol {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sqs" => Some(Protocol::Sqs),
            "http" => Some(Protocol::Http),
            "https" => Some(Protocol::Https),
            "lambda" => Some(Protocol::Lambda),
            "email" => Some(Protocol::Email),
            "email-json" => Some(Protocol::EmailJson),
            "sms" => Some(Protocol::Sms),
            "application" => Some(Protocol::Application),
            "firehose" => Some(Protocol::Firehose),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Sqs => "sqs",
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Lambda => "lambda",
            Protocol::Email => "email",
            Protocol::EmailJson => "email-json",
            Protocol::Sms => "sms",
            Protocol::Application => "application",
            Protocol::Firehose => "firehose",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub subscription_arn: String,
    pub topic_arn: String,
    pub protocol: Protocol,
    pub endpoint: String,
    pub owner: String,
    pub raw_message_delivery: bool,
    pub filter_policy: Option<FilterPolicy>,
    pub confirmation_was_authenticated: bool,
    pub pending_confirmation: bool,
    pub created: DateTime<Utc>,
}

impl Subscription {
    pub fn new(
        topic_arn: impl Into<String>,
        protocol: Protocol,
        endpoint: impl Into<String>,
        owner: impl Into<String>,
    ) -> Self {
        let sub_id = uuid::Uuid::new_v4().to_string();
        let topic_arn = topic_arn.into();
        let endpoint = endpoint.into();
        // arn:aws:sns:{region}:{account}:{topic_name}:{sub_id}
        let subscription_arn = format!("{topic_arn}:{sub_id}");
        Self {
            subscription_arn,
            topic_arn,
            protocol,
            endpoint,
            owner: owner.into(),
            raw_message_delivery: false,
            filter_policy: None,
            confirmation_was_authenticated: true,
            pending_confirmation: false,
            created: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Topic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub topic_arn: String,
    pub name: String,
    pub display_name: String,
    pub owner: String,
    pub region: String,
    pub created: DateTime<Utc>,
    pub fifo: bool,
    pub content_based_deduplication: bool,
    pub policy: Option<String>,
    pub delivery_policy: Option<String>,
    pub kms_master_key_id: Option<String>,
}

impl Topic {
    pub fn new(name: impl Into<String>, account_id: &str, region: &str) -> Self {
        let name = name.into();
        let fifo = name.ends_with(".fifo");
        let topic_arn = format!("arn:aws:sns:{region}:{account_id}:{name}");
        Self {
            topic_arn,
            name: name.clone(),
            display_name: name,
            owner: account_id.to_string(),
            region: region.to_string(),
            created: Utc::now(),
            fifo,
            content_based_deduplication: false,
            policy: None,
            delivery_policy: None,
            kms_master_key_id: None,
        }
    }
}

// ---------------------------------------------------------------------------
// SnsStore
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SnsStore {
    /// topic_arn → Topic
    pub topics: HashMap<String, Topic>,
    /// subscription_arn → Subscription
    pub subscriptions: HashMap<String, Subscription>,
}

impl SnsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_topic(
        &mut self,
        name: &str,
        account_id: &str,
        region: &str,
        attrs: &HashMap<String, String>,
    ) -> &Topic {
        let arn = format!("arn:aws:sns:{region}:{account_id}:{name}");
        if !self.topics.contains_key(&arn) {
            let mut topic = Topic::new(name, account_id, region);
            if let Some(v) = attrs.get("DisplayName") {
                topic.display_name = v.clone();
            }
            if let Some(v) = attrs.get("Policy") {
                topic.policy = Some(v.clone());
            }
            if let Some(v) = attrs.get("KmsMasterKeyId") {
                topic.kms_master_key_id = Some(v.clone());
            }
            if let Some(v) = attrs.get("ContentBasedDeduplication") {
                topic.content_based_deduplication = v == "true";
            }
            self.topics.insert(arn.clone(), topic);
        }
        self.topics.get(&arn).unwrap()
    }

    pub fn delete_topic(&mut self, topic_arn: &str) -> bool {
        if self.topics.remove(topic_arn).is_some() {
            // Remove all subscriptions for this topic
            self.subscriptions.retain(|_, s| s.topic_arn != topic_arn);
            true
        } else {
            false
        }
    }

    pub fn get_topic(&self, topic_arn: &str) -> Option<&Topic> {
        self.topics.get(topic_arn)
    }

    pub fn get_topic_mut(&mut self, topic_arn: &str) -> Option<&mut Topic> {
        self.topics.get_mut(topic_arn)
    }

    pub fn list_topics(&self) -> Vec<&Topic> {
        self.topics.values().collect()
    }

    pub fn subscribe(
        &mut self,
        topic_arn: &str,
        protocol: Protocol,
        endpoint: &str,
        account_id: &str,
    ) -> Option<String> {
        if !self.topics.contains_key(topic_arn) {
            return None;
        }
        let sub = Subscription::new(topic_arn, protocol, endpoint, account_id);
        let arn = sub.subscription_arn.clone();
        self.subscriptions.insert(arn.clone(), sub);
        Some(arn)
    }

    pub fn unsubscribe(&mut self, subscription_arn: &str) -> bool {
        self.subscriptions.remove(subscription_arn).is_some()
    }

    pub fn get_subscription(&self, subscription_arn: &str) -> Option<&Subscription> {
        self.subscriptions.get(subscription_arn)
    }

    pub fn get_subscription_mut(&mut self, subscription_arn: &str) -> Option<&mut Subscription> {
        self.subscriptions.get_mut(subscription_arn)
    }

    pub fn list_subscriptions(&self) -> Vec<&Subscription> {
        self.subscriptions.values().collect()
    }

    pub fn list_subscriptions_by_topic(&self, topic_arn: &str) -> Vec<&Subscription> {
        self.subscriptions
            .values()
            .filter(|s| s.topic_arn == topic_arn)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Helper: compute SNS message MD5
// ---------------------------------------------------------------------------

pub fn md5_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(data);
    hex::encode(&digest[..16])
}
