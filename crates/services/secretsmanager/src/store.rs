use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretVersion {
    pub version_id: String,
    pub secret_string: Option<String>,
    pub secret_binary: Option<Vec<u8>>,
    pub created: DateTime<Utc>,
    pub version_stages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub arn: String,
    pub name: String,
    pub description: String,
    pub created: DateTime<Utc>,
    pub last_changed: DateTime<Utc>,
    pub deleted: bool,
    pub deletion_date: Option<DateTime<Utc>>,
    pub versions: Vec<SecretVersion>,
    pub tags: HashMap<String, String>,
}

impl Secret {
    /// Get the current (AWSCURRENT) version.
    pub fn current_version(&self) -> Option<&SecretVersion> {
        self.versions
            .iter()
            .find(|v| v.version_stages.contains(&"AWSCURRENT".to_string()))
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SecretsManagerStore {
    /// name → Secret
    pub secrets: HashMap<String, Secret>,
}

impl SecretsManagerStore {
    pub fn new() -> Self {
        Self::default()
    }
}
