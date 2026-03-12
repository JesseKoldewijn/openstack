use axum::{Router, routing::get};

use crate::ApiState;

/// Builds an Axum router configured for all `/_localstack/*` endpoints.
///
/// The returned router has the provided `ApiState` attached via `with_state`.
///
/// # Examples
///
/// ```
/// // Create your ApiState (implementation-specific) and pass it in:
/// let router = internal_api_router(state);
/// // Use `router` with an Axum server or mount it into a larger router.
/// ```
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
        .route(
            "/_localstack/studio-api/services",
            get(crate::studio::get_studio_services),
        )
        .route(
            "/_localstack/studio-api/interactions/schema",
            get(crate::studio::get_studio_interaction_schema),
        )
        .route(
            "/_localstack/studio-api/flows/catalog",
            get(crate::studio::get_studio_flow_catalog),
        )
        .route(
            "/_localstack/studio-api/flows/coverage",
            get(crate::studio::get_studio_flow_coverage),
        )
        .route(
            "/_localstack/studio-api/flows/{service}",
            get(crate::studio::get_studio_flow_definition),
        )
        .with_state(state)
}
