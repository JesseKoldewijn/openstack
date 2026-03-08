use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Rest API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestApi {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Resource (path segment)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResource {
    pub id: String,
    pub api_id: String,
    pub parent_id: Option<String>,
    pub path_part: String,
    pub path: String,
    /// method -> Method
    pub methods: HashMap<String, ApiMethod>,
}

// ---------------------------------------------------------------------------
// Method
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMethod {
    pub http_method: String,
    pub authorization_type: String,
    pub integration: Option<ApiIntegration>,
}

// ---------------------------------------------------------------------------
// Integration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiIntegration {
    pub integration_type: String, // "AWS_PROXY" | "AWS" | "HTTP" | "MOCK"
    pub uri: String,
    pub http_method: String,
}

// ---------------------------------------------------------------------------
// Deployment / Stage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDeployment {
    pub id: String,
    pub api_id: String,
    pub description: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStage {
    pub api_id: String,
    pub stage_name: String,
    pub deployment_id: String,
    pub description: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ApiGatewayStore {
    /// api_id -> RestApi
    pub apis: HashMap<String, RestApi>,
    /// resource_id -> ApiResource
    pub resources: HashMap<String, ApiResource>,
    /// deployment_id -> ApiDeployment
    pub deployments: HashMap<String, ApiDeployment>,
    /// (api_id, stage_name) -> ApiStage
    pub stages: HashMap<(String, String), ApiStage>,
}
