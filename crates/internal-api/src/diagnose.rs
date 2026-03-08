use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::ApiState;

pub async fn get_diagnose(State(state): State<ApiState>) -> Response {
    // Only available when DEBUG=1
    if !state.config.debug {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "diagnose endpoint requires DEBUG=1"})),
        )
            .into_response();
    }

    let service_states = state.plugin_manager.service_states().await;
    let services: serde_json::Value = service_states
        .iter()
        .map(|(name, svc_state)| (name.clone(), json!({ "status": svc_state.as_str() })))
        .collect::<serde_json::Map<_, _>>()
        .into();

    let body = json!({
        "info": {
            "implementation": "openstack",
            "version": env!("CARGO_PKG_VERSION"),
            "session_id": state.session_id,
            "uptime": state.start_time.elapsed().as_secs(),
        },
        "config": {
            "gateway_listen": state.config.gateway_listen.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            "persistence": state.config.persistence,
            "debug": state.config.debug,
            "localstack_host": state.config.localstack_host,
        },
        "services": services,
    });
    Json(body).into_response()
}
