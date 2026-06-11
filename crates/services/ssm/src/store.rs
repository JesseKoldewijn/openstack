use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Parameter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParameterType {
    String,
    StringList,
    SecureString,
}

impl ParameterType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParameterType::String => "String",
            ParameterType::StringList => "StringList",
            ParameterType::SecureString => "SecureString",
        }
    }
}

impl std::str::FromStr for ParameterType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "StringList" => Ok(ParameterType::StringList),
            "SecureString" => Ok(ParameterType::SecureString),
            _ => Ok(ParameterType::String),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub type_: ParameterType,
    pub value: String,
    pub description: String,
    pub version: i64,
    pub last_modified: DateTime<Utc>,
    pub arn: String,
    pub overwrite: bool,
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    pub document_type: String, // "Command" | "Session" | "Policy" | "Automation"
    pub document_format: String, // "JSON" | "YAML"
    pub schema_version: String,
    pub status: String, // "Active" | "Deleting"
    pub content: String,
    pub owner: String,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Command / Invocation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInvocation {
    pub command_id: String,
    pub instance_id: String,
    pub document_name: String,
    pub status: String, // "Success" | "Failed" | "InProgress"
    pub status_details: String,
    pub output: String,
    pub response_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub command_id: String,
    pub document_name: String,
    pub status: String, // "Success" | "InProgress" | "Failed"
    pub requested_date: DateTime<Utc>,
    pub instance_ids: Vec<String>,
    pub invocations: Vec<CommandInvocation>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SsmStore {
    /// parameter name → Parameter
    pub parameters: HashMap<String, Parameter>,
    /// parameter name → version history (sorted by version)
    pub parameter_history: HashMap<String, Vec<Parameter>>,
    /// document name → Document
    pub documents: HashMap<String, Document>,
    /// command_id → Command
    pub commands: HashMap<String, Command>,
}

impl SsmStore {
    pub fn new() -> Self {
        Self::default()
    }
}
