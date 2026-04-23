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
// Email template (SES v1 templates)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailTemplate {
    pub template_name: String,
    pub subject_part: String,
    pub html_part: String,
    pub text_part: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Identity notification attributes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityNotificationAttributes {
    pub bounce_topic: Option<String>,
    pub complaint_topic: Option<String>,
    pub delivery_topic: Option<String>,
    pub forwarding_enabled: bool,
}

impl Default for IdentityNotificationAttributes {
    fn default() -> Self {
        Self {
            bounce_topic: None,
            complaint_topic: None,
            delivery_topic: None,
            forwarding_enabled: true,
        }
    }
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
    /// template_name -> EmailTemplate
    pub templates: HashMap<String, EmailTemplate>,
    /// identity -> IdentityNotificationAttributes
    pub notification_attrs: HashMap<String, IdentityNotificationAttributes>,
}
