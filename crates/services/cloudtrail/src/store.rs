use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Trail
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trail {
    pub name: String,
    pub trail_arn: String,
    pub s3_bucket_name: String,
    pub s3_key_prefix: Option<String>,
    pub sns_topic_name: Option<String>,
    pub include_global_service_events: bool,
    pub is_multi_region_trail: bool,
    pub log_file_validation_enabled: bool,
    pub home_region: String,
    pub logging_enabled: bool,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Event (for LookupEvents)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudTrailEvent {
    pub event_id: String,
    pub event_name: String,
    pub event_time: DateTime<Utc>,
    pub event_source: String,
    pub username: Option<String>,
    pub resources: Vec<EventResource>,
    pub cloud_trail_event: String, // raw JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventResource {
    pub resource_type: String,
    pub resource_name: String,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CloudTrailStore {
    /// trail_name -> Trail
    pub trails: HashMap<String, Trail>,
    /// event_id -> CloudTrailEvent (last N events)
    pub events: Vec<CloudTrailEvent>,
}
