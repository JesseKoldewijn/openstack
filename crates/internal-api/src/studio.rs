use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use openstack_aws_protocol::AwsProtocol;
use openstack_service_framework::ServiceState;
use serde::Deserialize;
use serde_json::json;

use crate::ApiState;

const GUIDED_MANIFEST_SCHEMA_VERSION: &str = "1.2";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuidedManifestFile {
    schema_version: String,
    service: String,
    protocol: String,
    flows: Vec<serde_json::Value>,
    #[serde(default)]
    inputs: Vec<serde_json::Value>,
}

pub(crate) fn load_service_matrix_services() -> HashSet<String> {
    let matrix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("harness")
        .join("service-matrix.json");

    let Ok(raw) = std::fs::read_to_string(matrix_path) else {
        return HashSet::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return HashSet::new();
    };

    value
        .get("services")
        .and_then(|services| services.as_array())
        .map(|services| {
            services
                .iter()
                .filter_map(|entry| entry.get("name").and_then(|name| name.as_str()))
                .map(ToOwned::to_owned)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

fn manifests_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("manifests")
        .join("guided")
}

fn protocol_name_for_service(service: &str) -> &'static str {
    match AwsProtocol::from_service(service) {
        AwsProtocol::Query | AwsProtocol::Ec2 => "query",
        AwsProtocol::Json => "json_target",
        AwsProtocol::RestXml => "rest_xml",
        AwsProtocol::RestJson => "rest_json",
    }
}

fn default_inputs(service: &str) -> Vec<serde_json::Value> {
    vec![json!({
        "name": "resource_name",
        "type": "string",
        "required": true,
        "description": format!("Resource name for {} guided flow", service)
    })]
}

pub(crate) fn load_manifest_inventory() -> HashMap<String, GuidedManifestFile> {
    let mut manifests = HashMap::new();
    let Ok(entries) = std::fs::read_dir(manifests_root()) else {
        return manifests;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".guided.json") {
            continue;
        }

        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<GuidedManifestFile>(&source) else {
            continue;
        };
        if parsed.schema_version != GUIDED_MANIFEST_SCHEMA_VERSION {
            continue;
        }

        manifests.insert(parsed.service.clone(), parsed);
    }

    manifests
}

pub(crate) fn support_tier(
    service: &str,
    manifests: &HashMap<String, GuidedManifestFile>,
) -> &'static str {
    if manifests.contains_key(service) {
        "guided"
    } else {
        "raw"
    }
}

fn l1_flow_count(manifest: &GuidedManifestFile) -> usize {
    manifest
        .flows
        .iter()
        .filter(|flow| {
            flow.get("level")
                .and_then(|level| level.as_str())
                .is_some_and(|level| level.eq_ignore_ascii_case("l1"))
        })
        .count()
}

pub async fn get_studio_services(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let manifests = &state.guided_manifest_inventory;

    let services = service_states
        .into_iter()
        .map(|(name, svc_state)| {
            let status = match svc_state {
                ServiceState::Running => "running",
                ServiceState::Starting => "starting",
                ServiceState::Stopping => "stopping",
                ServiceState::Stopped => "stopped",
                ServiceState::Available => "available",
                ServiceState::Error(_) => "error",
            };
            json!({
                "name": name,
                "status": status,
                "support_tier": support_tier(&name, manifests),
            })
        })
        .collect::<Vec<_>>();

    Json(json!({ "services": services }))
}

pub async fn get_studio_interaction_schema() -> impl IntoResponse {
    Json(json!({
        "request": {
            "fields": [
                {"name": "method", "type": "string", "required": true},
                {"name": "path", "type": "string", "required": true},
                {"name": "query", "type": "object", "required": false},
                {"name": "headers", "type": "object", "required": false},
                {"name": "body", "type": "string", "required": false}
            ]
        },
        "response": {
            "fields": [
                {"name": "status", "type": "number"},
                {"name": "headers", "type": "object"},
                {"name": "body", "type": "string"}
            ]
        }
    }))
}

pub async fn get_studio_flow_catalog(State(state): State<ApiState>) -> impl IntoResponse {
    let manifests = &state.guided_manifest_inventory;
    let services = state
        .plugin_manager
        .service_states()
        .await
        .into_iter()
        .map(|(name, _)| {
            let manifest = manifests.get(&name);
            let l1_flows = manifest.map(l1_flow_count).unwrap_or(0);
            json!({
                "service": name,
                "manifest_version": GUIDED_MANIFEST_SCHEMA_VERSION,
                "protocol": manifest
                    .map(|item| item.protocol.as_str())
                    .unwrap_or_else(|| protocol_name_for_service(&name)),
                "flow_count": manifest.map(|item| item.flows.len()).unwrap_or(0),
                "maturity": if l1_flows > 0 { "l1" } else { "none" },
            })
        })
        .collect::<Vec<_>>();

    Json(json!({ "services": services }))
}

pub async fn get_studio_flow_definition(
    State(state): State<ApiState>,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let manifest = state.guided_manifest_inventory.get(&service);

    (
        axum::http::StatusCode::OK,
        Json(json!({
            "service": service,
            "schema_version": GUIDED_MANIFEST_SCHEMA_VERSION,
            "protocol": manifest
                .map(|item| item.protocol.as_str())
                .unwrap_or_else(|| protocol_name_for_service(&service)),
            "flows": manifest
                .map(|item| item.flows.clone())
                .unwrap_or_default(),
            "inputs": manifest
                .map(|item| {
                    if item.inputs.is_empty() {
                        default_inputs(&service)
                    } else {
                        item.inputs.clone()
                    }
                })
                .unwrap_or_else(|| default_inputs(&service)),
        })),
    )
        .into_response()
}

pub async fn get_studio_flow_coverage(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let manifests = &state.guided_manifest_inventory;
    let matrix_services = &state.guided_service_matrix;
    let mut seen = HashSet::new();

    let mut services = service_states
        .into_iter()
        .map(|(name, _)| {
            seen.insert(name.clone());
            let manifest = manifests.get(&name);
            let total_flows = manifest.map(|item| item.flows.len()).unwrap_or(0);
            let l1_flows = manifest.map(l1_flow_count).unwrap_or(0);
            json!({
                "service": name,
                "has_manifest": manifest.is_some(),
                "l1_flows": l1_flows,
                "total_flows": total_flows,
                "quality": if l1_flows > 0 { "meets_l1" } else { "missing" }
            })
        })
        .collect::<Vec<_>>();

    for (service, manifest) in manifests {
        if seen.contains(service) {
            continue;
        }
        if !matrix_services.is_empty() && !matrix_services.contains(service) {
            continue;
        }
        let l1_flows = l1_flow_count(manifest);
        services.push(json!({
            "service": service,
            "has_manifest": true,
            "l1_flows": l1_flows,
            "total_flows": manifest.flows.len(),
            "quality": if l1_flows > 0 { "meets_l1" } else { "missing" }
        }));
    }

    services.sort_by(|a, b| {
        a["service"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["service"].as_str().unwrap_or_default())
    });

    let guided_services = services
        .iter()
        .filter(|item| item["has_manifest"].as_bool().unwrap_or(false))
        .count();
    let supported_services = services.len();

    Json(json!({
        "schema_version": GUIDED_MANIFEST_SCHEMA_VERSION,
        "summary": "guided coverage by service",
        "counts": {
            "guided_services": guided_services,
            "supported_services": supported_services,
        },
        "services": services,
    }))
}
