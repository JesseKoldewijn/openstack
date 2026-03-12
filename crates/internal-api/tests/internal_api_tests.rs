//! Integration tests for the /_localstack/* internal API endpoints.

#[cfg(test)]
mod internal_api_tests {
    use std::sync::Arc;
    use std::time::Instant;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use openstack_config::{Config, Directories};
    use openstack_internal_api::{ApiState, internal_api_router};
    use openstack_service_framework::ServicePluginManager;
    use serde_json::Value;
    use tokio::sync::broadcast;
    use tower::ServiceExt; // for `oneshot`

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

    fn test_debug_config() -> Config {
        let mut cfg = test_config();
        cfg.debug = true;
        cfg
    }

    fn test_config_updates_config() -> Config {
        let mut cfg = test_config();
        cfg.enable_config_updates = true;
        cfg
    }

    fn make_state(config: Config) -> ApiState {
        let (shutdown_tx, _) = broadcast::channel(1);
        let plugin_manager = ServicePluginManager::new(config.clone());
        ApiState {
            config,
            plugin_manager,
            session_id: "test-session-id".to_string(),
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

    // ── 7.1  GET /_localstack/health ──────────────────────────────────────────

    #[tokio::test]
    async fn health_get_returns_edition_and_version() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["edition"], "community");
        assert!(body["version"].is_string());
        assert!(body["services"].is_object());
    }

    // ── 7.2  HEAD /_localstack/health ─────────────────────────────────────────

    #[tokio::test]
    async fn health_head_returns_200() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let req = Request::builder()
            .method(Method::HEAD)
            .uri("/_localstack/health")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── 7.3  POST /_localstack/health ─────────────────────────────────────────

    #[tokio::test]
    async fn health_post_unknown_action_returns_error() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/health")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action": "unknown"}"#))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].is_string());
    }

    #[tokio::test]
    async fn health_post_kill_action_sends_shutdown() {
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        let config = test_config();
        let plugin_manager = ServicePluginManager::new(config.clone());
        let state = ApiState {
            config,
            plugin_manager,
            session_id: "test".to_string(),
            start_time: Arc::new(Instant::now()),
            shutdown_tx,
        };
        let router = internal_api_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/health")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action": "kill"}"#))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // The shutdown channel should have received a signal
        assert!(shutdown_rx.try_recv().is_ok());
    }

    // ── 7.4  GET /_localstack/info ────────────────────────────────────────────

    #[tokio::test]
    async fn info_returns_version_and_session_id() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/info").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["edition"], "community");
        assert!(body["version"].is_string());
        assert_eq!(body["session_id"], "test-session-id");
        assert!(body["uptime"].is_number());
        assert_eq!(body["studio"]["enabled"], true);
        assert_eq!(body["studio"]["base_path"], "/_localstack/studio");
        assert_eq!(body["studio"]["api_base_path"], "/_localstack/studio-api");
        assert_eq!(
            body["studio"]["guided_flow"]["manifest_schema_version"],
            "1.2"
        );
        assert_eq!(
            body["studio"]["guided_flow"]["catalog_endpoint"],
            "/_localstack/studio-api/flows/catalog"
        );
        assert_eq!(
            body["studio"]["guided_flow"]["coverage_endpoint"],
            "/_localstack/studio-api/flows/coverage"
        );
        assert!(body["daemon"]["managed"].is_boolean());
        assert!(body["daemon"]["pid"].is_number());
    }

    // ── 7.6  GET /_localstack/init ────────────────────────────────────────────

    #[tokio::test]
    async fn init_returns_scripts_map() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/init").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["scripts"].is_object());
    }

    #[tokio::test]
    async fn init_stage_returns_scripts_array() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/init/boot").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["scripts"].is_array());
    }

    // ── 7.7  GET /_localstack/plugins ─────────────────────────────────────────

    #[tokio::test]
    async fn plugins_returns_plugins_array() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/plugins").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["plugins"].is_array());
        if let Some(first) = body["plugins"].as_array().and_then(|a| a.first()) {
            assert!(first.get("startup_attempts").is_some());
            assert!(first.get("startup_wait_count").is_some());
            assert!(first.get("last_startup_duration_ms").is_some());
            assert!(first.get("studio_support_tier").is_some());
        }
    }

    #[tokio::test]
    async fn health_includes_daemon_metadata() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["daemon"]["managed"].is_boolean());
        assert_eq!(body["daemon"]["status"], "running");
        assert!(body["daemon"]["pid"].is_number());
    }

    #[tokio::test]
    async fn studio_api_services_returns_contract_shape() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/studio-api/services").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["services"].is_array());
    }

    /// Checks that the studio API's interactions schema endpoint returns a contract containing
    /// `request.fields` and `response.fields` as arrays.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Integration test example (runs inside the test harness)
    /// async fn example() {
    ///     let state = make_state(test_config());
    ///     let router = internal_api_router(state);
    ///     let (_status, body) = get_json(&router, "/_localstack/studio-api/interactions/schema").await;
    ///     assert!(body["request"]["fields"].is_array());
    ///     assert!(body["response"]["fields"].is_array());
    /// }
    /// ```
    #[tokio::test]
    async fn studio_api_interaction_schema_returns_contract_shape() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/studio-api/interactions/schema").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["request"]["fields"].is_array());
        assert!(body["response"]["fields"].is_array());
    }

    #[tokio::test]
    async fn studio_api_flow_catalog_returns_contract_shape() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/studio-api/flows/catalog").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["services"].is_array());
    }

    #[tokio::test]
    async fn studio_api_flow_definition_returns_contract_shape() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/studio-api/flows/s3").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["service"], "s3");
        assert!(body["flows"].is_array());
    }

    #[tokio::test]
    async fn studio_api_flow_coverage_returns_contract_shape() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/studio-api/flows/coverage").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["services"].is_array());
        assert!(body["schema_version"].is_string());
    }

    // ── 7.8  GET /_localstack/diagnose ────────────────────────────────────────

    #[tokio::test]
    async fn diagnose_requires_debug_mode() {
        let state = make_state(test_config()); // debug = false
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/diagnose").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body["error"].is_string());
    }

    #[tokio::test]
    async fn diagnose_returns_info_when_debug_enabled() {
        let state = make_state(test_debug_config()); // debug = true
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/diagnose").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["info"].is_object());
        assert!(body["config"].is_object());
        assert!(body["services"].is_object());
    }

    // ── 7.9  GET/POST /_localstack/config ─────────────────────────────────────

    #[tokio::test]
    async fn config_get_requires_enable_config_updates() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/config").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body["error"].is_string());
    }

    #[tokio::test]
    async fn config_get_returns_config_when_enabled() {
        let state = make_state(test_config_updates_config());
        let router = internal_api_router(state);

        let (status, body) = get_json(&router, "/_localstack/config").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["PERSISTENCE"].is_boolean());
        assert!(body["DEBUG"].is_boolean());
    }

    #[tokio::test]
    async fn config_post_requires_enable_config_updates() {
        let state = make_state(test_config());
        let router = internal_api_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn config_post_returns_updated_when_enabled() {
        let state = make_state(test_config_updates_config());
        let router = internal_api_router(state);

        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/config")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .unwrap();
        let resp = router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["updated"], true);
    }
}
