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

    /// Creates a Config prepopulated with sensible defaults for tests.
    ///
    /// The returned configuration is tuned for an in-memory test gateway: it listens
    /// on 0.0.0.0:4566, disables persistence and debug features, sets conservative
    /// snapshot and CORS defaults, and loads service and directory settings from
    /// the environment.
    ///
    /// # Examples
    ///
    /// ```
    /// let cfg = test_config();
    /// assert_eq!(cfg.persistence, false);
    /// assert_eq!(cfg.gateway_listen[0].to_string(), "0.0.0.0:4566");
    /// assert_eq!(cfg.log_level, crate::config::LogLevel::Info);
    /// ```
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

    /// Ensures directory traversal in studio asset paths does not resolve to a whitelisted immutable asset.
    ///
    /// Sends a GET request to "/_localstack/studio/assets/../../secret" and verifies the response
    /// status is 200 OK and the `Cache-Control` header is not `public, max-age=31536000, immutable`.
    ///
    /// # Examples
    ///
    /// ```
    /// // within the test harness: build gateway app and send request to the traversal path
    /// # async fn example(app: _) {
    /// let req = Request::builder()
    ///     .method(Method::GET)
    ///     .uri("/_localstack/studio/assets/../../secret")
    ///     .body(Body::empty())
    ///     .unwrap();
    /// let resp = app.oneshot(req).await.unwrap();
    /// assert_eq!(resp.status(), StatusCode::OK);
    /// assert_ne!(
    ///     resp.headers().get("cache-control").and_then(|v| v.to_str().ok()),
    ///     Some("public, max-age=31536000, immutable")
    /// );
    /// # }
    /// ```
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
        // Should not hit whitelisted immutable asset route.
        assert_eq!(resp.status(), StatusCode::OK);
        assert_ne!(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
    }

    /// Verifies that requests to unknown Studio API endpoints return HTTP 404 Not Found.
    ///
    /// # Examples
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// let config = test_config();
    /// let manager = ServicePluginManager::new(config.clone());
    /// let gateway = Gateway::new(config, manager);
    /// let app = gateway.build_app_for_tests();
    ///
    /// let req = Request::builder()
    ///     .method(Method::GET)
    ///     .uri("/_localstack/studio-api/unknown")
    ///     .body(Body::empty())
    ///     .unwrap();
    /// let resp = app.oneshot(req).await.unwrap();
    /// assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    /// # });
    /// ```
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
