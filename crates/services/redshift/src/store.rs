use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Cluster
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub cluster_identifier: String,
    pub node_type: String,
    pub master_username: String,
    pub db_name: String,
    pub port: u16,
    pub cluster_status: String, // "available" | "deleting"
    pub endpoint: Option<ClusterEndpoint>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterEndpoint {
    pub address: String,
    pub port: u16,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RedshiftStore {
    /// cluster_identifier -> Cluster
    pub clusters: HashMap<String, Cluster>,
}
