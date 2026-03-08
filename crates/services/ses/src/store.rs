use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Identity (email address or domain)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub identity: String,
    pub verified: bool,
}

// ---------------------------------------------------------------------------
// Stored email (for test verification)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEmail {
    pub message_id: String,
    pub source: String,
    pub destination_to: Vec<String>,
    pub destination_cc: Vec<String>,
    pub destination_bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: String,
    pub sent_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SesStore {
    /// identity -> Identity
    pub identities: HashMap<String, Identity>,
    /// message_id -> StoredEmail
    pub emails: HashMap<String, StoredEmail>,
}
