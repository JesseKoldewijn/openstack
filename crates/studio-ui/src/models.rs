use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub name: String,
    pub status: String,
    pub support_tier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioServicesResponse {
    pub services: Vec<ServiceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiSection {
    pub fields: Vec<ApiField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionSchema {
    pub request: ApiSection,
    pub response: ApiSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowCatalogEntry {
    pub service: String,
    pub manifest_version: String,
    pub protocol: String,
    pub flow_count: usize,
    pub maturity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowCatalogResponse {
    pub services: Vec<FlowCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedInputField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowDefinitionResponse {
    pub service: String,
    pub schema_version: String,
    pub protocol: String,
    pub flows: Vec<serde_json::Value>,
    #[serde(default)]
    pub inputs: Vec<GuidedInputField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowCoverageEntry {
    pub service: String,
    pub has_manifest: bool,
    pub l1_flows: usize,
    pub total_flows: usize,
    pub quality: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowCoverageResponse {
    pub schema_version: String,
    pub summary: String,
    pub services: Vec<FlowCoverageEntry>,
}
