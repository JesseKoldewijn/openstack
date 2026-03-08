use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CloudWatch Metrics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDatum {
    pub namespace: String,
    pub metric_name: String,
    pub dimensions: Vec<(String, String)>,
    pub timestamp: DateTime<Utc>,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAlarm {
    pub alarm_name: String,
    pub alarm_description: String,
    pub metric_name: String,
    pub namespace: String,
    pub statistic: String,
    pub period: u64,
    pub evaluation_periods: u64,
    pub threshold: f64,
    pub comparison_operator: String,
    pub state_value: String, // "OK" | "ALARM" | "INSUFFICIENT_DATA"
    pub state_reason: String,
    pub actions_enabled: bool,
    pub alarm_actions: Vec<String>,
    pub ok_actions: Vec<String>,
    pub insufficient_data_actions: Vec<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// CloudWatch Logs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogGroup {
    pub log_group_name: String,
    pub retention_in_days: Option<u32>,
    pub created_at: i64, // unix ms
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogStream {
    pub log_stream_name: String,
    pub log_group_name: String,
    pub created_at: i64,
    pub first_event_timestamp: Option<i64>,
    pub last_event_timestamp: Option<i64>,
    pub upload_sequence_token: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp: i64,
    pub message: String,
    pub ingestion_time: i64,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CloudWatchStore {
    /// namespace:metric_name -> Vec<MetricDatum>
    pub metrics: Vec<MetricDatum>,
    /// alarm_name -> MetricAlarm
    pub alarms: HashMap<String, MetricAlarm>,

    // CloudWatch Logs
    /// log_group_name -> LogGroup
    pub log_groups: HashMap<String, LogGroup>,
    /// (log_group_name, log_stream_name) -> LogStream
    pub log_streams: HashMap<(String, String), LogStream>,
    /// (log_group_name, log_stream_name) -> Vec<LogEvent>
    pub log_events: HashMap<(String, String), Vec<LogEvent>>,
}
