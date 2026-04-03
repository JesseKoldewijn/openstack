/// Studio storage API: live snapshots of per-service in-memory resources.
///
/// Routes:
///   GET /_localstack/studio-api/storage
///   GET /_localstack/studio-api/storage/{service}
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_studio_ui::to_provider_slug;
use serde_json::json;

use crate::ApiState;

/// GET /_localstack/studio-api/storage
pub async fn list_all_storage(State(state): State<ApiState>) -> impl IntoResponse {
    let raw_snapshots = state.plugin_manager.storage_snapshots().await;
    let snapshots: Vec<serde_json::Value> = raw_snapshots
        .into_iter()
        .map(|(service, snapshot)| json!({ "service": service, "snapshot": snapshot }))
        .collect();
    Json(json!({ "schema_version": "1.0", "snapshots": snapshots }))
}

/// GET /_localstack/studio-api/storage/{service}
///
/// Accepts both manifest slugs (`events`) and provider slugs (`eventbridge`).
pub async fn get_service_storage(
    State(state): State<ApiState>,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let canonical = to_provider_slug(&service).to_string();
    let raw_snapshots = state.plugin_manager.storage_snapshots().await;

    match raw_snapshots
        .into_iter()
        .find(|(name, _)| name == &canonical)
    {
        Some((_, snapshot)) => {
            Json(json!({ "service": canonical, "snapshot": snapshot })).into_response()
        }
        None => {
            let service_states = state.plugin_manager.service_states().await;
            let is_registered = service_states.iter().any(|(name, _)| name == &canonical);
            if is_registered {
                Json(json!({
                    "service": canonical,
                    "snapshot": null,
                    "message": "Service is registered but does not expose storage snapshots",
                }))
                .into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "service_not_found",
                        "service": canonical,
                        "message": format!("Service '{canonical}' is not registered"),
                    })),
                )
                    .into_response()
            }
        }
    }
}
