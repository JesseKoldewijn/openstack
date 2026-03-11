use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use openstack_service_framework::ServiceState;
use serde_json::json;

use crate::ApiState;

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
                "startup_attempts": metric.map(|m| m.startup_attempts).unwrap_or(0),
                "startup_wait_count": metric.map(|m| m.startup_wait_count).unwrap_or(0),
                "last_startup_duration_ms": metric.map(|m| m.last_startup_duration_ms).unwrap_or(0),
            })
        })
        .collect();

    Json(json!({ "plugins": plugins }))
}
