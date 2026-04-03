/// Studio operations API: per-service operation catalogue.
///
/// Routes:
///   GET /_localstack/studio-api/operations
///   GET /_localstack/studio-api/operations/{service}
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_studio_ui::{OperationCatalog, OperationEntry, to_provider_slug};
use serde_json::json;
use tracing::warn;

use crate::ApiState;

fn normalize_protocol_alias(mut value: serde_json::Value) -> serde_json::Value {
    // Some guided manifests still use legacy protocol alias names (e.g. "ec2").
    // OperationCatalog expects ProtocolClass variants from studio-ui.
    if let Some(protocol) = value
        .get_mut("protocol")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
    {
        let normalized = match protocol.as_str() {
            "ec2" => "query",
            // Backward-compatible aliases (if encountered)
            "json" => "json_target",
            "restxml" => "rest_xml",
            "restjson" => "rest_json",
            _ => protocol.as_str(),
        };
        if let Some(p) = value.get_mut("protocol") {
            *p = serde_json::Value::String(normalized.to_string());
        }
    }
    value
}

fn build_catalog(state: &ApiState) -> OperationCatalog {
    let mut manifests: Vec<openstack_studio_ui::GuidedManifest> = Vec::new();

    for (service, raw) in &state.guided_manifest_inventory {
        match serde_json::to_value(raw)
            .map(normalize_protocol_alias)
            .and_then(serde_json::from_value::<openstack_studio_ui::GuidedManifest>)
        {
            Ok(manifest) => manifests.push(manifest),
            Err(err) => {
                warn!(
                    service = %service,
                    error = %err,
                    "failed to decode guided manifest for operation catalog; skipping entry"
                );
            }
        }
    }

    OperationCatalog::build(&manifests)
}

fn operation_to_json(op: &OperationEntry) -> serde_json::Value {
    json!({
        "name": op.name,
        "method": op.method,
        "path": op.path,
        "has_guided_flow": op.has_guided_flow,
    })
}

pub async fn list_all_operations(State(state): State<ApiState>) -> impl IntoResponse {
    let catalog = build_catalog(&state);
    let mut services: Vec<serde_json::Value> = catalog
        .all_services()
        .map(|set| {
            json!({
                "service": set.service,
                "total": set.total(),
                "guided_count": set.guided_count(),
                "operations": set.operations.iter().map(operation_to_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    services.sort_by(|a, b| {
        a["service"]
            .as_str()
            .unwrap_or("")
            .cmp(b["service"].as_str().unwrap_or(""))
    });
    Json(json!({ "schema_version": "1.0", "services": services }))
}

/// GET /_localstack/studio-api/operations/{service}
///
/// Accepts both manifest slugs (`events`) and provider slugs (`eventbridge`).
pub async fn get_service_operations(
    State(state): State<ApiState>,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let catalog = build_catalog(&state);
    let canonical = to_provider_slug(&service).to_string();
    match catalog.for_service(&canonical) {
        Some(set) => Json(json!({
            "service": set.service,
            "total": set.total(),
            "guided_count": set.guided_count(),
            "operations": set.operations.iter().map(operation_to_json).collect::<Vec<_>>(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "service_not_found",
                "service": canonical,
                "message": format!("No operation catalogue entry for service '{canonical}'"),
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalize_protocol_alias_maps_ec2_to_query() {
        let raw = serde_json::json!({
            "schemaVersion": "1.2",
            "service": "ec2",
            "protocol": "ec2",
            "flows": []
        });

        let normalized = super::normalize_protocol_alias(raw);
        assert_eq!(normalized["protocol"], "query");

        let parsed = serde_json::from_value::<openstack_studio_ui::GuidedManifest>(normalized)
            .expect("normalized manifest should decode");
        assert_eq!(parsed.service, "ec2");
    }
}
