use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Function state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FunctionState {
    Pending,
    Active,
    Inactive,
    Failed,
}

impl FunctionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FunctionState::Pending => "Pending",
            FunctionState::Active => "Active",
            FunctionState::Inactive => "Inactive",
            FunctionState::Failed => "Failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Function
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaFunction {
    pub function_name: String,
    pub function_arn: String,
    pub runtime: String,
    pub handler: String,
    /// Base64-encoded zip file
    pub code_zip: String,
    pub environment: HashMap<String, String>,
    /// Seconds
    pub timeout: i64,
    /// MB
    pub memory_size: i64,
    pub role: String,
    pub description: String,
    pub state: FunctionState,
    pub version: String,
    pub code_sha256: String,
    /// Layer ARNs
    pub layers: Vec<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Alias
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaAlias {
    pub name: String,
    pub function_name: String,
    pub function_version: String,
    pub description: String,
    pub arn: String,
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaVersion {
    pub version: String,
    pub function_name: String,
    pub code_sha256: String,
    pub description: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Event source mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSourceMapping {
    pub uuid: String,
    pub function_arn: String,
    pub event_source_arn: String,
    /// "Enabled" | "Disabled" | "Creating" | "Deleting"
    pub state: String,
    pub batch_size: i64,
    pub starting_position: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LambdaLayerVersion {
    pub layer_name: String,
    pub version: i64,
    pub layer_arn: String,
    pub layer_version_arn: String,
    pub description: String,
    /// Base64-encoded zip
    pub code_zip: String,
    pub compatible_runtimes: Vec<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LambdaStore {
    /// function_name → LambdaFunction
    pub functions: HashMap<String, LambdaFunction>,
    /// function_name → Vec<LambdaAlias>
    pub aliases: HashMap<String, Vec<LambdaAlias>>,
    /// function_name → Vec<LambdaVersion>
    pub versions: HashMap<String, Vec<LambdaVersion>>,
    /// uuid → EventSourceMapping
    pub event_source_mappings: HashMap<String, EventSourceMapping>,
    /// layer_name → Vec<LambdaLayerVersion>
    pub layers: HashMap<String, Vec<LambdaLayerVersion>>,
}
