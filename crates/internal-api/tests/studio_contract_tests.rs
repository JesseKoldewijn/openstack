use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use openstack_config::{Config, Directories};
use openstack_internal_api::{ApiState, internal_api_router};
use openstack_service_framework::ServicePluginManager;
use serde_json::Value;
use tokio::sync::broadcast;
use tower::ServiceExt;

/// Produces a Config pre-populated with sensible defaults for use in tests.
///
/// The returned configuration uses test-friendly settings (persistence disabled, debug disabled,
/// snapshot strategies suitable for tests, default CORS checks enabled, and directories/services
/// loaded from the environment).
///
/// # Examples
///
/// ```
/// let cfg = test_config();
/// // persistence is disabled in the test configuration
/// assert!(!cfg.persistence);
/// ```
fn test_config() -> Config {
    Config {
        gateway_listen: vec!["0.0.0.0:4566".parse().unwrap()],
        persistence: false,
        services: openstack_config::ServicesConfig::from_env(),
        debug: false,
        log_level: openstack_config::LogLevel::Info,
        localstack_host: "localhost:4566".to_string(),
        allow_nonstandard_regions: false,
        cors: openstack_config::CorsConfig {
            disable_cors_headers: false,
            disable_cors_checks: false,
            extra_allowed_origins: vec![],
            extra_allowed_headers: vec![],
        },
        snapshot_save_strategy: openstack_config::SnapshotSaveStrategy::OnShutdown,
        snapshot_load_strategy: openstack_config::SnapshotLoadStrategy::OnStartup,
        snapshot_flush_interval: std::time::Duration::from_secs(15),
        dns_address: None,
        dns_port: 53,
        dns_resolve_ip: "127.0.0.1".to_string(),
        lambda_keepalive_ms: 600_000,
        lambda_remove_containers: true,
        bucket_marker_local: None,
        eager_service_loading: false,
        enable_config_updates: false,
        directories: Directories::from_env(),
        body_spool_threshold_bytes: 1_048_576,
    }
}

/// Builds an ApiState initialized for studio contract tests.
///
/// The returned state contains the provided `config`, a `ServicePluginManager` created from a clone
/// of that config, a `session_id` set to `"studio-contracts"`, the current `start_time`, and a
/// shutdown broadcast sender.
///
/// # Examples
///
/// ```
/// let cfg = test_config();
/// let state = make_state(cfg);
/// assert_eq!(state.session_id, "studio-contracts");
/// ```
fn make_state(config: Config) -> ApiState {
    let (shutdown_tx, _) = broadcast::channel(1);
    let plugin_manager = ServicePluginManager::new(config.clone());
    ApiState {
        config,
        plugin_manager,
        session_id: "studio-contracts".to_string(),
        start_time: Arc::new(Instant::now()),
        shutdown_tx,
    }
}

/// Sends a GET request to `path` against the provided `router` and returns the response status and the body parsed as JSON.
///
/// The body is parsed using `serde_json::from_slice`; if parsing fails the function returns `serde_json::Value::Null`.
///
/// # Examples
///
/// ```
/// # use axum::Router;
/// # use http::StatusCode;
/// # use serde_json::Value;
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// let router = Router::new();
/// let (status, json): (StatusCode, Value) = get_json(&router, "/some/path").await;
/// // `status` contains the HTTP status code and `json` the parsed body (or `Value::Null` on parse failure).
/// # });
/// ```
async fn get_json(router: &axum::Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

/// Integration test that verifies the studio services API response contains expected fields.
///
/// Asserts that the endpoint `/_localstack/studio-api/services` returns HTTP 200 and a JSON
/// body where `services` is an array. If the array contains at least one item, the first item
/// must include the keys `name`, `status`, and `support_tier`.
///
/// # Examples
///
/// ```
/// // This example demonstrates the assertions performed by the test.
/// let router = internal_api_router(make_state(test_config()));
/// let (status, body) = get_json(&router, "/_localstack/studio-api/services").await;
/// assert_eq!(status, StatusCode::OK);
/// assert!(body["services"].is_array());
/// if let Some(first) = body["services"].as_array().and_then(|x| x.first()) {
///     assert!(first.get("name").is_some());
///     assert!(first.get("status").is_some());
///     assert!(first.get("support_tier").is_some());
/// }
/// ```
#[tokio::test]
async fn studio_services_contract_contains_expected_fields() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/services").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["services"].is_array());
    if let Some(first) = body["services"].as_array().and_then(|x| x.first()) {
        assert!(first.get("name").is_some());
        assert!(first.get("status").is_some());
        assert!(first.get("support_tier").is_some());
    }
}

#[tokio::test]
async fn studio_interaction_schema_contract_contains_fields() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/interactions/schema").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["request"]["fields"].is_array());
    assert!(body["response"]["fields"].is_array());
}

/// Verifies the flows catalog endpoint returns a services array and that a service entry contains required fields.
///
/// Ensures the response status is 200 and that `services` is an array. If a first service entry exists, asserts it contains
/// `service`, `manifest_version`, `protocol`, `flow_count`, and `maturity`.
///
/// # Examples
///
/// ```no_run
/// let router = internal_api_router(make_state(test_config()));
/// let (status, body) = get_json(&router, "/_localstack/studio-api/flows/catalog").await;
/// assert_eq!(status, StatusCode::OK);
/// assert!(body["services"].is_array());
/// ```
#[tokio::test]
async fn studio_flow_catalog_contract_contains_expected_fields() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/flows/catalog").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["services"].is_array());
    if let Some(first) = body["services"].as_array().and_then(|x| x.first()) {
        assert!(first.get("service").is_some());
        assert!(first.get("manifest_version").is_some());
        assert!(first.get("protocol").is_some());
        assert!(first.get("flow_count").is_some());
        assert!(first.get("maturity").is_some());
    }
}

/// Verifies the studio flows definition endpoint returns data specific to the `s3` service.
///
/// The test sends a GET request to `/_localstack/studio-api/flows/s3` and asserts the response
/// has HTTP 200, the `service` field equals `"s3"`, and the `flows` field is an array.
///
/// # Examples
///
/// ```
/// let router = internal_api_router(make_state(test_config()));
/// let (status, body) = get_json(&router, "/_localstack/studio-api/flows/s3").await;
/// assert_eq!(status, StatusCode::OK);
/// assert_eq!(body["service"], "s3");
/// assert!(body["flows"].is_array());
/// ```
#[tokio::test]
async fn studio_flow_definition_contract_is_service_specific() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/flows/s3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["service"], "s3");
    assert!(body["flows"].is_array());
}

#[tokio::test]
async fn studio_flow_coverage_contract_contains_metrics() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/flows/coverage").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["schema_version"].is_string());
    assert!(body["counts"]["guided_services"].is_u64());
    assert!(body["counts"]["supported_services"].is_u64());
    assert!(body["services"].is_array());
}
