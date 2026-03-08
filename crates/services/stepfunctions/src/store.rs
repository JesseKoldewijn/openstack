use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// State Machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub state_machine_arn: String,
    pub name: String,
    pub definition: Value, // parsed ASL JSON
    pub role_arn: String,
    pub status: String,       // "ACTIVE" | "DELETING"
    pub machine_type: String, // "STANDARD" | "EXPRESS"
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Aborted,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionStatus::Running => "RUNNING",
            ExecutionStatus::Succeeded => "SUCCEEDED",
            ExecutionStatus::Failed => "FAILED",
            ExecutionStatus::TimedOut => "TIMED_OUT",
            ExecutionStatus::Aborted => "ABORTED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub execution_arn: String,
    pub state_machine_arn: String,
    pub name: String,
    pub status: ExecutionStatus,
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub cause: Option<String>,
    pub started_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StepFunctionsStore {
    /// state_machine_arn -> StateMachine
    pub state_machines: HashMap<String, StateMachine>,
    /// execution_arn -> Execution
    pub executions: HashMap<String, Execution>,
}
