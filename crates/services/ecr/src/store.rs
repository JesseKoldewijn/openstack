use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub registry_id: String,
    pub arn: String,
    pub uri: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub repository_name: String,
    pub image_digest: String,
    pub image_tags: Vec<String>,
    pub image_manifest: String,
    pub pushed_at: DateTime<Utc>,
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EcrStore {
    /// repository_name -> Repository
    pub repositories: HashMap<String, Repository>,
    /// image_digest -> Image
    pub images: HashMap<String, Image>,
}
