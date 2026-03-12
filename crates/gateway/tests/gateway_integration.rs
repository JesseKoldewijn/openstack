//! Integration tests for the gateway handler pipeline.

#[cfg(test)]
mod gateway_tests {
    use axum::http::{HeaderMap, HeaderValue, Method};
    use openstack_config::{
        Config, CorsConfig, Directories, LogLevel, ServicesConfig, SnapshotLoadStrategy,
        SnapshotSaveStrategy,
    };
    use openstack_gateway::cors::CorsHandler;
    use openstack_gateway::sigv4::{
        access_key_to_account_id, is_valid_region, parse_sigv4_auth, DEFAULT_ACCOUNT_ID,
    };

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
}
