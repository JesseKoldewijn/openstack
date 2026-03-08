use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Event Bus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBus {
    pub name: String,
    pub arn: String,
}

// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRule {
    pub name: String,
    pub event_bus_name: String,
    pub event_pattern: Option<Value>,
    pub schedule_expression: Option<String>,
    pub state: String, // "ENABLED" | "DISABLED"
    pub description: String,
    pub targets: HashMap<String, RuleTarget>,
    pub arn: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Target
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTarget {
    pub id: String,
    pub arn: String,
    pub input: Option<String>,
    pub input_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EventBridgeStore {
    /// bus_name -> EventBus
    pub buses: HashMap<String, EventBus>,
    /// rule_name -> EventRule
    pub rules: HashMap<String, EventRule>,
}
