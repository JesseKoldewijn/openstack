use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CertificateStatus {
    Issued,
    PendingValidation,
    Expired,
    Revoked,
}

impl CertificateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CertificateStatus::Issued => "ISSUED",
            CertificateStatus::PendingValidation => "PENDING_VALIDATION",
            CertificateStatus::Expired => "EXPIRED",
            CertificateStatus::Revoked => "REVOKED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub arn: String,
    pub domain_name: String,
    pub subject_alternative_names: Vec<String>,
    pub status: CertificateStatus,
    pub created: DateTime<Utc>,
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AcmStore {
    /// arn → Certificate
    pub certificates: HashMap<String, Certificate>,
}

impl AcmStore {
    pub fn new() -> Self {
        Self::default()
    }
}
