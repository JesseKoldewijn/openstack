use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use openstack_service_framework::ServiceState;
use serde_json::{Value, json};

use crate::ApiState;

pub async fn get_health(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;

    let mut services = serde_json::Map::new();
    for (name, svc_state) in &service_states {
        let status_str = match svc_state {
            ServiceState::Running => "running",
            ServiceState::Starting => "starting",
            ServiceState::Stopping => "stopping",
            ServiceState::Stopped => "stopped",
            ServiceState::Available => "available",
            ServiceState::Error(_) => "error",
        };
        services.insert(name.clone(), json!({ "status": status_str }));
    }

    let body = json!({
        "edition": "community",
        "version": env!("CARGO_PKG_VERSION"),
        "services": services,
    });
    Json(body)
}

pub async fn head_health() -> impl IntoResponse {
    StatusCode::OK
}

pub async fn post_health(
    State(state): State<ApiState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    match body.get("action").and_then(|a| a.as_str()) {
        Some("restart") => {
            // Signal the shutdown channel which the main binary can reconnect
            let _ = state.shutdown_tx.send(());
            Json(json!({"message": "restart requested"}))
        }
        Some("kill") => {
            let _ = state.shutdown_tx.send(());
            Json(json!({"message": "kill requested"}))
        }
        _ => Json(json!({"error": "unknown action; supported: restart, kill"})),
    }
}
