use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use openstack_service_framework::ServiceState;
use serde_json::json;

use crate::ApiState;

pub async fn get_plugins(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;

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
            json!({
                "name": name,
                "status": status_str,
            })
        })
        .collect();

    Json(json!({ "plugins": plugins }))
}
