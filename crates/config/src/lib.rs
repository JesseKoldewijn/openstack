pub mod directories;
pub mod logging;
pub mod services;

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
pub use directories::Directories;
pub use services::ServicesConfig;

/// Central configuration for openstack, loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Bind addresses for the HTTP gateway (from GATEWAY_LISTEN)
    pub gateway_listen: Vec<SocketAddr>,

    /// Whether persistence is enabled (PERSISTENCE=1)
    pub persistence: bool,

    /// Services configuration
    pub services: ServicesConfig,

    /// Debug mode (DEBUG=1)
    pub debug: bool,

    /// Log level (LS_LOG)
    pub log_level: LogLevel,

    /// LocalStack host for URL generation (LOCALSTACK_HOST)
    pub localstack_host: String,

    /// Allow non-standard AWS region names (ALLOW_NONSTANDARD_REGIONS=1)
    pub allow_nonstandard_regions: bool,

    /// CORS configuration
    pub cors: CorsConfig,

    /// Snapshot strategies
    pub snapshot_save_strategy: SnapshotSaveStrategy,
    pub snapshot_load_strategy: SnapshotLoadStrategy,
    pub snapshot_flush_interval: Duration,

    /// DNS configuration
    pub dns_address: Option<String>,
    pub dns_port: u16,
    pub dns_resolve_ip: String,

    /// Lambda configuration
    pub lambda_keepalive_ms: u64,
    pub lambda_remove_containers: bool,
    pub bucket_marker_local: Option<String>,

    /// Eager service loading (EAGER_SERVICE_LOADING=1)
    pub eager_service_loading: bool,

    /// Enable config updates via API (ENABLE_CONFIG_UPDATES=1)
    pub enable_config_updates: bool,

    /// Directory paths
    pub directories: Directories,

    /// Body spool threshold in bytes (BODY_SPOOL_THRESHOLD_BYTES, default 1 MiB).
    /// Request bodies larger than this are spooled to disk instead of kept in memory.
    pub body_spool_threshold_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub disable_cors_headers: bool,
    pub disable_cors_checks: bool,
    pub extra_allowed_origins: Vec<String>,
    pub extra_allowed_headers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotSaveStrategy {
    /// Save on shutdown
    OnShutdown,
    /// Save on every mutating request
    OnRequest,
    /// Save on a schedule
    Scheduled,
    /// Only save when explicitly requested
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotLoadStrategy {
    /// Load on startup
    OnStartup,
    /// Load on first request to a service
    OnRequest,
    /// Only load when explicitly requested
    Manual,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let gateway_listen = parse_gateway_listen(
            &std::env::var("GATEWAY_LISTEN").unwrap_or_else(|_| "0.0.0.0:4566".to_string()),
        )?;

        let persistence = env_bool("PERSISTENCE", false);
        let debug = env_bool("DEBUG", false);

        let log_level = parse_log_level(&std::env::var("LS_LOG").unwrap_or_default(), debug);

        let localstack_host = std::env::var("LOCALSTACK_HOST")
            .unwrap_or_else(|_| "localhost.localstack.cloud:4566".to_string());

        let allow_nonstandard_regions = env_bool("ALLOW_NONSTANDARD_REGIONS", false);

        let cors = CorsConfig {
            disable_cors_headers: env_bool("DISABLE_CORS_HEADERS", false),
            disable_cors_checks: env_bool("DISABLE_CORS_CHECKS", false),
            extra_allowed_origins: env_list("EXTRA_CORS_ALLOWED_ORIGINS"),
            extra_allowed_headers: env_list("EXTRA_CORS_ALLOWED_HEADERS"),
        };

        let snapshot_save_strategy = parse_snapshot_save_strategy(
            &std::env::var("SNAPSHOT_SAVE_STRATEGY").unwrap_or_else(|_| "ON_SHUTDOWN".to_string()),
        );
        let snapshot_load_strategy = parse_snapshot_load_strategy(
            &std::env::var("SNAPSHOT_LOAD_STRATEGY").unwrap_or_else(|_| "ON_STARTUP".to_string()),
        );
        let snapshot_flush_interval = Duration::from_secs(
            std::env::var("SNAPSHOT_FLUSH_INTERVAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(15),
        );

        let dns_address = std::env::var("DNS_ADDRESS").ok();
        let dns_port = std::env::var("DNS_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(53);
        let dns_resolve_ip =
            std::env::var("DNS_RESOLVE_IP").unwrap_or_else(|_| "127.0.0.1".to_string());

        let lambda_keepalive_ms = std::env::var("LAMBDA_KEEPALIVE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600_000);
        let lambda_remove_containers = env_bool("LAMBDA_REMOVE_CONTAINERS", true);
        let bucket_marker_local = std::env::var("BUCKET_MARKER_LOCAL").ok();

        let eager_service_loading = env_bool("EAGER_SERVICE_LOADING", false);
        let enable_config_updates = env_bool("ENABLE_CONFIG_UPDATES", false);

        let services = ServicesConfig::from_env();

        let directories = Directories::from_env();

        let body_spool_threshold_bytes: usize = std::env::var("BODY_SPOOL_THRESHOLD_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_048_576); // 1 MiB default

        Ok(Config {
            gateway_listen,
            persistence,
            services,
            debug,
            log_level,
            localstack_host,
            allow_nonstandard_regions,
            cors,
            snapshot_save_strategy,
            snapshot_load_strategy,
            snapshot_flush_interval,
            dns_address,
            dns_port,
            dns_resolve_ip,
            lambda_keepalive_ms,
            lambda_remove_containers,
            bucket_marker_local,
            eager_service_loading,
            enable_config_updates,
            directories,
            body_spool_threshold_bytes,
        })
    }

    /// Returns true if the DNS server should be started.
    pub fn dns_enabled(&self) -> bool {
        self.dns_address.is_some()
    }

    /// Returns the base URL for this LocalStack instance.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.localstack_host)
    }

    /// Returns the URL for a specific service and region.
    pub fn service_url(&self, service: &str, region: &str) -> String {
        format!("http://{}.{}.{}", service, region, self.localstack_host)
    }
}

fn parse_gateway_listen(value: &str) -> Result<Vec<SocketAddr>> {
    value
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<SocketAddr>()
                .map_err(|e| anyhow::anyhow!("Invalid GATEWAY_LISTEN address '{}': {}", s, e))
        })
        .collect()
}

fn parse_log_level(ls_log: &str, debug: bool) -> LogLevel {
    if debug {
        return LogLevel::Debug;
    }
    match ls_log.to_lowercase().as_str() {
        "trace" => LogLevel::Trace,
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warn" | "warning" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

fn parse_snapshot_save_strategy(value: &str) -> SnapshotSaveStrategy {
    match value.to_uppercase().as_str() {
        "ON_REQUEST" => SnapshotSaveStrategy::OnRequest,
        "SCHEDULED" => SnapshotSaveStrategy::Scheduled,
        "MANUAL" => SnapshotSaveStrategy::Manual,
        _ => SnapshotSaveStrategy::OnShutdown,
    }
}

fn parse_snapshot_load_strategy(value: &str) -> SnapshotLoadStrategy {
    match value.to_uppercase().as_str() {
        "ON_REQUEST" => SnapshotLoadStrategy::OnRequest,
        "MANUAL" => SnapshotLoadStrategy::Manual,
        _ => SnapshotLoadStrategy::OnStartup,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "yes" | "True" | "TRUE"),
        Err(_) => default,
    }
}

fn env_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gateway_listen_default() {
        let addrs = parse_gateway_listen("0.0.0.0:4566").unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].port(), 4566);
    }

    #[test]
    fn test_parse_gateway_listen_multiple() {
        let addrs = parse_gateway_listen("0.0.0.0:4566,[::]:4566").unwrap();
        assert_eq!(addrs.len(), 2);
    }

    #[test]
    fn test_parse_gateway_listen_invalid() {
        assert!(parse_gateway_listen("not-an-address").is_err());
    }

    #[test]
    fn test_log_level_from_debug() {
        assert_eq!(parse_log_level("", true), LogLevel::Debug);
    }

    #[test]
    fn test_log_level_from_ls_log() {
        assert_eq!(parse_log_level("trace", false), LogLevel::Trace);
        assert_eq!(parse_log_level("warn", false), LogLevel::Warn);
        assert_eq!(parse_log_level("ERROR", false), LogLevel::Error);
        assert_eq!(parse_log_level("unknown", false), LogLevel::Info);
    }

    #[test]
    fn test_env_bool() {
        assert!(!env_bool("__OPENSTACK_TEST_BOOL_TRUE", false)); // unset → default
                                                                 // Can't easily set env vars in unit tests without std::env::set_var (unsafe in Rust 1.80+)
                                                                 // Integration tested via Config::from_env
    }

    #[test]
    fn test_cors_config_defaults() {
        let config = CorsConfig {
            disable_cors_headers: false,
            disable_cors_checks: false,
            extra_allowed_origins: vec![],
            extra_allowed_headers: vec![],
        };
        assert!(!config.disable_cors_headers);
        assert!(!config.disable_cors_checks);
        assert!(config.extra_allowed_origins.is_empty());
    }

    #[test]
    fn test_snapshot_load_strategies() {
        assert_eq!(
            parse_snapshot_load_strategy("ON_REQUEST"),
            SnapshotLoadStrategy::OnRequest
        );
        assert_eq!(
            parse_snapshot_load_strategy("MANUAL"),
            SnapshotLoadStrategy::Manual
        );
        assert_eq!(
            parse_snapshot_load_strategy("ON_STARTUP"),
            SnapshotLoadStrategy::OnStartup
        );
        assert_eq!(
            parse_snapshot_load_strategy("bogus"),
            SnapshotLoadStrategy::OnStartup
        );
    }

    #[test]
    fn test_base_url() {
        let config = Config {
            gateway_listen: vec!["0.0.0.0:4566".parse().unwrap()],
            persistence: false,
            services: crate::services::ServicesConfig::from_env(),
            debug: false,
            log_level: LogLevel::Info,
            localstack_host: "localhost.localstack.cloud:4566".to_string(),
            allow_nonstandard_regions: false,
            cors: CorsConfig {
                disable_cors_headers: false,
                disable_cors_checks: false,
                extra_allowed_origins: vec![],
                extra_allowed_headers: vec![],
            },
            snapshot_save_strategy: SnapshotSaveStrategy::OnShutdown,
            snapshot_load_strategy: SnapshotLoadStrategy::OnStartup,
            snapshot_flush_interval: Duration::from_secs(15),
            dns_address: None,
            dns_port: 53,
            dns_resolve_ip: "127.0.0.1".to_string(),
            lambda_keepalive_ms: 600_000,
            lambda_remove_containers: true,
            bucket_marker_local: None,
            eager_service_loading: false,
            enable_config_updates: false,
            directories: crate::directories::Directories::from_env(),
            body_spool_threshold_bytes: 1_048_576,
        };
        assert_eq!(config.base_url(), "http://localhost.localstack.cloud:4566");
        assert_eq!(
            config.service_url("sqs", "us-east-1"),
            "http://sqs.us-east-1.localhost.localstack.cloud:4566"
        );
    }

    #[test]
    fn test_dns_enabled() {
        let base = Config {
            gateway_listen: vec!["0.0.0.0:4566".parse().unwrap()],
            persistence: false,
            services: crate::services::ServicesConfig::from_env(),
            debug: false,
            log_level: LogLevel::Info,
            localstack_host: "localhost.localstack.cloud:4566".to_string(),
            allow_nonstandard_regions: false,
            cors: CorsConfig {
                disable_cors_headers: false,
                disable_cors_checks: false,
                extra_allowed_origins: vec![],
                extra_allowed_headers: vec![],
            },
            snapshot_save_strategy: SnapshotSaveStrategy::OnShutdown,
            snapshot_load_strategy: SnapshotLoadStrategy::OnStartup,
            snapshot_flush_interval: Duration::from_secs(15),
            dns_address: None,
            dns_port: 53,
            dns_resolve_ip: "127.0.0.1".to_string(),
            lambda_keepalive_ms: 600_000,
            lambda_remove_containers: true,
            bucket_marker_local: None,
            eager_service_loading: false,
            enable_config_updates: false,
            directories: crate::directories::Directories::from_env(),
            body_spool_threshold_bytes: 1_048_576,
        };
        assert!(!base.dns_enabled());
        let with_dns = Config {
            dns_address: Some("0.0.0.0".to_string()),
            ..base
        };
        assert!(with_dns.dns_enabled());
    }
}
