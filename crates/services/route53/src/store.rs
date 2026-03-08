use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Hosted Zone
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedZone {
    pub id: String,   // /hostedzone/<id>
    pub name: String, // DNS name with trailing dot
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
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Route53Store {
    /// zone_id -> HostedZone
    pub zones: HashMap<String, HostedZone>,
    /// (zone_id, name, type) -> ResourceRecordSet
    pub records: HashMap<(String, String, String), ResourceRecordSet>,
}
