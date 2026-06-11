use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Cache Cluster (single-node, e.g. Memcached or single Redis)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheClusterEndpoint {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCluster {
    pub cache_cluster_id: String,
    pub cache_node_type: String,
    pub engine: String, // "redis" | "memcached"
    pub engine_version: String,
    pub cache_cluster_status: String, // "available" | "creating" | "deleting"
    pub num_cache_nodes: u32,
    pub cache_subnet_group_name: Option<String>,
    pub configuration_endpoint: Option<CacheClusterEndpoint>,
    pub replication_group_id: Option<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Replication Group (multi-node Redis)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationGroup {
    pub replication_group_id: String,
    pub description: String,
    pub status: String,             // "available" | "creating" | "deleting"
    pub automatic_failover: String, // "enabled" | "disabled"
    pub multi_az: String,           // "enabled" | "disabled"
    pub num_cache_clusters: u32,
    pub member_clusters: Vec<String>,
    pub node_groups: Vec<NodeGroup>,
    pub snapshot_retention_limit: u32,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeGroup {
    pub node_group_id: String,
    pub status: String,
    pub primary_endpoint: Option<CacheClusterEndpoint>,
    pub reader_endpoint: Option<CacheClusterEndpoint>,
}

// ---------------------------------------------------------------------------
// Cache Subnet Group
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSubnetGroup {
    pub cache_subnet_group_name: String,
    pub cache_subnet_group_description: String,
    pub vpc_id: String,
    pub subnet_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ElastiCacheStore {
    /// cache_cluster_id -> CacheCluster
    pub clusters: HashMap<String, CacheCluster>,
    /// replication_group_id -> ReplicationGroup
    pub replication_groups: HashMap<String, ReplicationGroup>,
    /// subnet_group_name -> CacheSubnetGroup
    pub subnet_groups: HashMap<String, CacheSubnetGroup>,
}
