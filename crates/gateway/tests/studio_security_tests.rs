#[cfg(test)]
mod studio_security_tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use openstack_config::{
        Config, CorsConfig, Directories, LogLevel, ServicesConfig, SnapshotLoadStrategy,
        SnapshotSaveStrategy,
    };
    use openstack_gateway::Gateway;
    use openstack_service_framework::ServicePluginManager;
    use tower::ServiceExt;
    fn test_config() -> Config {
        Config {
            gateway_listen: vec!["0.0.0.0:4566".parse().unwrap()],
            persistence: false,
            services: ServicesConfig::from_env(),
            debug: false,
            log_level: LogLevel::Info,
            localstack_host: "localhost:4566".to_string(),
            allow_nonstandard_regions: false,
            cors: CorsConfig {
                disable_cors_headers: false,
                disable_cors_checks: false,
                extra_allowed_origins: vec![],
                extra_allowed_headers: vec![],
            },
            snapshot_save_strategy: SnapshotSaveStrategy::OnShutdown,
            snapshot_load_strategy: SnapshotLoadStrategy::OnStartup,
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

    #[tokio::test]
    async fn studio_asset_path_disallows_directory_traversal() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio/assets/../../secret")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Unknown assets under the asset namespace should be explicit 404.
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_ne!(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
    }

    #[tokio::test]
    async fn unknown_studio_api_endpoint_returns_not_found() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio-api/unknown")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn guided_execution_endpoint_rejects_disallowed_method() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio-api/flows/execute")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn guided_execution_endpoint_rejects_oversized_payload() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let payload = vec![b'a'; 300 * 1024];
        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/studio-api/flows/execute")
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    // ── Studio-enabled / disabled gateway construction ──────────────────

    #[tokio::test]
    async fn gateway_default_has_studio_disabled() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        // Default gateway should not have studio enabled.
        assert!(!gateway.studio_enabled());
    }

    #[tokio::test]
    async fn gateway_new_with_studio_has_studio_enabled() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new_with_studio(config, manager);
        assert!(gateway.studio_enabled());
    }

    #[tokio::test]
    async fn gateway_debug_config_enables_studio() {
        let mut config = test_config();
        config.debug = true;
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        assert!(gateway.studio_enabled());
    }

    #[tokio::test]
    async fn studio_disabled_tx_record_endpoint_returns_created_silently() {
        // In default (non-studio) mode the record endpoint is a no-op
        // but must still return 201 so the caller doesn't error.
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let payload = serde_json::json!({
            "service": "s3", "method": "GET", "path": "/",
            "status": 200, "startedAtMs": 0, "durationMs": 5,
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/_localstack/studio-api/transactions/record")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn studio_disabled_tx_list_endpoint_returns_empty_ok() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio-api/transactions")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["transactions"].as_array().unwrap().len(), 0);
        assert_eq!(body["_studio_disabled"], true);
    }
}
