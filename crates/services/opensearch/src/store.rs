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
    pub service_software_options: ServiceSoftwareOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub instance_type: String,
    pub instance_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSoftwareOptions {
    pub current_version: String,
    pub new_version: String,
    pub update_available: bool,
    pub cancellable: bool,
    pub update_status: String, // "COMPLETED" | "PENDING_UPDATE" | "IN_PROGRESS" | "FAILED" | "NOT_ELIGIBLE"
    pub description: String,
    pub automated_update_date: Option<String>,
}

impl Default for ServiceSoftwareOptions {
    fn default() -> Self {
        Self {
            current_version: "OpenSearch_2.5".to_string(),
            new_version: String::new(),
            update_available: false,
            cancellable: false,
            update_status: "COMPLETED".to_string(),
            description: "There is no software update available for this domain.".to_string(),
            automated_update_date: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OpenSearchStore {
    /// domain_name -> Domain
    pub domains: HashMap<String, Domain>,
    /// domain_arn -> tags
    pub tags: HashMap<String, HashMap<String, String>>,
}
