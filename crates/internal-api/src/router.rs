use axum::{Router, routing::get};

use crate::ApiState;

/// Build the Axum router for all `/_localstack/*` endpoints.
/// The router requires `ApiState` to be injected via `.with_state(...)`.
pub fn internal_api_router(state: ApiState) -> Router {
    Router::new()
        .route(
            "/_localstack/health",
            get(crate::health::get_health)
                .head(crate::health::head_health)
                .post(crate::health::post_health),
        )
        .route("/_localstack/info", get(crate::info::get_info))
        .route("/_localstack/init", get(crate::init::get_init))
        .route(
            "/_localstack/init/{stage}",
            get(crate::init::get_init_stage),
        )
        .route("/_localstack/plugins", get(crate::plugins::get_plugins))
        .route("/_localstack/diagnose", get(crate::diagnose::get_diagnose))
        .route(
            "/_localstack/config",
            get(crate::config_api::get_config).post(crate::config_api::post_config),
        )
        .with_state(state)
}
