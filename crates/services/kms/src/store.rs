use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyState {
    Enabled,
    Disabled,
    PendingDeletion,
}

impl std::fmt::Display for KeyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyState::Enabled => write!(f, "Enabled"),
            KeyState::Disabled => write!(f, "Disabled"),
            KeyState::PendingDeletion => write!(f, "PendingDeletion"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KmsKey {
    pub key_id: String,
    pub arn: String,
    pub description: String,
    pub key_state: KeyState,
    pub created: DateTime<Utc>,
    /// The key material stored as hex-encoded bytes (32 bytes = 64 hex chars).
    pub key_material: String,
    pub aliases: Vec<String>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct KmsStore {
    /// key_id → KmsKey
    pub keys: HashMap<String, KmsKey>,
    /// alias_name → key_id  (e.g. "alias/my-key" → uuid)
    pub alias_to_key: HashMap<String, String>,
}

impl KmsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a key identifier (key ID, ARN, or alias) to a KmsKey reference.
    pub fn resolve_key(&self, id: &str) -> Option<&KmsKey> {
        // Direct key ID
        if let Some(k) = self.keys.get(id) {
            return Some(k);
        }
        // ARN: arn:aws:kms:region:account:key/key-id
        if id.starts_with("arn:") {
            let key_id = id.split('/').next_back()?;
            return self.keys.get(key_id);
        }
        // Alias name
        if id.starts_with("alias/") {
            let key_id = self.alias_to_key.get(id)?;
            return self.keys.get(key_id);
        }
        None
    }

    pub fn resolve_key_mut(&mut self, id: &str) -> Option<&mut KmsKey> {
        // Direct key ID
        if self.keys.contains_key(id) {
            return self.keys.get_mut(id);
        }
        // ARN
        if id.starts_with("arn:") {
            let key_id = id.split('/').next_back()?.to_string();
            return self.keys.get_mut(&key_id);
        }
        // Alias
        if id.starts_with("alias/") {
            let key_id = self.alias_to_key.get(id)?.clone();
            return self.keys.get_mut(&key_id);
        }
        None
    }
}
