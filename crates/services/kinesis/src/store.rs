use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shard iterator types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShardIteratorType {
    TrimHorizon,
    Latest,
    AtSequenceNumber,
    AfterSequenceNumber,
    AtTimestamp,
}

impl ShardIteratorType {
    pub fn parse(s: &str) -> Self {
        match s {
            "LATEST" => Self::Latest,
            "AT_SEQUENCE_NUMBER" => Self::AtSequenceNumber,
            "AFTER_SEQUENCE_NUMBER" => Self::AfterSequenceNumber,
            "AT_TIMESTAMP" => Self::AtTimestamp,
            _ => Self::TrimHorizon,
        }
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KinesisRecord {
    pub sequence_number: String,
    pub partition_key: String,
    /// Data stored as base64
    pub data: String,
    pub approximate_arrival_timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Shard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    pub shard_id: String,
    pub records: VecDeque<KinesisRecord>,
    pub sequence_counter: u64,
    pub is_open: bool,
    pub parent_shard_id: Option<String>,
    pub adjacent_parent_shard_id: Option<String>,
}

impl Shard {
    pub fn new(shard_id: String) -> Self {
        Self {
            shard_id,
            records: VecDeque::new(),
            sequence_counter: 0,
            is_open: true,
            parent_shard_id: None,
            adjacent_parent_shard_id: None,
        }
    }

    pub fn next_sequence_number(&mut self) -> String {
        self.sequence_counter += 1;
        format!("{:049}", self.sequence_counter)
    }
}

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamStatus {
    Creating,
    Active,
    Updating,
    Deleting,
}

impl StreamStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamStatus::Creating => "CREATING",
            StreamStatus::Active => "ACTIVE",
            StreamStatus::Updating => "UPDATING",
            StreamStatus::Deleting => "DELETING",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KinesisStream {
    pub stream_name: String,
    pub stream_arn: String,
    pub status: StreamStatus,
    pub shard_count: usize,
    pub retention_period_hours: i64,
    pub created: DateTime<Utc>,
    pub shards: Vec<Shard>,
    pub shard_id_counter: u64,
}

impl KinesisStream {
    pub fn new(stream_name: String, stream_arn: String, shard_count: usize) -> Self {
        let mut shards = Vec::new();
        for i in 0..shard_count {
            shards.push(Shard::new(format!("shardId-{i:012}")));
        }
        Self {
            stream_name,
            stream_arn,
            status: StreamStatus::Active,
            shard_count,
            retention_period_hours: 24,
            created: Utc::now(),
            shards,
            shard_id_counter: shard_count as u64,
        }
    }
}

// ---------------------------------------------------------------------------
// Shard Iterator state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardIteratorState {
    pub stream_name: String,
    pub shard_id: String,
    /// The next record index within the shard's record deque to return
    pub next_index: usize,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct KinesisStore {
    /// stream_name → KinesisStream
    pub streams: std::collections::HashMap<String, KinesisStream>,
    /// shard_iterator_token → ShardIteratorState
    pub shard_iterators: std::collections::HashMap<String, ShardIteratorState>,
}

impl KinesisStore {
    pub fn new() -> Self {
        Self::default()
    }
}
