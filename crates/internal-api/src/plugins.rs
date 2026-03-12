use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use openstack_service_framework::ServiceState;
use serde_json::json;

use crate::ApiState;

/// Return a JSON object describing all registered plugins, their computed status, studio support tier, and selected runtime metrics.
///
/// The response body is an object with a `plugins` array; each element contains `name`, `status`, `studio_support_tier`, `startup_attempts`, `startup_wait_count`, and `last_startup_duration_ms`.
///
/// # Examples
///
/// ```no_run
/// # use axum::extract::State;
/// # use serde_json::Value;
/// # async fn example() {
/// // `state` should be an instance of ApiState wired with a plugin manager.
/// // let state = ApiState::new(...);
/// // let resp = get_plugins(State(state)).await;
/// // let json: Value = resp.into_response().into_body().into();
/// // assert!(json.get("plugins").is_some());
/// # }
/// ```
pub async fn get_plugins(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let runtime_metrics = state.plugin_manager.service_runtime_metrics().await;
    let mut metrics_map = std::collections::HashMap::new();
    for metric in runtime_metrics {
        metrics_map.insert(metric.service.clone(), metric);
    }

    let plugins: Vec<serde_json::Value> = service_states
        .iter()
        .map(|(name, svc_state)| {
            let status_str = match svc_state {
                ServiceState::Running => "running",
                ServiceState::Starting => "starting",
                ServiceState::Stopping => "stopping",
                ServiceState::Stopped => "stopped",
                ServiceState::Available => "available",
                ServiceState::Error(_) => "error",
            };
            let metric = metrics_map.get(name);
            json!({
                "name": name,
                "status": status_str,
                "studio_support_tier": match name.as_str() {
                    "s3" | "sqs" => "guided",
                    _ => "raw",
                },
                "startup_attempts": metric.map(|m| m.startup_attempts).unwrap_or(0),
                "startup_wait_count": metric.map(|m| m.startup_wait_count).unwrap_or(0),
                "last_startup_duration_ms": metric.map(|m| m.last_startup_duration_ms).unwrap_or(0),
            })
        })
        .collect();

    Json(json!({ "plugins": plugins }))
}
