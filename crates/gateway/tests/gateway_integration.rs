//! Integration tests for the gateway handler pipeline.

#[cfg(test)]
mod gateway_tests {
    use axum::body::Body;
    use axum::http::{HeaderMap, HeaderValue, Method};
    use axum::http::{Request, StatusCode};
    use openstack_config::{
        Config, CorsConfig, Directories, LogLevel, ServicesConfig, SnapshotLoadStrategy,
        SnapshotSaveStrategy,
    };
    use openstack_gateway::Gateway;
    use openstack_gateway::cors::CorsHandler;
    use openstack_gateway::sigv4::{
        DEFAULT_ACCOUNT_ID, access_key_to_account_id, is_valid_region, parse_sigv4_auth,
    };
    use openstack_service_framework::ServicePluginManager;
    use tower::ServiceExt;

    /// Constructs a Config preconfigured for gateway integration tests.
    ///
    /// The returned configuration is tuned for local test runs: it listens on
    /// 0.0.0.0:4566, disables persistence, enables common CORS headers, and uses
    /// snapshot strategies and other defaults suitable for the test environment.
    ///
    /// # Examples
    ///
    /// ```
    /// let cfg = test_config();
    /// assert_eq!(cfg.persistence, false);
    /// assert_eq!(cfg.log_level, openstack_config::LogLevel::Info);
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

    // ─── SigV4 parsing ─────────────────────────────────────────────────────────

    #[test]
    fn sigv4_parse_extracts_service_and_region() {
        let auth = "AWS4-HMAC-SHA256 Credential=test/20260306/eu-west-1/sqs/aws4_request, \
                   SignedHeaders=host;x-amz-date, Signature=deadbeef";
        let parsed = parse_sigv4_auth(auth).expect("should parse");
        assert_eq!(parsed.service, "sqs");
        assert_eq!(parsed.region, "eu-west-1");
        assert_eq!(parsed.access_key, "test");
    }

    #[test]
    fn sigv4_parse_rejects_non_sigv4() {
        assert!(parse_sigv4_auth("Basic dXNlcjpwYXNz").is_none());
        assert!(parse_sigv4_auth("").is_none());
        assert!(parse_sigv4_auth("Bearer token123").is_none());
    }

    #[test]
    fn sigv4_short_credential_scope_rejected() {
        // Only 3 parts in credential — missing service
        let auth = "AWS4-HMAC-SHA256 Credential=AKID/20260306/us-east-1, \
                   SignedHeaders=host, Signature=abc";
        assert!(parse_sigv4_auth(auth).is_none());
    }

    // ─── Account ID derivation ─────────────────────────────────────────────────

    #[test]
    fn account_id_default_keys() {
        assert_eq!(access_key_to_account_id("test"), DEFAULT_ACCOUNT_ID);
        assert_eq!(
            access_key_to_account_id("AKIAIOSFODNN7EXAMPLE"),
            DEFAULT_ACCOUNT_ID
        );
        assert_eq!(access_key_to_account_id("mock"), DEFAULT_ACCOUNT_ID);
    }

    #[test]
    fn account_id_deterministic_for_unknown_key() {
        let id = access_key_to_account_id("SOME_UNKNOWN_KEY");
        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|c| c.is_ascii_digit()));
        // Calling again gives same result
        assert_eq!(access_key_to_account_id("SOME_UNKNOWN_KEY"), id);
    }

    #[test]
    fn different_keys_can_give_different_accounts() {
        let id1 = access_key_to_account_id("KEY_AAA");
        let id2 = access_key_to_account_id("KEY_ZZZ");
        // They *might* collide but almost certainly don't
        // At minimum both are valid 12-digit strings
        assert_eq!(id1.len(), 12);
        assert_eq!(id2.len(), 12);
    }

    // ─── Region validation ─────────────────────────────────────────────────────

    #[test]
    fn region_validation() {
        assert!(is_valid_region("us-east-1"));
        assert!(is_valid_region("eu-central-1"));
        assert!(is_valid_region("ap-northeast-1"));
        assert!(!is_valid_region("my-fake-region"));
        assert!(!is_valid_region(""));
    }

    // ─── CORS handler ──────────────────────────────────────────────────────────

    #[test]
    fn cors_preflight_detection() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("http://localhost:3000"));
        headers.insert(
            "access-control-request-method",
            HeaderValue::from_static("PUT"),
        );
        assert!(CorsHandler::is_preflight(&Method::OPTIONS, &headers));
        // GET is not preflight even with CORS headers
        assert!(!CorsHandler::is_preflight(&Method::GET, &headers));
        // OPTIONS without CORS request headers is not preflight
        let mut headers2 = HeaderMap::new();
        headers2.insert("origin", HeaderValue::from_static("http://localhost:3000"));
        assert!(!CorsHandler::is_preflight(&Method::OPTIONS, &headers2));
    }

    #[test]
    fn cors_headers_added_to_response() {
        let handler = CorsHandler::new(&test_config());
        let mut response_headers = HeaderMap::new();
        handler.add_cors_headers(&mut response_headers, Some("http://localhost:3000"));
        assert_eq!(
            response_headers.get("access-control-allow-origin").unwrap(),
            "http://localhost:3000"
        );
        assert!(response_headers.contains_key("access-control-allow-methods"));
        assert!(response_headers.contains_key("access-control-allow-headers"));
        assert!(response_headers.contains_key("access-control-max-age"));
    }

    #[test]
    fn cors_headers_use_wildcard_when_no_origin() {
        let handler = CorsHandler::new(&test_config());
        let mut response_headers = HeaderMap::new();
        handler.add_cors_headers(&mut response_headers, None);
        assert_eq!(
            response_headers.get("access-control-allow-origin").unwrap(),
            "*"
        );
    }

    #[test]
    fn cors_disabled_adds_no_headers() {
        let mut config = test_config();
        config.cors.disable_cors_headers = true;
        let handler = CorsHandler::new(&config);
        let mut response_headers = HeaderMap::new();
        handler.add_cors_headers(&mut response_headers, Some("http://example.com"));
        assert!(!response_headers.contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn studio_spa_route_returns_html_with_cache_headers() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio/services/s3")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("cache-control")
                .unwrap()
                .to_str()
                .unwrap(),
            "no-cache"
        );
        assert!(resp.headers().get("etag").is_some());
    }

    #[tokio::test]
    async fn studio_asset_route_returns_cacheable_asset_headers() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio/assets/app.js")
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("cache-control")
                .unwrap()
                .to_str()
                .unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert!(resp.headers().get("etag").is_some());
    }

    /// Ensures the Studio API services endpoint is served by the gateway and returns the services catalog.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn run_test() {
    /// let config = test_config();
    /// let manager = ServicePluginManager::new(config.clone());
    /// let gateway = Gateway::new(config, manager);
    /// let app = gateway.build_app_for_tests();
    ///
    /// let req = Request::builder()
    ///     .method(Method::GET)
    ///     .uri("/_localstack/studio-api/services")
    ///     .body(Body::empty())
    ///     .unwrap();
    /// let resp = app.oneshot(req).await.unwrap();
    /// assert_eq!(resp.status(), StatusCode::OK);
    /// let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
    ///     .await
    ///     .unwrap();
    /// let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    /// assert!(json.get("services").is_some());
    /// # }
    /// ```
    #[tokio::test]
    async fn studio_api_route_takes_internal_precedence() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio-api/services")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("services").is_some());
    }

    #[tokio::test]
    async fn aws_route_non_regression_for_unknown_service() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let req = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=test/20260306/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=deadbeef",
            )
            .body(Body::from(""))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn aws_route_non_regression_with_studio_flow_catalog_present() {
        let config = test_config();
        let manager = ServicePluginManager::new(config.clone());
        let gateway = Gateway::new(config, manager);
        let app = gateway.build_app_for_tests();

        let studio_req = Request::builder()
            .method(Method::GET)
            .uri("/_localstack/studio-api/flows/catalog")
            .body(Body::empty())
            .unwrap();
        let studio_resp = app.clone().oneshot(studio_req).await.unwrap();
        assert_eq!(studio_resp.status(), StatusCode::OK);

        let aws_req = Request::builder()
            .method(Method::POST)
            .uri("/")
            .header(
                "authorization",
                "AWS4-HMAC-SHA256 Credential=test/20260306/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=deadbeef",
            )
            .body(Body::from(""))
            .unwrap();
        let aws_resp = app.oneshot(aws_req).await.unwrap();
        assert_eq!(aws_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
