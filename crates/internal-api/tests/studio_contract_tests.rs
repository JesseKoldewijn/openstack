use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use openstack_config::{Config, Directories};
use openstack_internal_api::{internal_api_router, ApiState};
use openstack_service_framework::ServicePluginManager;
use serde_json::Value;
use tokio::sync::broadcast;
use tower::ServiceExt;

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
    assert!(body["services"].is_array());
}
