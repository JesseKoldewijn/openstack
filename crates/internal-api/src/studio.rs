use std::collections::{HashMap, HashSet};
use std::fs;
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
struct GuidedManifestFile {
    schema_version: String,
    service: String,
    protocol: String,
    flows: Vec<serde_json::Value>,
    #[serde(default)]
    inputs: Vec<serde_json::Value>,
}

/// Reads the test harness service-matrix.json and returns the set of service names it lists.
///
/// Attempts to read tests/harness/service-matrix.json relative to the crate root and parse
/// the top-level "services" array, collecting each entry's "name" string into a `HashSet`.
/// If the file cannot be read, parsed, or does not contain a services array, an empty set is returned.
///
/// # Examples
///
/// ```
/// let services = service_matrix_services();
/// // If the repository includes tests/harness/service-matrix.json with service entries,
/// // `services` will contain their names; otherwise it will be empty.
/// assert!(services.is_empty() || services.iter().all(|s| !s.is_empty()));
/// ```
fn service_matrix_services() -> HashSet<String> {
    let matrix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("harness")
        .join("service-matrix.json");

    let Ok(raw) = fs::read_to_string(matrix_path) else {
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

/// Compute the filesystem root path for guided manifests.
///
/// The returned path is the crate's manifest directory moved two levels up, then
/// appended with `manifests/guided` (i.e., "<CARGO_MANIFEST_DIR>/../../manifests/guided").
///
/// # Examples
///
/// ```
/// let _root = manifests_root();
/// ```
fn manifests_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("manifests")
        .join("guided")
}

/// Map an AWS service name to its canonical protocol identifier.
///
/// The returned string is one of: "query", "json_target", "rest_xml", or "rest_json",
/// indicating the protocol typically used by that service.
///
/// # Examples
///
/// ```
/// let p = protocol_name_for_service("s3");
/// assert!(["query", "json_target", "rest_xml", "rest_json"].contains(&p));
/// ```
fn protocol_name_for_service(service: &str) -> &'static str {
    match AwsProtocol::from_service(service) {
        AwsProtocol::Query | AwsProtocol::Ec2 => "query",
        AwsProtocol::Json => "json_target",
        AwsProtocol::RestXml => "rest_xml",
        AwsProtocol::RestJson => "rest_json",
    }
}

/// Create the default input schema for a service's guided flow.
///
/// The returned vector contains a single JSON object describing a required
/// string input named `"resource_name"`. The object's `description` field
/// includes the provided service name.
///
/// # Examples
///
/// ```
/// let inputs = default_inputs("s3");
/// let first = &inputs[0];
/// assert_eq!(first["name"], "resource_name");
/// assert_eq!(first["type"], "string");
/// assert_eq!(first["required"], true);
/// assert!(first["description"].as_str().unwrap().contains("s3"));
/// ```
fn default_inputs(service: &str) -> Vec<serde_json::Value> {
    vec![json!({
        "name": "resource_name",
        "type": "string",
        "required": true,
        "description": format!("Resource name for {} guided flow", service)
    })]
}

/// Load guided manifest files from the manifests root and return them keyed by service name.
///
/// Files are discovered by scanning the manifests root for filenames ending with `.guided.json`.
/// Only files that parse successfully as `GuidedManifestFile` and whose `schema_version` matches
/// `GUIDED_MANIFEST_SCHEMA_VERSION` are included. Files that cannot be read or parsed are skipped.
///
/// # Returns
///
/// A `HashMap` mapping each manifest's `service` name to its parsed `GuidedManifestFile`.
///
/// # Examples
///
/// ```
/// let inventory = manifest_inventory();
/// // `inventory` is a map from service name to `GuidedManifestFile`
/// assert!(inventory.is_empty() || inventory.len() >= 1);
/// ```
fn manifest_inventory() -> HashMap<String, GuidedManifestFile> {
    let mut manifests = HashMap::new();
    let Ok(entries) = fs::read_dir(manifests_root()) else {
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

        let Ok(source) = fs::read_to_string(&path) else {
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

/// Determine the support tier for a service based on presence of a guided manifest.
///
/// # Returns
///
/// `'guided'` if a guided manifest exists for the given service, `'raw'` otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// // assume `support_tier`, `GuidedManifestFile`, and `GUIDED_MANIFEST_SCHEMA_VERSION` are in scope
///
/// let manifests: HashMap<String, GuidedManifestFile> = HashMap::new();
/// assert_eq!(support_tier("my-service", &manifests), "raw");
///
/// let mut manifests = HashMap::new();
/// manifests.insert(
///     "my-service".to_string(),
///     GuidedManifestFile {
///         schema_version: GUIDED_MANIFEST_SCHEMA_VERSION.to_string(),
///         service: "my-service".to_string(),
///         protocol: None,
///         flows: Vec::new(),
///         inputs: Vec::new(),
///     },
/// );
/// assert_eq!(support_tier("my-service", &manifests), "guided");
/// ```
fn support_tier(service: &str, manifests: &HashMap<String, GuidedManifestFile>) -> &'static str {
    if manifests.contains_key(service) {
        "guided"
    } else {
        "raw"
    }
}

/// Return a JSON object listing all studio services with their runtime status and support tier.
///
/// The response body is a JSON object with a "services" array; each element contains
/// "name" (service name), "status" (one of "running", "starting", "stopping", "stopped", "available", "error"),
/// and "support_tier" (either "guided" or "raw").
///
/// # Returns
///
/// A JSON response: `{ "services": [ { "name": string, "status": string, "support_tier": string }, ... ] }`.
///
/// # Examples
///
/// ```
/// # use axum::extract::State;
/// # async fn example(state: ApiState) {
/// let response = get_studio_services(State(state)).await;
/// // `response` is a JSON response containing the "services" array described above.
/// # }
/// ```
pub async fn get_studio_services(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let manifests = manifest_inventory();

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
                "support_tier": support_tier(&name, &manifests),
            })
        })
        .collect::<Vec<_>>();

    Json(json!({ "services": services }))
}

/// Provide the fixed interaction schema describing request and response fields used by the studio.
///
/// The JSON object contains a `request` entry with fields: `method`, `path`, `query`, `headers`, and `body`,
/// and a `response` entry with fields: `status`, `headers`, and `body`.
///
/// # Examples
///
/// ```
/// // Resulting JSON structure (formatted):
/// {
///   "request": {
///     "fields": [
///       {"name":"method","type":"string","required":true},
///       {"name":"path","type":"string","required":true},
///       {"name":"query","type":"object","required":false},
///       {"name":"headers","type":"object","required":false},
///       {"name":"body","type":"string","required":false}
///     ]
///   },
///   "response": {
///     "fields":[
///       {"name":"status","type":"number"},
///       {"name":"headers","type":"object"},
///       {"name":"body","type":"string"}
///     ]
///   }
/// }
/// ```
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

/// Builds a catalog of services with guided flow metadata.
///
/// The response is a JSON object with a top-level `services` array. Each service entry contains:
/// - `service`: service name
/// - `manifest_version`: the guided manifest schema version
/// - `protocol`: protocol name (from manifest or derived from the service)
/// - `flow_count`: number of flows in the manifest (0 if none)
/// - `maturity`: `"l1"` if a manifest is present, otherwise `"none"`.
///
/// # Examples
///
/// ```
/// // In an async test or runtime:
/// // let resp = get_studio_flow_catalog().await;
/// // The response body will be JSON similar to:
/// // { "services": [ { "service": "s3", "manifest_version": "1.2", "protocol": "rest_xml", "flow_count": 3, "maturity": "l1" }, ... ] }
/// ```
pub async fn get_studio_flow_catalog(State(state): State<ApiState>) -> impl IntoResponse {
    let manifests = manifest_inventory();
    let services = state
        .plugin_manager
        .service_states()
        .await
        .into_iter()
        .map(|(name, _)| {
            let manifest = manifests.get(&name);
            json!({
                "service": name,
                "manifest_version": GUIDED_MANIFEST_SCHEMA_VERSION,
                "protocol": manifest
                    .map(|item| item.protocol.as_str())
                    .unwrap_or_else(|| protocol_name_for_service(&name)),
                "flow_count": manifest.map(|item| item.flows.len()).unwrap_or(0),
                "maturity": if manifest.is_some() { "l1" } else { "none" },
            })
        })
        .collect::<Vec<_>>();

    Json(json!({ "services": services }))
}

/// Fetches the guided flow manifest for the specified service.
///
/// The response is a JSON object containing:
/// - `service`: the requested service name,
/// - `schema_version`: the guided manifest schema version,
/// - `protocol`: the manifest protocol if present, otherwise a protocol derived from the service name,
/// - `flows`: the manifest's flows or an empty list if none,
/// - `inputs`: the manifest's inputs when present and non-empty, otherwise generated default inputs for the service.
///
/// # Examples
///
/// ```
/// // Example usage (handler can be invoked by an Axum router)
/// use axum::extract::Path;
///
/// // Call the handler with a service name
/// let _resp = tokio::runtime::Runtime::new()
///     .unwrap()
///     .block_on(crate::studio::get_studio_flow_definition(Path("s3".to_string())));
/// ```
pub async fn get_studio_flow_definition(Path(service): Path<String>) -> impl IntoResponse {
    let manifests = manifest_inventory();
    let manifest = manifests.get(&service);

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

/// Computes guided-manifest coverage across services and returns a JSON summary.
///
/// The response includes a schema version, a brief summary, aggregate counts
/// (`guided_services`, `supported_services`), and a sorted list of per-service
/// entries with `service`, `has_manifest`, `l1_flows`, `total_flows`, and `quality`.
///
/// # Returns
///
/// A JSON object containing coverage metadata and the `services` array described above.
///
/// # Examples
///
/// ```ignore
/// // The handler returns a `Json` wrapper around a structure like this:
/// let resp = serde_json::json!({
///     "schema_version": "1.2",
///     "summary": "guided coverage by service",
///     "counts": {
///         "guided_services": 5,
///         "supported_services": 10,
///     },
///     "services": [
///         {
///             "service": "s3",
///             "has_manifest": true,
///             "l1_flows": 1,
///             "total_flows": 3,
///             "quality": "meets_l1"
///         },
///         {
///             "service": "unknown",
///             "has_manifest": false,
///             "l1_flows": 0,
///             "total_flows": 0,
///             "quality": "missing"
///         }
///     ]
/// });
/// assert!(resp["services"].is_array());
/// ```
pub async fn get_studio_flow_coverage(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let manifests = manifest_inventory();
    let matrix_services = service_matrix_services();
    let mut seen = HashSet::new();

    let mut services = service_states
        .into_iter()
        .map(|(name, _)| {
            seen.insert(name.clone());
            let manifest = manifests.get(&name);
            let total_flows = manifest.map(|item| item.flows.len()).unwrap_or(0);
            json!({
                "service": name,
                "has_manifest": manifest.is_some(),
                "l1_flows": if total_flows > 0 { 1 } else { 0 },
                "total_flows": total_flows,
                "quality": if total_flows > 0 { "meets_l1" } else { "missing" }
            })
        })
        .collect::<Vec<_>>();

    for (service, manifest) in &manifests {
        if seen.contains(service) {
            continue;
        }
        if !matrix_services.is_empty() && !matrix_services.contains(service) {
            continue;
        }
        services.push(json!({
            "service": service,
            "has_manifest": true,
            "l1_flows": 1,
            "total_flows": manifest.flows.len(),
            "quality": "meets_l1"
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
