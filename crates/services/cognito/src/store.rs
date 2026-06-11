use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// User Pool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPool {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub status: String, // "Active"
    pub creation_date: DateTime<Utc>,
    pub last_modified_date: DateTime<Utc>,
    pub mfa_configuration: String, // "OFF" | "ON" | "OPTIONAL"
    pub email_verification_subject: String,
    pub email_verification_message: String,
    pub username_attributes: Vec<String>,
    pub auto_verified_attributes: Vec<String>,
}

// ---------------------------------------------------------------------------
// User Pool Client (App Client)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPoolClient {
    pub client_id: String,
    pub client_name: String,
    pub user_pool_id: String,
    pub client_secret: Option<String>,
    pub explicit_auth_flows: Vec<String>,
    pub allowed_o_auth_flows: Vec<String>,
    pub allowed_o_auth_scopes: Vec<String>,
    #[serde(rename = "CallbackURLs")]
    pub callback_urls: Vec<String>,
    #[serde(rename = "LogoutURLs")]
    pub logout_urls: Vec<String>,
    pub creation_date: DateTime<Utc>,
    pub last_modified_date: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserStatus {
    Confirmed,
    Unconfirmed,
    ForceChangePassword,
    Disabled,
}

impl UserStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserStatus::Confirmed => "CONFIRMED",
            UserStatus::Unconfirmed => "UNCONFIRMED",
            UserStatus::ForceChangePassword => "FORCE_CHANGE_PASSWORD",
            UserStatus::Disabled => "DISABLED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAttribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub user_pool_id: String,
    pub attributes: Vec<UserAttribute>,
    pub user_status: UserStatus,
    pub enabled: bool,
    pub user_create_date: DateTime<Utc>,
    pub user_last_modified_date: DateTime<Utc>,
    /// Hashed password for AdminInitiateAuth (stored plaintext in mock — no real security)
    pub password: Option<String>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CognitoStore {
    /// pool_id -> UserPool
    pub user_pools: HashMap<String, UserPool>,
    /// client_id -> UserPoolClient
    pub clients: HashMap<String, UserPoolClient>,
    /// (pool_id, username) -> User
    pub users: HashMap<(String, String), User>,
}
