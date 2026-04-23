use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Hosted Zone
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedZone {
    pub id: String,   // /hostedzone/<id>
    pub name: String, // DNS name with trailing dot
    pub caller_reference: String,
    pub comment: String,
    pub private_zone: bool,
    pub record_count: usize,
}

// ---------------------------------------------------------------------------
// Resource Record Set
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRecordSet {
    pub name: String,
    pub record_type: String,
    pub ttl: u64,
    pub values: Vec<String>,
}

// ---------------------------------------------------------------------------
// Health Check
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub ip_address: Option<String>,
    pub port: u16,
    pub health_check_type: String, // "HTTP" | "HTTPS" | "TCP"
    pub resource_path: Option<String>,
    pub fully_qualified_domain_name: Option<String>,
    pub request_interval: u32,
    pub failure_threshold: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub id: String,
    pub caller_reference: String,
    pub config: HealthCheckConfig,
    pub health_check_version: u64,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Route53Store {
    /// zone_id -> HostedZone
    pub zones: HashMap<String, HostedZone>,
    /// (zone_id, name, type) -> ResourceRecordSet
    pub records: HashMap<(String, String, String), ResourceRecordSet>,
    /// health_check_id -> HealthCheck
    pub health_checks: HashMap<String, HealthCheck>,
    /// (resource_type, resource_id) -> tags
    pub tags: HashMap<(String, String), HashMap<String, String>>,
}
