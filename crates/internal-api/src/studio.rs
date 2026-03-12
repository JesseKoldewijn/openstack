use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use openstack_service_framework::ServiceState;
use serde_json::json;

use crate::ApiState;

fn support_tier(service: &str) -> &'static str {
    match service {
        "s3" | "sqs" => "guided",
        _ => "raw",
    }
}

pub async fn get_studio_services(State(state): State<ApiState>) -> impl IntoResponse {
    let service_states = state.plugin_manager.service_states().await;
    let services: Vec<serde_json::Value> = service_states
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
                "support_tier": support_tier(&name),
            })
        })
        .collect();

    Json(json!({
        "services": services,
    }))
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
