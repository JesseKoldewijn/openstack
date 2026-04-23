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
    pub logging_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterEndpoint {
    pub address: String,
    pub port: u16,
}

// ---------------------------------------------------------------------------
// Cluster Snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub snapshot_identifier: String,
    pub cluster_identifier: String,
    pub status: String, // "available" | "creating"
    pub created: DateTime<Utc>,
    pub node_type: String,
    pub db_name: String,
    pub master_username: String,
}

// ---------------------------------------------------------------------------
// Cluster Subnet Group
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSubnetGroup {
    pub cluster_subnet_group_name: String,
    pub description: String,
    pub vpc_id: String,
    pub subnet_ids: Vec<String>,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Cluster Parameter Group
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterParameterGroup {
    pub parameter_group_name: String,
    pub parameter_group_family: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RedshiftStore {
    /// cluster_identifier -> Cluster
    pub clusters: HashMap<String, Cluster>,
    /// snapshot_identifier -> ClusterSnapshot
    pub snapshots: HashMap<String, ClusterSnapshot>,
    /// subnet_group_name -> ClusterSubnetGroup
    pub subnet_groups: HashMap<String, ClusterSubnetGroup>,
    /// parameter_group_name -> ClusterParameterGroup
    pub parameter_groups: HashMap<String, ClusterParameterGroup>,
}
