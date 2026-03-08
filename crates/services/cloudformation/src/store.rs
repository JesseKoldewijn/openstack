use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Stack status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StackStatus {
    CreateInProgress,
    CreateComplete,
    CreateFailed,
    UpdateInProgress,
    UpdateComplete,
    UpdateFailed,
    DeleteInProgress,
    DeleteComplete,
    DeleteFailed,
    RollbackInProgress,
    RollbackComplete,
}

impl StackStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StackStatus::CreateInProgress => "CREATE_IN_PROGRESS",
            StackStatus::CreateComplete => "CREATE_COMPLETE",
            StackStatus::CreateFailed => "CREATE_FAILED",
            StackStatus::UpdateInProgress => "UPDATE_IN_PROGRESS",
            StackStatus::UpdateComplete => "UPDATE_COMPLETE",
            StackStatus::UpdateFailed => "UPDATE_FAILED",
            StackStatus::DeleteInProgress => "DELETE_IN_PROGRESS",
            StackStatus::DeleteComplete => "DELETE_COMPLETE",
            StackStatus::DeleteFailed => "DELETE_FAILED",
            StackStatus::RollbackInProgress => "ROLLBACK_IN_PROGRESS",
            StackStatus::RollbackComplete => "ROLLBACK_COMPLETE",
        }
    }
}

// ---------------------------------------------------------------------------
// Stack resource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackResource {
    pub logical_id: String,
    pub physical_id: String,
    pub resource_type: String,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Stack
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfnStack {
    pub stack_id: String,
    pub stack_name: String,
    pub description: String,
    pub status: StackStatus,
    pub status_reason: String,
    /// Parsed template body
    pub template: Value,
    /// Input parameters  (key → value)
    pub parameters: HashMap<String, String>,
    /// Stack outputs (key → value)
    pub outputs: HashMap<String, String>,
    /// Logical → physical resource ID
    pub resources: HashMap<String, StackResource>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CloudFormationStore {
    /// stack_name → CfnStack
    pub stacks: HashMap<String, CfnStack>,
}
