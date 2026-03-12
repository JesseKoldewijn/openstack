//! Tests for the service framework: lifecycle, lazy loading, concurrent access.

#[cfg(test)]
mod service_framework_tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use async_trait::async_trait;
    use openstack_config::Config;
    use openstack_service_framework::{
        lifecycle::ServiceState,
        traits::{DispatchError, DispatchResponse, RequestContext, ServiceProvider},
        ServiceContainer, ServicePluginManager,
    };

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
            directories: openstack_config::Directories::from_env(),
            body_spool_threshold_bytes: 1_048_576,
        }
    }

    fn test_ctx(service: &str, operation: &str) -> RequestContext {
        RequestContext::new(service, operation, "us-east-1", "000000000000")
    }

    /// A minimal service provider that counts starts and dispatches.
    struct EchoProvider {
        name: String,
        start_count: Arc<AtomicUsize>,
    }

    impl EchoProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                start_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        #[allow(dead_code)]
        fn starts(&self) -> usize {
            self.start_count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl ServiceProvider for EchoProvider {
        fn service_name(&self) -> &str {
            &self.name
        }

        async fn start(&self) -> Result<(), anyhow::Error> {
            self.start_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
            let body = serde_json::json!({
                "service": ctx.service,
                "operation": ctx.operation,
                "echo": "ok",
            });
            DispatchResponse::ok_json(body)
        }
    }

    /// A provider that always fails to start.
    struct FailingProvider;

    #[async_trait]
    impl ServiceProvider for FailingProvider {
        fn service_name(&self) -> &str {
            "failing"
        }
        async fn start(&self) -> Result<(), anyhow::Error> {
            Err(anyhow::anyhow!("intentional start failure"))
        }
        async fn dispatch(&self, _ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
            Ok(DispatchResponse::ok_xml("<ok/>".to_string()))
        }
    }

    // ─── ServiceContainer tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn container_starts_lazily() {
        let provider = EchoProvider::new("echo");
        let start_counter = provider.start_count.clone();
        let container = ServiceContainer::new(std::sync::Arc::new(provider));

        assert_eq!(start_counter.load(Ordering::Relaxed), 0);
        container.ensure_running().await.expect("should start");
        assert_eq!(start_counter.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn container_does_not_double_start() {
        let provider = EchoProvider::new("echo");
        let start_counter = provider.start_count.clone();
        let container = Arc::new(ServiceContainer::new(Arc::new(provider)));

        // Call ensure_running concurrently from 8 tasks
        let mut handles = Vec::new();
        for _ in 0..8 {
            let c = container.clone();
            handles.push(tokio::spawn(async move { c.ensure_running().await }));
        }
        for h in handles {
            h.await.unwrap().expect("all should succeed");
        }

        // start() must be called exactly once despite concurrent ensure_running
        assert_eq!(start_counter.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn container_transitions_through_lifecycle() {
        let provider = EchoProvider::new("echo");
        let container = ServiceContainer::new(Arc::new(provider));

        assert_eq!(container.current_state().await, ServiceState::Available);
        container.ensure_running().await.expect("start ok");
        assert_eq!(container.current_state().await, ServiceState::Running);
        container.stop().await.expect("stop ok");
        assert_eq!(container.current_state().await, ServiceState::Stopped);
    }

    #[tokio::test]
    async fn container_error_state_propagates() {
        let provider = FailingProvider;
        let container = ServiceContainer::new(Arc::new(provider));
        assert!(container.ensure_running().await.is_err());
        assert!(matches!(
            container.current_state().await,
            ServiceState::Error(_)
        ));
    }

    // ─── ServicePluginManager tests ────────────────────────────────────────────

    #[tokio::test]
    async fn manager_dispatches_to_registered_provider() {
        let manager = ServicePluginManager::new(test_config());
        manager.register("echo", EchoProvider::new("echo"));

        let ctx = test_ctx("echo", "TestOperation");
        let resp = manager.dispatch(&ctx).await.expect("dispatch ok");
        assert_eq!(resp.status_code, 200);
        let body: serde_json::Value =
            serde_json::from_slice(resp.body.as_bytes()).expect("valid json");
        assert_eq!(body["echo"], "ok");
        assert_eq!(body["operation"], "TestOperation");
    }

    #[tokio::test]
    async fn manager_returns_not_found_for_unknown_service() {
        let manager = ServicePluginManager::new(test_config());
        let ctx = test_ctx("nonexistent", "Foo");
        let result = manager.dispatch(&ctx).await;
        assert!(matches!(result, Err(DispatchError::ServiceNotFound(_))));
    }

    #[tokio::test]
    async fn manager_returns_unavailable_when_start_fails() {
        let manager = ServicePluginManager::new(test_config());
        manager.register("failing", FailingProvider);
        let ctx = test_ctx("failing", "Foo");
        let result = manager.dispatch(&ctx).await;
        assert!(matches!(result, Err(DispatchError::ServiceUnavailable(_))));
    }

    #[tokio::test]
    async fn manager_reports_service_states() {
        let manager = ServicePluginManager::new(test_config());
        manager.register("echo", EchoProvider::new("echo"));
        let ctx = test_ctx("echo", "Test");
        manager.dispatch(&ctx).await.expect("ok");

        let states = manager.service_states().await;
        assert!(!states.is_empty());
        let echo_state = states.iter().find(|(n, _)| n == "echo").unwrap();
        assert_eq!(echo_state.1, ServiceState::Running);
    }

    // ─── ServiceState tests ────────────────────────────────────────────────────

    #[test]
    fn service_state_as_str() {
        assert_eq!(ServiceState::Available.as_str(), "available");
        assert_eq!(ServiceState::Running.as_str(), "running");
        assert_eq!(ServiceState::Stopped.as_str(), "stopped");
        assert_eq!(ServiceState::Error("oops".to_string()).as_str(), "error");
    }

    // ─── DispatchResponse helpers ──────────────────────────────────────────────

    #[test]
    fn dispatch_response_ok_json() {
        let resp =
            DispatchResponse::ok_json(serde_json::json!({"foo": "bar"})).expect("serialize ok");
        assert_eq!(resp.status_code, 200);
        assert!(resp.content_type.contains("json"));
        let v: serde_json::Value = serde_json::from_slice(resp.body.as_bytes()).unwrap();
        assert_eq!(v["foo"], "bar");
    }

    #[test]
    fn dispatch_response_ok_xml() {
        let resp = DispatchResponse::ok_xml("<root/>".to_string());
        assert_eq!(resp.status_code, 200);
        assert!(resp.content_type.contains("xml"));
    }
}
