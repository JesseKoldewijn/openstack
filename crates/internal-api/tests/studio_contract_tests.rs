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
    let mut state = ApiState::new(config, plugin_manager, shutdown_tx);
    state.session_id = "studio-contracts".to_string();
    state.start_time = Arc::new(Instant::now());
    state
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
    assert!(body["counts"]["guided_services"].is_u64());
    assert!(body["counts"]["supported_services"].is_u64());
    assert!(body["services"].is_array());
}

// ---------------------------------------------------------------------------
// Operations catalogue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn studio_operations_all_returns_schema_version_and_services() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/operations").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema_version"], "1.0");
    assert!(body["services"].is_array());
    // At least one service with operations should be present (static catalog)
    let services = body["services"].as_array().unwrap();
    assert!(!services.is_empty());
    let first = &services[0];
    assert!(first["service"].is_string());
    assert!(first["total"].is_u64());
    assert!(first["guided_count"].is_u64());
    assert!(first["operations"].is_array());
}

#[tokio::test]
async fn studio_operations_s3_contains_expected_operations() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/operations/s3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["service"], "s3");
    let ops = body["operations"].as_array().unwrap();
    assert!(!ops.is_empty());
    // Every operation entry must have the required fields
    for op in ops {
        assert!(op["name"].is_string(), "operation.name should be string");
        assert!(op["method"].is_string(), "operation.method should be string");
        assert!(op["path"].is_string(), "operation.path should be string");
        assert!(op["has_guided_flow"].is_boolean(), "operation.has_guided_flow should be bool");
    }
    // PutObject must be present
    let names: Vec<&str> = ops
        .iter()
        .filter_map(|o| o["name"].as_str())
        .collect();
    assert!(names.contains(&"PutObject"), "PutObject should be in S3 operations");
    assert!(names.contains(&"GetObject"), "GetObject should be in S3 operations");
}

#[tokio::test]
async fn studio_operations_unknown_service_returns_404() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/operations/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "service_not_found");
    assert_eq!(body["service"], "nonexistent");
}

// ---------------------------------------------------------------------------
// Storage snapshots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn studio_storage_all_returns_schema_version() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/storage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema_version"], "1.0");
    assert!(body["snapshots"].is_array());
    // No services are registered in test config, so snapshots array is empty
    assert_eq!(body["snapshots"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn studio_storage_unregistered_service_returns_404() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/storage/s3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "service_not_found");
}

// ---------------------------------------------------------------------------
// Transaction log
// ---------------------------------------------------------------------------

#[tokio::test]
async fn studio_transactions_all_returns_summary_and_empty_log() {
    let router = internal_api_router(make_state(test_config()));
    let (status, body) = get_json(&router, "/_localstack/studio-api/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema_version"], "1.0");
    assert!(body["summary"]["total"].is_u64());
    assert!(body["transactions"].is_array());
    assert_eq!(body["transactions"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn studio_transactions_record_and_retrieve() {
    use axum::http::Method;

    let state = make_state(test_config());
    let router = internal_api_router(state);

    // POST a transaction record
    let payload = serde_json::json!({
        "service": "s3",
        "operation": "PutObject",
        "method": "PUT",
        "path": "/my-bucket/my-key",
        "status": 200,
        "requestBodyPreview": null,
        "responseBodyPreview": "<PutObjectResult/>",
        "startedAtMs": 1000,
        "durationMs": 42,
        "fromGuidedFlow": false,
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri("/_localstack/studio-api/transactions/record")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(body["id"].as_u64().is_some());

    // Now list all transactions and verify the entry is present
    let (status, list_body) = get_json(&router, "/_localstack/studio-api/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txns = list_body["transactions"].as_array().unwrap();
    assert_eq!(txns.len(), 1);
    assert_eq!(txns[0]["service"], "s3");
    assert_eq!(txns[0]["status"], 200);
    assert_eq!(txns[0]["outcome"], "success");

    // List by service
    let (status, svc_body) = get_json(&router, "/_localstack/studio-api/transactions/s3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc_body["service"], "s3");
    assert_eq!(svc_body["transactions"].as_array().unwrap().len(), 1);

    // Different service should return empty list (not 404)
    let (status, empty_body) = get_json(&router, "/_localstack/studio-api/transactions/sqs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty_body["transactions"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn studio_transactions_clear_empties_log() {
    use axum::http::Method;

    let state = make_state(test_config());
    let router = internal_api_router(state);

    // Insert a record
    let payload = serde_json::json!({
        "service": "sqs", "method": "POST", "path": "/",
        "status": 200, "startedAtMs": 0, "durationMs": 5,
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/_localstack/studio-api/transactions/record")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();
    router.clone().oneshot(req).await.unwrap();

    // DELETE the log
    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri("/_localstack/studio-api/transactions")
        .body(Body::empty())
        .unwrap();
    let del_resp = router.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_bytes = axum::body::to_bytes(del_resp.into_body(), usize::MAX).await.unwrap();
    let del_body: Value = serde_json::from_slice(&del_bytes).unwrap();
    assert_eq!(del_body["cleared"], 1);

    // Log should now be empty
    let (status, body) = get_json(&router, "/_localstack/studio-api/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["transactions"].as_array().unwrap().len(), 0);
}

