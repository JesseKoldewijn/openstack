use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// IAM types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamUser {
    pub user_id: String,
    pub user_name: String,
    pub arn: String,
    pub path: String,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
    pub attached_policies: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamRole {
    pub role_id: String,
    pub role_name: String,
    pub arn: String,
    pub path: String,
    pub assume_role_policy_document: String,
    pub description: String,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
    pub attached_policies: Vec<String>,
    pub inline_policies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamPolicy {
    pub policy_id: String,
    pub policy_name: String,
    pub arn: String,
    pub path: String,
    pub document: String,
    pub description: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamGroup {
    pub group_id: String,
    pub group_name: String,
    pub arn: String,
    pub path: String,
    pub created: DateTime<Utc>,
    pub members: Vec<String>,
    pub attached_policies: Vec<String>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IamStore {
    pub users: HashMap<String, IamUser>,
    pub roles: HashMap<String, IamRole>,
    pub policies: HashMap<String, IamPolicy>, // ARN → policy
    pub groups: HashMap<String, IamGroup>,
}

impl IamStore {
    pub fn new() -> Self {
        Self::default()
    }
}
