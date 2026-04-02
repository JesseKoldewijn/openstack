/// Studio storage API: live snapshots of per-service in-memory resources.
///
/// Routes:
///   GET /_localstack/studio-api/storage
///   GET /_localstack/studio-api/storage/{service}
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;

use crate::ApiState;

/// GET /_localstack/studio-api/storage
///
/// Returns storage snapshots for all services that implement the
/// `storage_snapshot()` introspection hook.
pub async fn list_all_storage(State(state): State<ApiState>) -> impl IntoResponse {
    let raw_snapshots = state.plugin_manager.storage_snapshots().await;

    let snapshots: Vec<serde_json::Value> = raw_snapshots
        .into_iter()
        .map(|(service, snapshot)| {
            json!({
                "service": service,
                "snapshot": snapshot,
            })
        })
        .collect();

    Json(json!({
        "schema_version": "1.0",
        "snapshots": snapshots,
    }))
}

/// GET /_localstack/studio-api/storage/{service}
///
/// Returns the storage snapshot for a single service.
/// Returns 404 if the service is not registered or does not implement
/// the storage introspection hook.
pub async fn get_service_storage(
    State(state): State<ApiState>,
    Path(service): Path<String>,
) -> impl IntoResponse {
    // Gather all snapshots and find the one matching this service slug.
    // The plugin manager doesn't expose a per-service introspection call yet,
    // so we collect all and filter — the overhead is negligible.
    let raw_snapshots = state.plugin_manager.storage_snapshots().await;

    match raw_snapshots.into_iter().find(|(name, _)| name == &service) {
        Some((_, snapshot)) => Json(json!({
            "service": service,
            "snapshot": snapshot,
        }))
        .into_response(),
        None => {
            // Check whether the service is registered at all.
            let service_states = state.plugin_manager.service_states().await;
            let is_registered = service_states.iter().any(|(name, _)| name == &service);

            if is_registered {
                // Registered but no storage snapshot — return empty.
                Json(json!({
                    "service": service,
                    "snapshot": null,
                    "message": "Service is registered but does not expose storage snapshots",
                }))
                .into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "service_not_found",
                        "service": service,
                        "message": format!("Service '{service}' is not registered"),
                    })),
                )
                    .into_response()
            }
        }
    }
}
