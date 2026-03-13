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
}
