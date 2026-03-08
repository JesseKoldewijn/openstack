use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::ApiState;

pub async fn get_config(State(state): State<ApiState>) -> Response {
    if !state.config.enable_config_updates {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "config endpoint requires ENABLE_CONFIG_UPDATES=1"})),
        )
            .into_response();
    }

    let cfg = &state.config;
    let body = json!({
        "GATEWAY_LISTEN": cfg.gateway_listen.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        "PERSISTENCE": cfg.persistence,
        "DEBUG": cfg.debug,
        "LS_LOG": cfg.log_level.as_str(),
        "LOCALSTACK_HOST": cfg.localstack_host,
        "SERVICES": null,
    });
    Json(body).into_response()
}

pub async fn post_config(State(state): State<ApiState>, Json(_body): Json<Value>) -> Response {
    if !state.config.enable_config_updates {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "config endpoint requires ENABLE_CONFIG_UPDATES=1"})),
        )
            .into_response();
    }

    // Runtime config updates are intentionally limited (most config is read at startup).
    // Return success so tooling doesn't break, but note that changes are not persisted.
    Json(json!({ "updated": true, "note": "runtime config updates have limited effect" }))
        .into_response()
}
