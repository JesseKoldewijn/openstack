use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Destination
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DestinationType {
    S3 { bucket_arn: String },
    ExtendedS3 { bucket_arn: String },
    Other,
}

// ---------------------------------------------------------------------------
// Stream status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryStreamStatus {
    Creating,
    Active,
    Deleting,
}

impl DeliveryStreamStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryStreamStatus::Creating => "CREATING",
            DeliveryStreamStatus::Active => "ACTIVE",
            DeliveryStreamStatus::Deleting => "DELETING",
        }
    }
}

// ---------------------------------------------------------------------------
// Record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirehoseRecord {
    pub record_id: String,
    /// Data stored as base64
    pub data: String,
    pub arrival: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Delivery stream
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirehoseDeliveryStream {
    pub name: String,
    pub arn: String,
    pub status: DeliveryStreamStatus,
    pub destination: DestinationType,
    pub records: Vec<FirehoseRecord>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FirehoseStore {
    /// stream_name → FirehoseDeliveryStream
    pub streams: HashMap<String, FirehoseDeliveryStream>,
}

impl FirehoseStore {
    pub fn new() -> Self {
        Self::default()
    }
}
