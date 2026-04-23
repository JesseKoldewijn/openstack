use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Cluster
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub cluster_name: String,
    pub cluster_arn: String,
    pub status: String, // "ACTIVE" | "INACTIVE"
    pub registered_container_instances_count: u32,
    pub running_tasks_count: u32,
    pub pending_tasks_count: u32,
    pub active_services_count: u32,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Task Definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDefinition {
    pub name: String,
    pub image: String,
    pub cpu: u32,
    pub memory: Option<u32>,
    pub essential: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    pub family: String,
    pub revision: u32,
    pub task_definition_arn: String,
    pub status: String, // "ACTIVE" | "INACTIVE"
    pub container_definitions: Vec<ContainerDefinition>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub network_mode: String,
    pub registered_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub service_name: String,
    pub service_arn: String,
    pub cluster_arn: String,
    pub task_definition: String,
    pub desired_count: u32,
    pub running_count: u32,
    pub pending_count: u32,
    pub status: String, // "ACTIVE" | "DRAINING" | "INACTIVE"
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_arn: String,
    pub cluster_arn: String,
    pub task_definition_arn: String,
    pub last_status: String, // "RUNNING" | "STOPPED" | "PENDING"
    pub desired_status: String,
    pub group: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub stop_code: Option<String>,
    pub stopped_reason: Option<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EcsStore {
    /// cluster_arn -> Cluster
    pub clusters: HashMap<String, Cluster>,
    /// task_definition_arn -> TaskDefinition (family:revision as key)
    pub task_definitions: HashMap<String, TaskDefinition>,
    /// latest_revision[family] -> revision number
    pub task_def_revisions: HashMap<String, u32>,
    /// service_arn -> Service
    pub services: HashMap<String, Service>,
    /// task_arn -> Task
    pub tasks: HashMap<String, Task>,
}
