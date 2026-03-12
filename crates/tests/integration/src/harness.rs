//! Test harness: starts the openstack server on a random port and returns a
//! client pre-configured to speak to it.
//!
//! # Example
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() {
//! use openstack_integration_tests::harness::TestHarness;
//! let harness = TestHarness::start().await;
//! let resp = harness.client
//!     .get(harness.url("/_localstack/health"))
//!     .send().await.unwrap();
//! assert!(resp.status().is_success());
//! harness.shutdown();
//! # }
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use openstack_config::{
    Config, CorsConfig, Directories, LogLevel, ServicesConfig, SnapshotLoadStrategy,
    SnapshotSaveStrategy,
};
use openstack_gateway::Gateway;
use openstack_service_framework::ServicePluginManager;
use openstack_state::StateManager;

/// A running openstack server for use in integration tests.
pub struct TestHarness {
    /// Base URL, e.g. `http://127.0.0.1:12345`
    pub base_url: String,
    /// HTTP client pre-configured for AWS-like requests.
    pub client: reqwest::Client,
    /// Send on this to stop the server background task.
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    /// Keep the temp directory alive for the duration of the test.
    _temp_dir: tempfile::TempDir,
}

impl TestHarness {
    /// Start a server on a random free port with all services enabled.
    pub async fn start() -> Self {
        Self::start_with_config(None).await
    }

    /// Start a server with only the specified services enabled (comma-separated).
    pub async fn start_services(services: &str) -> Self {
        let svc_config = ServicesConfig::only(
            services
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty()),
        );
        Self::start_with_config(Some(svc_config)).await
    }

    async fn start_with_config(svc_config: Option<ServicesConfig>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind random port");
        let addr: SocketAddr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let services = svc_config.unwrap_or_else(ServicesConfig::all);

        let temp_dir = tempfile::tempdir().expect("create temp dir for test harness");
        let config = test_config(addr, services, temp_dir.path());
        let plugin_manager = ServicePluginManager::new(config.clone());
        register_all_services(&plugin_manager, &config);

        let state_manager = StateManager::new(config.clone());
        // Skip disk I/O in tests
        let _ = state_manager.load_on_startup().await;

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let gateway = Gateway::new(config.clone(), plugin_manager.clone());
        tokio::spawn(async move {
            gateway.run_with_listener(listener, shutdown_rx).await.ok();
        });

        // Wait for the server to become ready
        wait_for_ready(&base_url, Duration::from_secs(5)).await;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        Self {
            base_url,
            client,
            shutdown_tx,
            _temp_dir: temp_dir,
        }
    }

    /// Return the full URL for the given path.
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Helpers that inject a fake SigV4 Authorization header.
    pub fn aws_get(&self, path: &str, service: &str, region: &str) -> reqwest::RequestBuilder {
        self.client
            .get(self.url(path))
            .header("authorization", fake_auth(service, region))
            .header("x-amz-date", "20260306T000000Z")
    }

    pub fn aws_post(&self, path: &str, service: &str, region: &str) -> reqwest::RequestBuilder {
        self.client
            .post(self.url(path))
            .header("authorization", fake_auth(service, region))
            .header("x-amz-date", "20260306T000000Z")
    }

    pub fn aws_put(&self, path: &str, service: &str, region: &str) -> reqwest::RequestBuilder {
        self.client
            .put(self.url(path))
            .header("authorization", fake_auth(service, region))
            .header("x-amz-date", "20260306T000000Z")
    }

    pub fn aws_delete(&self, path: &str, service: &str, region: &str) -> reqwest::RequestBuilder {
        self.client
            .delete(self.url(path))
            .header("authorization", fake_auth(service, region))
            .header("x-amz-date", "20260306T000000Z")
    }

    /// Stop the server.
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn fake_auth(service: &str, region: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=test/20260306/{region}/{service}/aws4_request, \
         SignedHeaders=host;x-amz-date, \
         Signature=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )
}

fn test_config(addr: SocketAddr, services: ServicesConfig, data_dir: &std::path::Path) -> Config {
    Config {
        gateway_listen: vec![addr],
        persistence: false,
        services,
        debug: false,
        log_level: LogLevel::Warn,
        localstack_host: format!("{}:{}", addr.ip(), addr.port()),
        allow_nonstandard_regions: false,
        cors: CorsConfig {
            disable_cors_headers: false,
            disable_cors_checks: false,
            extra_allowed_origins: vec![],
            extra_allowed_headers: vec![],
        },
        snapshot_save_strategy: SnapshotSaveStrategy::Manual,
        snapshot_load_strategy: SnapshotLoadStrategy::Manual,
        snapshot_flush_interval: Duration::from_secs(3600),
        dns_address: None,
        dns_port: 53,
        dns_resolve_ip: "127.0.0.1".to_string(),
        lambda_keepalive_ms: 0,
        lambda_remove_containers: true,
        bucket_marker_local: None,
        eager_service_loading: false,
        enable_config_updates: false,
        directories: Directories::from_root(data_dir),
        body_spool_threshold_bytes: 1_048_576,
    }
}

fn register_all_services(manager: &ServicePluginManager, config: &Config) {
    let s = &config.services;
    macro_rules! reg {
        ($name:literal, $provider:expr) => {
            if s.is_enabled($name) {
                manager.register($name, $provider);
            }
        };
    }
    reg!(
        "s3",
        openstack_s3::S3Provider::new(&config.directories.s3_objects)
    );
    reg!("sqs", openstack_sqs::SqsProvider::new());
    reg!("sns", openstack_sns::SnsProvider::new());
    reg!("dynamodb", openstack_dynamodb::DynamoDbProvider::new());
    reg!("iam", openstack_iam::IamProvider::new());
    reg!("sts", openstack_sts::StsProvider::new());
    reg!("kms", openstack_kms::KmsProvider::new());
    reg!(
        "secretsmanager",
        openstack_secretsmanager::SecretsManagerProvider::new()
    );
    reg!("ssm", openstack_ssm::SsmProvider::new());
    reg!("acm", openstack_acm::AcmProvider::new());
    reg!("kinesis", openstack_kinesis::KinesisProvider::new());
    reg!("firehose", openstack_firehose::FirehoseProvider::new());
    reg!(
        "cloudwatch",
        openstack_cloudwatch::CloudWatchProvider::new()
    );
    reg!("events", openstack_eventbridge::EventBridgeProvider::new());
    reg!(
        "states",
        openstack_stepfunctions::StepFunctionsProvider::new()
    );
    reg!(
        "apigateway",
        openstack_apigateway::ApiGatewayProvider::new()
    );
    reg!("ec2", openstack_ec2::Ec2Provider::new());
    reg!("route53", openstack_route53::Route53Provider::new());
    reg!("ses", openstack_ses::SesProvider::new());
    reg!("ecr", openstack_ecr::EcrProvider::new());
    reg!(
        "opensearch",
        openstack_opensearch::OpenSearchProvider::new()
    );
    reg!("redshift", openstack_redshift::RedshiftProvider::new());
    reg!(
        "cloudformation",
        openstack_cloudformation::CloudFormationProvider::new()
    );
    reg!("lambda", openstack_lambda::LambdaProvider::new());
}

async fn wait_for_ready(base_url: &str, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    let health_url = format!("{base_url}/_localstack/health");
    loop {
        if std::time::Instant::now() > deadline {
            panic!("Server at {base_url} did not become ready within {timeout:?}");
        }
        if let Ok(resp) = reqwest::get(&health_url).await
            && resp.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
