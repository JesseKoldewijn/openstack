/// Studio operations API: per-service operation catalogue.
///
/// Routes:
///   GET /_localstack/studio-api/operations
///   GET /_localstack/studio-api/operations/{service}
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_studio_ui::{OperationCatalog, OperationEntry};
use serde_json::json;

use crate::ApiState;

fn build_catalog(state: &ApiState) -> OperationCatalog {
    // Build the operation catalog enriched with guided-flow markers from the
    // loaded manifest inventory.  Manifests are already parsed at startup and
    // stored in ApiState, so re-using them here is zero-cost.
    let manifests: Vec<openstack_studio_ui::GuidedManifest> = state
        .guided_manifest_inventory
        .values()
        .filter_map(|raw| {
            serde_json::to_value(raw)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
        })
        .collect();
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

/// GET /_localstack/studio-api/operations
///
/// Returns the operation catalogue for every service that has static
/// operation metadata, grouped by service slug.
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

    Json(json!({
        "schema_version": "1.0",
        "services": services,
    }))
}

/// GET /_localstack/studio-api/operations/{service}
///
/// Returns the full operation list for a single service.
/// Returns 404 if the service slug is not found in the static catalogue.
pub async fn get_service_operations(
    State(state): State<ApiState>,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let catalog = build_catalog(&state);

    match catalog.for_service(&service) {
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
                "service": service,
                "message": format!("No operation catalogue entry for service '{service}'"),
            })),
        )
            .into_response(),
    }
}
