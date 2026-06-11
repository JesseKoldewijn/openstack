use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DB Instance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbEndpoint {
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbInstance {
    pub db_instance_identifier: String,
    pub db_instance_class: String,
    pub engine: String,
    pub engine_version: String,
    pub db_instance_status: String, // "available" | "creating" | "deleting" | "rebooting"
    pub master_username: String,
    pub db_name: Option<String>,
    pub endpoint: Option<DbEndpoint>,
    pub allocated_storage: u32,
    pub multi_az: bool,
    pub db_subnet_group_name: Option<String>,
    pub db_parameter_group_name: Option<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// DB Snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSnapshot {
    pub db_snapshot_identifier: String,
    pub db_instance_identifier: String,
    pub snapshot_type: String, // "manual" | "automated"
    pub status: String,        // "available" | "creating"
    pub engine: String,
    pub engine_version: String,
    pub allocated_storage: u32,
    pub master_username: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// DB Subnet Group
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSubnetGroup {
    pub db_subnet_group_name: String,
    pub db_subnet_group_description: String,
    pub vpc_id: String,
    pub subnet_ids: Vec<String>,
    pub status: String,
}

// ---------------------------------------------------------------------------
// DB Parameter Group
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbParameterGroup {
    pub db_parameter_group_name: String,
    pub db_parameter_group_family: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RdsStore {
    /// db_instance_identifier -> DbInstance
    pub instances: HashMap<String, DbInstance>,
    /// db_snapshot_identifier -> DbSnapshot
    pub snapshots: HashMap<String, DbSnapshot>,
    /// db_subnet_group_name -> DbSubnetGroup
    pub subnet_groups: HashMap<String, DbSubnetGroup>,
    /// db_parameter_group_name -> DbParameterGroup
    pub parameter_groups: HashMap<String, DbParameterGroup>,
}
