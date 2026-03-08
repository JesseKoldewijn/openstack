use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    pub domain_name: String,
    pub arn: String,
    pub engine_version: String,
    pub cluster_config: ClusterConfig,
    pub endpoint: Option<String>,
    pub status: String, // "ACTIVE" | "DELETING"
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub instance_type: String,
    pub instance_count: u32,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OpenSearchStore {
    /// domain_name -> Domain
    pub domains: HashMap<String, Domain>,
}
