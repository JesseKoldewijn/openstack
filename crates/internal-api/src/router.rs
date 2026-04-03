use axum::{
    Router,
    routing::{get, post},
};

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
        // --- Studio: service catalogue & guided flows ---
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
        // --- Studio: runtime config (credentials, endpoint, polling) ---
        .route(
            "/_localstack/studio-api/runtime-config",
            get(crate::studio_runtime_config::get_runtime_config),
        )
        // --- Studio: per-service operation catalogue ---
        .route(
            "/_localstack/studio-api/operations",
            get(crate::studio_operations::list_all_operations),
        )
        .route(
            "/_localstack/studio-api/operations/{service}",
            get(crate::studio_operations::get_service_operations),
        )
        // --- Studio: live storage snapshots ---
        .route(
            "/_localstack/studio-api/storage",
            get(crate::studio_storage::list_all_storage),
        )
        .route(
            "/_localstack/studio-api/storage/{service}",
            get(crate::studio_storage::get_service_storage),
        )
        // --- Studio: transaction log ---
        .route(
            "/_localstack/studio-api/transactions",
            get(crate::studio_transactions::list_all_transactions)
                .delete(crate::studio_transactions::clear_transactions),
        )
        .route(
            "/_localstack/studio-api/transactions/{service}",
            get(crate::studio_transactions::list_service_transactions)
                .delete(crate::studio_transactions::clear_service_transactions),
        )
        .route(
            "/_localstack/studio-api/transactions/record",
            post(crate::studio_transactions::record_transaction),
        )
        .with_state(state)
}
