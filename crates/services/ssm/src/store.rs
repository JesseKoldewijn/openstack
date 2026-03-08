use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SsmStore {
    /// parameter name → Parameter
    pub parameters: HashMap<String, Parameter>,
}

impl SsmStore {
    pub fn new() -> Self {
        Self::default()
    }
}
