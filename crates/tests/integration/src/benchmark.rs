use std::collections::{BTreeMap, HashMap};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::classification::{
    PersistenceMode, ServiceDurabilityClass, ServiceExecutionClass, class_envelope,
    parse_persistence_mode, service_durability_class, service_execution_class,
};
use crate::harness::TestHarness;
use crate::parity::{ProtocolFamily, ScenarioStep};

const CORE_PARITY_SERVICES: &[&str] = &[
    "dynamodb",
    "firehose",
    "iam",
    "kinesis",
    "s3",
    "secretsmanager",
    "sns",
    "sts",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkRuntimeMode {
    Asymmetric,
    SymmetricDocker,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionOrderPolicy {
    OpenstackFirst,
    LocalstackFirst,
    Alternating,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkScenarioClass {
    Coverage,
    #[default]
    Performance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkLoadTier {
    #[default]
    Medium,
    Low,
    High,
    Extreme,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkScenarioRole {
    Write,
    Read,
    Admin,
    #[default]
    Aux,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerRuntimeConstraints {
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
    pub network_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRuntimeMetadata {
    pub mode: BenchmarkRuntimeMode,
    pub execution_order_policy: ExecutionOrderPolicy,
    pub execution_driver: BenchmarkExecutionDriver,
    pub benchmark_lane_mode: BenchmarkLaneMode,
    pub openstack_persistence_mode: PersistenceMode,
    pub localstack_persistence_mode: PersistenceMode,
    pub persistence_mode_equivalent: bool,
    pub docker_constraints: Option<DockerRuntimeConstraints>,
    pub diagnostics_only_role_coverage: bool,
    pub strict_role_coverage_gate: bool,
    pub non_blocking_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStartupSample {
    pub target: String,
    pub startup_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkStartupSummary {
    pub openstack_avg_ms: Option<f64>,
    pub localstack_avg_ms: Option<f64>,
    pub startup_ratio_openstack_over_localstack: Option<f64>,
    pub samples: Vec<BenchmarkStartupSample>,
    pub missing_targets: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkLaneMode {
    HarnessInfluenced,
    LowOverhead,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkExecutionDriver {
    AwsCli,
    DirectHttp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub localstack_image: String,
    pub openstack_image: String,
    pub report_dir: PathBuf,
    pub profiles: HashMap<String, BenchmarkProfile>,
    pub openstack_endpoint: Option<String>,
    pub localstack_endpoint: Option<String>,
    pub runtime_mode: BenchmarkRuntimeMode,
    pub execution_order_policy: ExecutionOrderPolicy,
    pub docker_cpu_limit: Option<String>,
    pub docker_memory_limit: Option<String>,
    pub docker_network_mode: String,
    pub docker_startup_timeout_secs: u64,
    pub heavy_object_enabled: bool,
    pub lane_mode: BenchmarkLaneMode,
    pub execution_driver: BenchmarkExecutionDriver,
    pub openstack_persistence_mode: PersistenceMode,
    pub localstack_persistence_mode: PersistenceMode,
    pub diagnostics_only_role_coverage: bool,
    pub strict_role_coverage_gate: bool,
    pub startup_samples: usize,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert(
            "all-services-realistic".to_string(),
            BenchmarkProfile {
                name: "all-services-realistic".to_string(),
                warmup_iterations: 1,
                measured_iterations: 2,
                operations_per_iteration: 4,
                concurrency: 1,
                services: all_service_names(),
            },
        );
        profiles.insert(
            "all-services-smoke".to_string(),
            BenchmarkProfile {
                name: "all-services-smoke".to_string(),
                warmup_iterations: 1,
                measured_iterations: 3,
                operations_per_iteration: 10,
                concurrency: 2,
                services: all_service_names(),
            },
        );
        profiles.insert(
            "all-services-smoke-fast".to_string(),
            BenchmarkProfile {
                name: "all-services-smoke-fast".to_string(),
                warmup_iterations: 1,
                measured_iterations: 2,
                operations_per_iteration: 6,
                concurrency: 1,
                services: vec![
                    "s3".into(),
                    "sqs".into(),
                    "sns".into(),
                    "dynamodb".into(),
                    "kms".into(),
                    "ssm".into(),
                    "kinesis".into(),
                    "sts".into(),
                ],
            },
        );
        profiles.insert(
            "hot-path-deep".to_string(),
            BenchmarkProfile {
                name: "hot-path-deep".to_string(),
                warmup_iterations: 2,
                measured_iterations: 5,
                operations_per_iteration: 25,
                concurrency: 5,
                services: vec![
                    "s3".into(),
                    "sqs".into(),
                    "dynamodb".into(),
                    "lambda".into(),
                    "kinesis".into(),
                    "opensearch".into(),
                    "cloudwatch".into(),
                ],
            },
        );
        profiles.insert(
            "fair-low".to_string(),
            BenchmarkProfile {
                name: "fair-low".to_string(),
                warmup_iterations: 1,
                measured_iterations: 2,
                operations_per_iteration: 4,
                concurrency: 1,
                services: all_service_names(),
            },
        );
        profiles.insert(
            "fair-low-core".to_string(),
            BenchmarkProfile {
                name: "fair-low-core".to_string(),
                warmup_iterations: 1,
                measured_iterations: 2,
                operations_per_iteration: 6,
                concurrency: 1,
                services: CORE_PARITY_SERVICES.iter().map(|s| s.to_string()).collect(),
            },
        );
        profiles.insert(
            "fair-medium".to_string(),
            BenchmarkProfile {
                name: "fair-medium".to_string(),
                warmup_iterations: 1,
                measured_iterations: 3,
                operations_per_iteration: 10,
                concurrency: 2,
                services: all_service_names(),
            },
        );
        profiles.insert(
            "fair-medium-core".to_string(),
            BenchmarkProfile {
                name: "fair-medium-core".to_string(),
                warmup_iterations: 1,
                measured_iterations: 4,
                operations_per_iteration: 16,
                concurrency: 2,
                services: CORE_PARITY_SERVICES.iter().map(|s| s.to_string()).collect(),
            },
        );
        profiles.insert(
            "fair-high".to_string(),
            BenchmarkProfile {
                name: "fair-high".to_string(),
                warmup_iterations: 2,
                measured_iterations: 6,
                operations_per_iteration: 40,
                concurrency: 8,
                services: vec![
                    "s3".into(),
                    "sqs".into(),
                    "dynamodb".into(),
                    "lambda".into(),
                    "kinesis".into(),
                    "opensearch".into(),
                    "cloudwatch".into(),
                ],
            },
        );
        profiles.insert(
            "fair-extreme".to_string(),
            BenchmarkProfile {
                name: "fair-extreme".to_string(),
                warmup_iterations: 0,
                measured_iterations: 1,
                operations_per_iteration: 1,
                concurrency: 1,
                services: vec!["s3".into()],
            },
        );

        Self {
            localstack_image: std::env::var("PARITY_LOCALSTACK_IMAGE")
                .unwrap_or_else(|_| "localstack/localstack:3.7.2".to_string()),
            openstack_image: std::env::var("PARITY_OPENSTACK_IMAGE")
                .unwrap_or_else(|_| "ghcr.io/jessekoldewijn/openstack:latest".to_string()),
            report_dir: PathBuf::from("target/benchmark-reports"),
            profiles,
            openstack_endpoint: std::env::var("PARITY_OPENSTACK_ENDPOINT").ok(),
            localstack_endpoint: std::env::var("PARITY_LOCALSTACK_ENDPOINT").ok(),
            runtime_mode: benchmark_runtime_mode_from_env(),
            execution_order_policy: execution_order_policy_from_env(),
            docker_cpu_limit: std::env::var("PARITY_DOCKER_CPU_LIMIT").ok(),
            docker_memory_limit: std::env::var("PARITY_DOCKER_MEMORY_LIMIT").ok(),
            docker_network_mode: std::env::var("PARITY_DOCKER_NETWORK_MODE")
                .unwrap_or_else(|_| "bridge".to_string()),
            docker_startup_timeout_secs: std::env::var("PARITY_DOCKER_STARTUP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(120),
            heavy_object_enabled: std::env::var("BENCHMARK_HEAVY_OBJECTS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            lane_mode: benchmark_lane_mode_from_env(),
            execution_driver: benchmark_execution_driver_from_env(),
            openstack_persistence_mode: std::env::var("PARITY_OPENSTACK_PERSISTENCE_MODE")
                .ok()
                .and_then(|v| parse_persistence_mode(&v))
                .unwrap_or(PersistenceMode::NonDurable),
            localstack_persistence_mode: std::env::var("PARITY_LOCALSTACK_PERSISTENCE_MODE")
                .ok()
                .and_then(|v| parse_persistence_mode(&v))
                .unwrap_or(PersistenceMode::NonDurable),
            diagnostics_only_role_coverage: std::env::var(
                "PARITY_BENCHMARK_ROLE_COVERAGE_DIAGNOSTICS_ONLY",
            )
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
            strict_role_coverage_gate: std::env::var("PARITY_BENCHMARK_ROLE_COVERAGE_STRICT")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            startup_samples: std::env::var("PARITY_BENCHMARK_STARTUP_SAMPLES")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(3)
                .max(1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkProfile {
    pub name: String,
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub operations_per_iteration: usize,
    pub concurrency: usize,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkScenario {
    pub id: String,
    pub profile: String,
    pub service: String,
    #[serde(default)]
    pub scenario_class: BenchmarkScenarioClass,
    #[serde(default)]
    pub load_tier: BenchmarkLoadTier,
    #[serde(default)]
    pub scenario_role: BenchmarkScenarioRole,
    pub protocol: ProtocolFamily,
    pub setup: Vec<ScenarioStep>,
    pub operation: ScenarioStep,
    pub cleanup: Vec<ScenarioStep>,
    pub warmup_iterations: Option<usize>,
    pub measured_iterations: Option<usize>,
    pub operations_per_iteration: Option<usize>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRunConfig {
    pub warmup_iterations: usize,
    pub measured_iterations: usize,
    pub operations_per_iteration: usize,
    pub concurrency: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTargetMetadata {
    pub endpoint: String,
    pub runtime: String,
    pub image: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
    pub network_mode: Option<String>,
    pub localstack_image: Option<String>,
    pub localstack_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchmarkMetrics {
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_min_ms: f64,
    pub latency_max_ms: f64,
    pub latency_stddev_ms: f64,
    pub throughput_ops_per_sec: f64,
    pub operation_count: usize,
    pub error_count: usize,
    pub success_rate: f64,
    pub timeout_count: usize,
    pub retry_count: usize,
    pub total_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTargetResult {
    pub metadata: BenchmarkTargetMetadata,
    pub metrics: BenchmarkMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub latency_p50_ratio: Option<f64>,
    pub latency_p95_ratio: Option<f64>,
    pub throughput_ratio: Option<f64>,
    pub latency_p50_delta_ms: f64,
    pub latency_p95_delta_ms: f64,
    pub throughput_delta_ops_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkScenarioResult {
    pub scenario_id: String,
    pub service: String,
    pub service_execution_class: Option<ServiceExecutionClass>,
    pub service_durability_class: Option<ServiceDurabilityClass>,
    pub scenario_class: BenchmarkScenarioClass,
    pub load_tier: BenchmarkLoadTier,
    pub scenario_role: BenchmarkScenarioRole,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub valid_for_performance: bool,
    pub invalid_reason: Option<String>,
    pub run_config: BenchmarkRunConfig,
    pub openstack: BenchmarkTargetResult,
    pub localstack: BenchmarkTargetResult,
    pub comparison: BenchmarkComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub total_scenarios: usize,
    pub performance_scenarios: usize,
    pub valid_performance_scenarios: usize,
    pub invalid_performance_scenarios: usize,
    pub coverage_scenarios: usize,
    pub skipped_scenarios: usize,
    pub lane_interpretable: bool,
    pub invalid_reasons: Vec<String>,
    pub openstack_error_count: usize,
    pub localstack_error_count: usize,
    pub avg_latency_p50_ratio: Option<f64>,
    pub avg_latency_p95_ratio: Option<f64>,
    pub avg_latency_p99_ratio: Option<f64>,
    pub avg_throughput_ratio: Option<f64>,
    pub missing_required_role_count: usize,
    pub per_service: BTreeMap<String, BenchmarkServiceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkServiceSummary {
    pub service_execution_class: Option<ServiceExecutionClass>,
    pub service_durability_class: Option<ServiceDurabilityClass>,
    pub total_scenarios: usize,
    pub skipped_scenarios: usize,
    pub openstack_error_count: usize,
    pub localstack_error_count: usize,
    pub avg_latency_p50_ratio: Option<f64>,
    pub avg_latency_p95_ratio: Option<f64>,
    pub avg_latency_p99_ratio: Option<f64>,
    pub avg_throughput_ratio: Option<f64>,
    pub required_roles: Vec<BenchmarkScenarioRole>,
    pub covered_roles: Vec<BenchmarkScenarioRole>,
    pub missing_roles: Vec<BenchmarkScenarioRole>,
    pub role_exclusions: BTreeMap<String, String>,
    pub class_envelope_breaches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub profile: String,
    pub run_id: String,
    pub generated_at: String,
    pub runtime: BenchmarkRuntimeMetadata,
    pub startup_summary: Option<BenchmarkStartupSummary>,
    pub openstack_target: BenchmarkTargetMetadata,
    pub localstack_target: BenchmarkTargetMetadata,
    pub memory_summary: Option<BenchmarkMemorySummary>,
    pub results: Vec<BenchmarkScenarioResult>,
    pub summary: BenchmarkSummary,
}

#[derive(Debug)]
struct BenchmarkManagedTarget {
    endpoint: String,
    runtime: String,
    image: Option<String>,
    cpu_limit: Option<String>,
    memory_limit: Option<String>,
    network_mode: Option<String>,
}

struct BenchmarkTargetManager {
    openstack: BenchmarkManagedTarget,
    localstack: BenchmarkManagedTarget,
    openstack_container_id: Option<String>,
    localstack_container_id: Option<String>,
    openstack_harness: Option<TestHarness>,
    idle_memory_summary: Option<BenchmarkMemorySummary>,
}

#[derive(Debug, Clone)]
struct ServiceWorkloadMatrixEntry {
    required_roles: Vec<BenchmarkScenarioRole>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMemorySample {
    pub target: String,
    pub rss_bytes: Option<u64>,
    pub raw_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMemorySummary {
    pub openstack_idle_rss_bytes: Option<u64>,
    pub localstack_idle_rss_bytes: Option<u64>,
    pub openstack_rss_bytes: Option<u64>,
    pub localstack_rss_bytes: Option<u64>,
    pub rss_ratio_openstack_over_localstack: Option<f64>,
    pub missing_targets: Vec<String>,
    pub samples: Vec<BenchmarkMemorySample>,
}

impl BenchmarkTargetManager {
    async fn start(config: &BenchmarkConfig, services: &[String]) -> anyhow::Result<Self> {
        match config.runtime_mode {
            BenchmarkRuntimeMode::Asymmetric => Self::start_asymmetric(config, services).await,
            BenchmarkRuntimeMode::SymmetricDocker => {
                Self::start_symmetric_docker(config, services).await
            }
        }
    }

    async fn start_asymmetric(
        config: &BenchmarkConfig,
        services: &[String],
    ) -> anyhow::Result<Self> {
        let services_csv = services.join(",");

        let (openstack, openstack_harness) = if let Some(endpoint) = &config.openstack_endpoint {
            (
                BenchmarkManagedTarget {
                    endpoint: endpoint.clone(),
                    runtime: "external".to_string(),
                    image: None,
                    cpu_limit: None,
                    memory_limit: None,
                    network_mode: None,
                },
                None,
            )
        } else {
            let harness = TestHarness::start_services(&services_csv).await;
            (
                BenchmarkManagedTarget {
                    endpoint: harness.base_url.clone(),
                    runtime: "in-process".to_string(),
                    image: None,
                    cpu_limit: None,
                    memory_limit: None,
                    network_mode: None,
                },
                Some(harness),
            )
        };

        let (localstack, localstack_container_id) =
            if let Some(endpoint) = &config.localstack_endpoint {
                (
                    BenchmarkManagedTarget {
                        endpoint: endpoint.clone(),
                        runtime: "external".to_string(),
                        image: Some(config.localstack_image.clone()),
                        cpu_limit: None,
                        memory_limit: None,
                        network_mode: None,
                    },
                    None,
                )
            } else {
                let port = free_port()?;
                let endpoint = format!("http://127.0.0.1:{port}");
                let localstack_services = services
                    .iter()
                    .map(|service| map_service_for_localstack(service))
                    .collect::<Vec<_>>()
                    .join(",");
                let output = Command::new("docker")
                    .args([
                        "run",
                        "-d",
                        "-p",
                        &format!("127.0.0.1:{port}:4566"),
                        "-e",
                        &format!("SERVICES={localstack_services}"),
                        "-e",
                        "DEBUG=1",
                        &config.localstack_image,
                    ])
                    .output()?;
                if !output.status.success() {
                    return Err(anyhow::anyhow!(
                        "failed to start localstack: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                let container_id = container_id_from_output(&output);
                wait_for_health(
                    &endpoint,
                    Duration::from_secs(config.docker_startup_timeout_secs),
                    "localstack",
                    Some(&container_id),
                )
                .await?;
                (
                    BenchmarkManagedTarget {
                        endpoint,
                        runtime: "docker".to_string(),
                        image: Some(config.localstack_image.clone()),
                        cpu_limit: None,
                        memory_limit: None,
                        network_mode: None,
                    },
                    Some(container_id),
                )
            };

        Ok(Self {
            openstack,
            localstack,
            openstack_container_id: None,
            localstack_container_id,
            openstack_harness,
            idle_memory_summary: None,
        })
    }

    async fn start_symmetric_docker(
        config: &BenchmarkConfig,
        services: &[String],
    ) -> anyhow::Result<Self> {
        if config.openstack_endpoint.is_some() || config.localstack_endpoint.is_some() {
            return Err(anyhow::anyhow!(
                "symmetric-docker mode requires managed targets; unset PARITY_OPENSTACK_ENDPOINT and PARITY_LOCALSTACK_ENDPOINT"
            ));
        }

        let services_csv = services.join(",");
        let localstack_services = services
            .iter()
            .map(|service| map_service_for_localstack(service))
            .collect::<Vec<_>>()
            .join(",");

        let openstack_port = free_port()?;
        let localstack_port = free_port()?;
        let openstack_endpoint = format!("http://127.0.0.1:{openstack_port}");
        let localstack_endpoint = format!("http://127.0.0.1:{localstack_port}");

        let mut openstack_args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--network".to_string(),
            config.docker_network_mode.clone(),
            "-p".to_string(),
            format!("127.0.0.1:{openstack_port}:4566"),
            "-e".to_string(),
            format!("SERVICES={services_csv}"),
            "-e".to_string(),
            "DEBUG=1".to_string(),
        ];
        if let Some(cpu) = &config.docker_cpu_limit {
            openstack_args.push("--cpus".to_string());
            openstack_args.push(cpu.clone());
        }
        if let Some(memory) = &config.docker_memory_limit {
            openstack_args.push("--memory".to_string());
            openstack_args.push(memory.clone());
        }
        openstack_args.push(config.openstack_image.clone());

        let openstack_output = Command::new("docker").args(&openstack_args).output()?;
        if !openstack_output.status.success() {
            return Err(anyhow::anyhow!(
                "failed to start openstack container: {}",
                String::from_utf8_lossy(&openstack_output.stderr)
            ));
        }
        let openstack_container_id = String::from_utf8_lossy(&openstack_output.stdout)
            .trim()
            .to_string();

        let mut localstack_args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--network".to_string(),
            config.docker_network_mode.clone(),
            "-p".to_string(),
            format!("127.0.0.1:{localstack_port}:4566"),
            "-e".to_string(),
            format!("SERVICES={localstack_services}"),
            "-e".to_string(),
            "DEBUG=1".to_string(),
        ];
        if let Some(cpu) = &config.docker_cpu_limit {
            localstack_args.push("--cpus".to_string());
            localstack_args.push(cpu.clone());
        }
        if let Some(memory) = &config.docker_memory_limit {
            localstack_args.push("--memory".to_string());
            localstack_args.push(memory.clone());
        }
        localstack_args.push(config.localstack_image.clone());

        let localstack_output = Command::new("docker").args(&localstack_args).output()?;
        if !localstack_output.status.success() {
            let _ = Command::new("docker")
                .args(["rm", "-f", &openstack_container_id])
                .output();
            return Err(anyhow::anyhow!(
                "failed to start localstack container: {}",
                String::from_utf8_lossy(&localstack_output.stderr)
            ));
        }
        let localstack_container_id = String::from_utf8_lossy(&localstack_output.stdout)
            .trim()
            .to_string();

        let timeout = Duration::from_secs(config.docker_startup_timeout_secs);
        if let Err(err) = preflight_symmetric_runtime(
            config,
            &openstack_container_id,
            &localstack_container_id,
            &openstack_endpoint,
            &localstack_endpoint,
            timeout,
        )
        .await
        {
            let _ = Command::new("docker")
                .args(["rm", "-f", &openstack_container_id])
                .output();
            let _ = Command::new("docker")
                .args(["rm", "-f", &localstack_container_id])
                .output();
            return Err(err);
        }

        Ok(Self {
            openstack: BenchmarkManagedTarget {
                endpoint: openstack_endpoint,
                runtime: "docker".to_string(),
                image: Some(config.openstack_image.clone()),
                cpu_limit: config.docker_cpu_limit.clone(),
                memory_limit: config.docker_memory_limit.clone(),
                network_mode: Some(config.docker_network_mode.clone()),
            },
            localstack: BenchmarkManagedTarget {
                endpoint: localstack_endpoint,
                runtime: "docker".to_string(),
                image: Some(config.localstack_image.clone()),
                cpu_limit: config.docker_cpu_limit.clone(),
                memory_limit: config.docker_memory_limit.clone(),
                network_mode: Some(config.docker_network_mode.clone()),
            },
            openstack_container_id: Some(openstack_container_id),
            localstack_container_id: Some(localstack_container_id),
            openstack_harness: None,
            idle_memory_summary: None,
        })
    }

    async fn stop(&mut self) {
        if let Some(container_id) = &self.localstack_container_id {
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .output();
        }
        self.localstack_container_id = None;

        if let Some(container_id) = &self.openstack_container_id {
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .output();
        }
        self.openstack_container_id = None;

        if let Some(harness) = self.openstack_harness.take() {
            harness.shutdown();
        }
    }

    fn collect_memory_summary(&self) -> Option<BenchmarkMemorySummary> {
        let mut samples = Vec::new();

        let openstack_rss = self
            .openstack_container_id
            .as_deref()
            .and_then(inspect_container_rss_bytes);
        let localstack_rss = self
            .localstack_container_id
            .as_deref()
            .and_then(inspect_container_rss_bytes);

        samples.push(BenchmarkMemorySample {
            target: "openstack".to_string(),
            rss_bytes: openstack_rss,
            raw_value: self
                .openstack_container_id
                .as_deref()
                .and_then(inspect_container_memory_usage),
        });
        samples.push(BenchmarkMemorySample {
            target: "localstack".to_string(),
            rss_bytes: localstack_rss,
            raw_value: self
                .localstack_container_id
                .as_deref()
                .and_then(inspect_container_memory_usage),
        });

        let mut missing_targets = Vec::new();
        if openstack_rss.is_none() {
            missing_targets.push("openstack".to_string());
        }
        if localstack_rss.is_none() {
            missing_targets.push("localstack".to_string());
        }

        if openstack_rss.is_none() && localstack_rss.is_none() {
            return None;
        }

        let idle_openstack = self
            .idle_memory_summary
            .as_ref()
            .and_then(|m| m.openstack_rss_bytes);
        let idle_localstack = self
            .idle_memory_summary
            .as_ref()
            .and_then(|m| m.localstack_rss_bytes);

        Some(BenchmarkMemorySummary {
            openstack_idle_rss_bytes: idle_openstack,
            localstack_idle_rss_bytes: idle_localstack,
            openstack_rss_bytes: openstack_rss,
            localstack_rss_bytes: localstack_rss,
            rss_ratio_openstack_over_localstack: match (openstack_rss, localstack_rss) {
                (Some(a), Some(b)) if b > 0 => Some(a as f64 / b as f64),
                _ => None,
            },
            missing_targets,
            samples,
        })
    }

    fn capture_idle_memory_snapshot(&mut self) {
        self.idle_memory_summary = self.collect_memory_summary();
    }
}

fn average_duration(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

async fn collect_startup_summary(
    manager: &BenchmarkTargetManager,
    samples: usize,
) -> BenchmarkStartupSummary {
    let client = reqwest::Client::new();
    let mut startup_samples = Vec::new();
    let mut openstack_values = Vec::new();
    let mut localstack_values = Vec::new();
    let mut missing_targets = Vec::new();

    for _ in 0..samples {
        let start = Instant::now();
        let os_ok = client
            .get(format!("{}/_localstack/health", manager.openstack.endpoint))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        let os_ms = start.elapsed().as_secs_f64() * 1000.0;
        if os_ok {
            openstack_values.push(os_ms);
            startup_samples.push(BenchmarkStartupSample {
                target: "openstack".to_string(),
                startup_ms: os_ms,
            });
        }

        let start = Instant::now();
        let ls_ok = client
            .get(format!(
                "{}/_localstack/health",
                manager.localstack.endpoint
            ))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        let ls_ms = start.elapsed().as_secs_f64() * 1000.0;
        if ls_ok {
            localstack_values.push(ls_ms);
            startup_samples.push(BenchmarkStartupSample {
                target: "localstack".to_string(),
                startup_ms: ls_ms,
            });
        }
    }

    if openstack_values.is_empty() {
        missing_targets.push("openstack".to_string());
    }
    if localstack_values.is_empty() {
        missing_targets.push("localstack".to_string());
    }

    let openstack_avg_ms = average_duration(&openstack_values);
    let localstack_avg_ms = average_duration(&localstack_values);

    BenchmarkStartupSummary {
        openstack_avg_ms,
        localstack_avg_ms,
        startup_ratio_openstack_over_localstack: match (openstack_avg_ms, localstack_avg_ms) {
            (Some(a), Some(b)) if b > 0.0 => Some(a / b),
            _ => None,
        },
        samples: startup_samples,
        missing_targets,
    }
}

pub async fn run_profile(
    config: &BenchmarkConfig,
    profile_name: &str,
    output_override: Option<PathBuf>,
) -> anyhow::Result<BenchmarkReport> {
    let profile = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| anyhow::anyhow!("unknown benchmark profile: {profile_name}"))?
        .clone();

    validate_service_workload_matrix(&profile.services)?;

    std::fs::create_dir_all(&config.report_dir)?;

    let run_id = format!("{}-{}", profile_name, Utc::now().format("%Y%m%d%H%M%S"));
    let scenarios = load_profile_scenarios(profile_name, &run_id)
        .into_iter()
        .filter(|s| {
            profile_matches(profile_name, &s.profile) && profile.services.contains(&s.service)
        })
        .collect::<Vec<_>>();

    if scenarios.is_empty() {
        return Err(anyhow::anyhow!(
            "benchmark profile '{}' has no scenarios configured",
            profile_name
        ));
    }

    let runtime = BenchmarkRuntimeMetadata {
        mode: config.runtime_mode,
        execution_order_policy: config.execution_order_policy,
        execution_driver: config.execution_driver,
        benchmark_lane_mode: config.lane_mode,
        openstack_persistence_mode: config.openstack_persistence_mode,
        localstack_persistence_mode: config.localstack_persistence_mode,
        persistence_mode_equivalent: config.openstack_persistence_mode
            == config.localstack_persistence_mode,
        diagnostics_only_role_coverage: config.diagnostics_only_role_coverage,
        strict_role_coverage_gate: config.strict_role_coverage_gate,
        non_blocking_profile: matches!(profile_name, "fair-high" | "fair-extreme"),
        docker_constraints: (config.runtime_mode == BenchmarkRuntimeMode::SymmetricDocker).then(
            || DockerRuntimeConstraints {
                cpu_limit: config.docker_cpu_limit.clone(),
                memory_limit: config.docker_memory_limit.clone(),
                network_mode: config.docker_network_mode.clone(),
            },
        ),
    };

    let mut manager = BenchmarkTargetManager::start(config, &profile.services).await?;
    let startup_summary = collect_startup_summary(&manager, config.startup_samples).await;
    manager.capture_idle_memory_snapshot();
    let openstack_meta = BenchmarkTargetMetadata {
        endpoint: manager.openstack.endpoint.clone(),
        runtime: manager.openstack.runtime.clone(),
        image: manager.openstack.image.clone(),
        cpu_limit: manager.openstack.cpu_limit.clone(),
        memory_limit: manager.openstack.memory_limit.clone(),
        network_mode: manager.openstack.network_mode.clone(),
        localstack_image: None,
        localstack_version: None,
    };
    let localstack_meta = BenchmarkTargetMetadata {
        endpoint: manager.localstack.endpoint.clone(),
        runtime: manager.localstack.runtime.clone(),
        image: manager.localstack.image.clone(),
        cpu_limit: manager.localstack.cpu_limit.clone(),
        memory_limit: manager.localstack.memory_limit.clone(),
        network_mode: manager.localstack.network_mode.clone(),
        localstack_image: Some(config.localstack_image.clone()),
        localstack_version: localstack_version_from_image(&config.localstack_image),
    };

    let mut results = Vec::new();
    for (idx, scenario) in scenarios.into_iter().enumerate() {
        let scenario_run = scenario_run_config(&profile, &scenario);
        let skip_reason = heavy_object_skip_reason(config, &scenario);
        let skipped = skip_reason.is_some();

        let (openstack_metrics, localstack_metrics) = if skipped {
            (BenchmarkMetrics::default(), BenchmarkMetrics::default())
        } else {
            execute_in_order(
                &manager,
                &scenario,
                &scenario_run,
                config.lane_mode,
                config.execution_driver,
                config.execution_order_policy,
                idx,
            )
            .await
        };

        let comparison = compare_metrics(&openstack_metrics, &localstack_metrics);
        let service_execution_class = service_execution_class(&scenario.service);
        let service_durability_class = service_durability_class(&scenario.service);
        let invalid_reason = performance_invalid_reason(
            scenario.scenario_class,
            scenario.scenario_role,
            skipped,
            skip_reason.as_deref(),
            &openstack_metrics,
            &localstack_metrics,
            service_execution_class,
            config,
        );
        let valid_for_performance = scenario.scenario_class == BenchmarkScenarioClass::Performance
            && invalid_reason.is_none();

        results.push(BenchmarkScenarioResult {
            scenario_id: scenario.id.clone(),
            service: scenario.service.clone(),
            service_execution_class,
            service_durability_class,
            scenario_class: scenario.scenario_class,
            load_tier: scenario.load_tier,
            scenario_role: scenario.scenario_role,
            skipped,
            skip_reason,
            valid_for_performance,
            invalid_reason,
            run_config: scenario_run,
            openstack: BenchmarkTargetResult {
                metadata: openstack_meta.clone(),
                metrics: openstack_metrics,
            },
            localstack: BenchmarkTargetResult {
                metadata: localstack_meta.clone(),
                metrics: localstack_metrics,
            },
            comparison,
        });
    }

    let memory_summary = manager.collect_memory_summary();
    let mut summary = summarize_results(&results);
    enforce_required_role_completeness(&mut summary);
    if config.diagnostics_only_role_coverage && !config.strict_role_coverage_gate {
        summary.lane_interpretable = summary.valid_performance_scenarios > 0;
    }
    let report = BenchmarkReport {
        profile: profile_name.to_string(),
        run_id: run_id.clone(),
        generated_at: Utc::now().to_rfc3339(),
        runtime,
        startup_summary: Some(startup_summary),
        openstack_target: openstack_meta,
        localstack_target: localstack_meta,
        memory_summary,
        results,
        summary,
    };

    let output_path =
        output_override.unwrap_or_else(|| config.report_dir.join(format!("{run_id}.json")));
    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&output_path, report_json)?;
    let profile_latest_path = config
        .report_dir
        .join(format!("{}-latest.json", profile_name));
    let profile_latest_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(profile_latest_path, profile_latest_json)?;

    manager.stop().await;
    Ok(report)
}

fn parse_size_bytes(input: &str) -> Option<u64> {
    let trimmed = input.trim();
    if let Ok(v) = trimmed.parse::<u64>() {
        return Some(v);
    }

    let upper = trimmed.to_ascii_uppercase();
    if let Some(prefix) = upper.strip_suffix("GB") {
        return prefix
            .trim()
            .parse::<u64>()
            .ok()
            .map(|v| v * 1024 * 1024 * 1024);
    }
    if let Some(prefix) = upper.strip_suffix("MB") {
        return prefix.trim().parse::<u64>().ok().map(|v| v * 1024 * 1024);
    }
    if let Some(prefix) = upper.strip_suffix("KB") {
        return prefix.trim().parse::<u64>().ok().map(|v| v * 1024);
    }
    None
}

fn benchmark_file_for_size(size_bytes: u64) -> String {
    let root = std::env::var("BENCHMARK_LARGE_FILES_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "tests/benchmark/fixtures".to_string());
    format!("{root}/{size_bytes}.bin")
}

fn scenario_run_config(
    profile: &BenchmarkProfile,
    scenario: &BenchmarkScenario,
) -> BenchmarkRunConfig {
    BenchmarkRunConfig {
        warmup_iterations: scenario
            .warmup_iterations
            .unwrap_or(profile.warmup_iterations),
        measured_iterations: scenario
            .measured_iterations
            .unwrap_or(profile.measured_iterations),
        operations_per_iteration: scenario
            .operations_per_iteration
            .unwrap_or(profile.operations_per_iteration),
        concurrency: scenario.concurrency.unwrap_or(profile.concurrency).max(1),
    }
}

async fn execute_in_order(
    manager: &BenchmarkTargetManager,
    scenario: &BenchmarkScenario,
    scenario_run: &BenchmarkRunConfig,
    lane_mode: BenchmarkLaneMode,
    execution_driver: BenchmarkExecutionDriver,
    policy: ExecutionOrderPolicy,
    scenario_idx: usize,
) -> (BenchmarkMetrics, BenchmarkMetrics) {
    let openstack_first = match policy {
        ExecutionOrderPolicy::OpenstackFirst => true,
        ExecutionOrderPolicy::LocalstackFirst => false,
        ExecutionOrderPolicy::Alternating => scenario_idx.is_multiple_of(2),
    };

    if openstack_first {
        let openstack_metrics = execute_scenario(
            &manager.openstack.endpoint,
            scenario,
            scenario_run,
            lane_mode,
            execution_driver,
        )
        .await;
        let localstack_metrics = execute_scenario(
            &manager.localstack.endpoint,
            scenario,
            scenario_run,
            lane_mode,
            execution_driver,
        )
        .await;
        (openstack_metrics, localstack_metrics)
    } else {
        let localstack_metrics = execute_scenario(
            &manager.localstack.endpoint,
            scenario,
            scenario_run,
            lane_mode,
            execution_driver,
        )
        .await;
        let openstack_metrics = execute_scenario(
            &manager.openstack.endpoint,
            scenario,
            scenario_run,
            lane_mode,
            execution_driver,
        )
        .await;
        (openstack_metrics, localstack_metrics)
    }
}

fn heavy_object_skip_reason(
    config: &BenchmarkConfig,
    scenario: &BenchmarkScenario,
) -> Option<String> {
    if scenario.load_tier != BenchmarkLoadTier::Extreme {
        return None;
    }
    if scenario.service != "s3" {
        return None;
    }
    if !scenario.id.contains("heavy") {
        return None;
    }
    if config.heavy_object_enabled {
        let Some(size_bytes) = parse_heavy_s3_size_bytes(&scenario.id) else {
            return Some("unable to parse heavy object size from scenario id".to_string());
        };
        let path = benchmark_file_for_size(size_bytes);
        if !Path::new(&path).exists() {
            return Some(format!(
                "heavy object fixture missing: {path} (set BENCHMARK_LARGE_FILES_DIR or disable BENCHMARK_HEAVY_OBJECTS)"
            ));
        }
        return None;
    }

    Some(
        "heavy object scenarios require BENCHMARK_HEAVY_OBJECTS=1 and sufficient runtime resources"
            .to_string(),
    )
}

fn performance_invalid_reason(
    scenario_class: BenchmarkScenarioClass,
    scenario_role: BenchmarkScenarioRole,
    skipped: bool,
    skip_reason: Option<&str>,
    openstack: &BenchmarkMetrics,
    localstack: &BenchmarkMetrics,
    service_class: Option<ServiceExecutionClass>,
    config: &BenchmarkConfig,
) -> Option<String> {
    if scenario_class != BenchmarkScenarioClass::Performance {
        return None;
    }
    if config.openstack_persistence_mode != config.localstack_persistence_mode {
        return Some("mode_mismatch".to_string());
    }
    if service_class.is_none() {
        return Some("missing_service_class".to_string());
    }
    if scenario_role == BenchmarkScenarioRole::Aux {
        return Some("unknown scenario role".to_string());
    }
    if skipped {
        return Some(skip_reason.unwrap_or("scenario skipped").to_string());
    }
    if openstack.operation_count == 0 || localstack.operation_count == 0 {
        return Some("zero operation count".to_string());
    }

    let os_successes = openstack
        .operation_count
        .saturating_sub(openstack.error_count);
    let ls_successes = localstack
        .operation_count
        .saturating_sub(localstack.error_count);
    if os_successes == 0 || ls_successes == 0 {
        return Some("insufficient cross-target successful operations".to_string());
    }

    if openstack.error_count >= openstack.operation_count
        || localstack.error_count >= localstack.operation_count
    {
        return Some("all operations failed".to_string());
    }

    let class = service_class.expect("checked above");
    let envelope = class_envelope(class, "performance");
    if let Some(p95) = safe_ratio(openstack.latency_p95_ms, localstack.latency_p95_ms)
        && p95 > envelope.max_latency_p95_ratio
    {
        return Some(format!("class-envelope-latency-p95-breach:{p95:.3}"));
    }
    if let Some(p99) = safe_ratio(openstack.latency_p99_ms, localstack.latency_p99_ms)
        && p99 > envelope.max_latency_p99_ratio
    {
        return Some(format!("class-envelope-latency-p99-breach:{p99:.3}"));
    }
    if let Some(tp) = safe_ratio(
        openstack.throughput_ops_per_sec,
        localstack.throughput_ops_per_sec,
    ) && tp < envelope.min_throughput_ratio
    {
        return Some(format!("class-envelope-throughput-breach:{tp:.3}"));
    }

    None
}

fn benchmark_runtime_mode_from_env() -> BenchmarkRuntimeMode {
    match std::env::var("PARITY_BENCHMARK_RUNTIME_MODE") {
        Ok(mode) if mode.eq_ignore_ascii_case("symmetric-docker") => {
            BenchmarkRuntimeMode::SymmetricDocker
        }
        _ => BenchmarkRuntimeMode::Asymmetric,
    }
}

fn execution_order_policy_from_env() -> ExecutionOrderPolicy {
    match std::env::var("PARITY_BENCHMARK_EXECUTION_ORDER") {
        Ok(policy) if policy.eq_ignore_ascii_case("localstack-first") => {
            ExecutionOrderPolicy::LocalstackFirst
        }
        Ok(policy) if policy.eq_ignore_ascii_case("alternating") => {
            ExecutionOrderPolicy::Alternating
        }
        _ => ExecutionOrderPolicy::OpenstackFirst,
    }
}

fn benchmark_lane_mode_from_env() -> BenchmarkLaneMode {
    match std::env::var("PARITY_BENCHMARK_LANE_MODE") {
        Ok(mode) if mode.eq_ignore_ascii_case("low-overhead") => BenchmarkLaneMode::LowOverhead,
        _ => BenchmarkLaneMode::HarnessInfluenced,
    }
}

fn benchmark_execution_driver_from_env() -> BenchmarkExecutionDriver {
    match std::env::var("PARITY_BENCHMARK_EXECUTION_DRIVER") {
        Ok(mode) if mode.eq_ignore_ascii_case("direct-http") => {
            BenchmarkExecutionDriver::DirectHttp
        }
        _ => BenchmarkExecutionDriver::AwsCli,
    }
}

fn map_service_for_localstack(service: &str) -> String {
    match service {
        "events" => "eventbridge".to_string(),
        "states" => "stepfunctions".to_string(),
        _ => service.to_string(),
    }
}

fn default_read_write_commands_for_service(
    service: &str,
) -> Option<((ProtocolFamily, Vec<String>), (ProtocolFamily, Vec<String>))> {
    let pair = match service {
        "acm" => (
            (
                ProtocolFamily::Json,
                vec![
                    "acm".into(),
                    "request-certificate".into(),
                    "--domain-name".into(),
                    "bench-{{run_id}}.example.com".into(),
                    "--validation-method".into(),
                    "DNS".into(),
                ],
            ),
            (
                ProtocolFamily::Json,
                vec!["acm".into(), "list-certificates".into(), "--max-items".into(), "20".into()],
            ),
        ),
        "apigateway" => (
            (
                ProtocolFamily::RestJson,
                vec!["apigateway".into(), "create-rest-api".into(), "--name".into(), "bench-{{run_id}}".into()],
            ),
            (
                ProtocolFamily::RestJson,
                vec!["apigateway".into(), "get-rest-apis".into(), "--limit".into(), "50".into()],
            ),
        ),
        "cloudformation" => (
            (
                ProtocolFamily::QueryXml,
                vec![
                    "cloudformation".into(),
                    "create-stack".into(),
                    "--stack-name".into(),
                    "bench-{{run_id}}".into(),
                    "--template-body".into(),
                    "file://tests/benchmark/fixtures/cfn_minimal_template.json".into(),
                ],
            ),
            (
                ProtocolFamily::QueryXml,
                vec!["cloudformation".into(), "describe-stacks".into()],
            ),
        ),
        "cloudwatch" => (
            (
                ProtocolFamily::Json,
                vec![
                    "cloudwatch".into(),
                    "put-metric-data".into(),
                    "--namespace".into(),
                    "Benchmark".into(),
                    "--metric-name".into(),
                    "Latency".into(),
                    "--value".into(),
                    "1".into(),
                ],
            ),
            (ProtocolFamily::Json, vec!["cloudwatch".into(), "list-metrics".into()]),
        ),
        "dynamodb" => (
            (
                ProtocolFamily::Json,
                vec![
                    "dynamodb".into(),
                    "put-item".into(),
                    "--table-name".into(),
                    "{{table}}".into(),
                    "--item".into(),
                    "{\"id\":{\"S\":\"k\"},\"v\":{\"S\":\"1\"}}".into(),
                ],
            ),
            (
                ProtocolFamily::Json,
                vec![
                    "dynamodb".into(),
                    "get-item".into(),
                    "--table-name".into(),
                    "{{table}}".into(),
                    "--key".into(),
                    "{\"id\":{\"S\":\"k\"}}".into(),
                ],
            ),
        ),
        "ec2" => (
            (
                ProtocolFamily::QueryXml,
                vec!["ec2".into(), "create-tags".into(), "--resources".into(), "i-1234567890abcdef0".into(), "--tags".into(), "Key=bench,Value={{run_id}}".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["ec2".into(), "describe-instances".into()]),
        ),
        "ecr" => (
            (
                ProtocolFamily::Json,
                vec!["ecr".into(), "create-repository".into(), "--repository-name".into(), "bench-{{run_id}}".into()],
            ),
            (ProtocolFamily::Json, vec!["ecr".into(), "describe-repositories".into(), "--max-results".into(), "50".into()]),
        ),
        "events" => (
            (
                ProtocolFamily::Json,
                vec!["events".into(), "put-rule".into(), "--name".into(), "bench-{{run_id}}".into(), "--schedule-expression".into(), "rate(5 minutes)".into()],
            ),
            (ProtocolFamily::Json, vec!["events".into(), "list-rules".into(), "--limit".into(), "50".into()]),
        ),
        "firehose" => (
            (
                ProtocolFamily::Json,
                vec!["firehose".into(), "put-record".into(), "--delivery-stream-name".into(), "bench-{{run_id}}".into(), "--record".into(), "Data=YmVuY2g=".into()],
            ),
            (ProtocolFamily::Json, vec!["firehose".into(), "list-delivery-streams".into()]),
        ),
        "iam" => (
            (
                ProtocolFamily::QueryXml,
                vec!["iam".into(), "create-user".into(), "--user-name".into(), "bench-{{run_id}}".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["iam".into(), "list-users".into()]),
        ),
        "kinesis" => (
            (
                ProtocolFamily::Json,
                vec!["kinesis".into(), "put-record".into(), "--stream-name".into(), "bench-{{run_id}}".into(), "--partition-key".into(), "pk".into(), "--data".into(), "YmVuY2g=".into()],
            ),
            (ProtocolFamily::Json, vec!["kinesis".into(), "list-streams".into()]),
        ),
        "kms" => (
            (
                ProtocolFamily::Json,
                vec!["kms".into(), "create-alias".into(), "--alias-name".into(), "alias/bench-{{run_id}}".into(), "--target-key-id".into(), "1234abcd-12ab-34cd-56ef-1234567890ab".into()],
            ),
            (ProtocolFamily::Json, vec!["kms".into(), "list-keys".into(), "--limit".into(), "100".into()]),
        ),
        "lambda" => (
            (
                ProtocolFamily::RestJson,
                vec!["lambda".into(), "invoke".into(), "--function-name".into(), "bench-{{run_id}}".into(), "--payload".into(), "{}".into(), "/tmp/lambda-out.json".into()],
            ),
            (ProtocolFamily::RestJson, vec!["lambda".into(), "list-functions".into()]),
        ),
        "opensearch" => (
            (
                ProtocolFamily::RestJson,
                vec!["opensearch".into(), "create-domain".into(), "--domain-name".into(), "bench-{{run_id}}".into(), "--engine-version".into(), "OpenSearch_2.11".into(), "--cluster-config".into(), "InstanceType=t3.small.search,InstanceCount=1".into()],
            ),
            (ProtocolFamily::RestJson, vec!["opensearch".into(), "list-domain-names".into()]),
        ),
        "redshift" => (
            (
                ProtocolFamily::QueryXml,
                vec!["redshift".into(), "create-cluster-snapshot".into(), "--cluster-identifier".into(), "bench-{{run_id}}".into(), "--snapshot-identifier".into(), "bench-snap-{{run_id}}".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["redshift".into(), "describe-clusters".into()]),
        ),
        "route53" => (
            (
                ProtocolFamily::RestXml,
                vec!["route53".into(), "create-health-check".into(), "--caller-reference".into(), "{{run_id}}".into(), "--health-check-config".into(), "IPAddress=127.0.0.1,Port=80,Type=HTTP,ResourcePath=/,FullyQualifiedDomainName=example.com,RequestInterval=30,FailureThreshold=3".into()],
            ),
            (ProtocolFamily::RestXml, vec!["route53".into(), "list-hosted-zones".into(), "--max-items".into(), "50".into()]),
        ),
        "s3" => (
            (
                ProtocolFamily::RestXml,
                vec!["s3api".into(), "put-object".into(), "--bucket".into(), "{{bucket}}".into(), "--key".into(), "bench-{{run_id}}.txt".into(), "--body".into(), "README.md".into()],
            ),
            (
                ProtocolFamily::RestXml,
                vec!["s3api".into(), "get-object".into(), "--bucket".into(), "{{bucket}}".into(), "--key".into(), "bench-{{run_id}}.txt".into(), "/tmp/s3-bench-{{run_id}}.txt".into()],
            ),
        ),
        "secretsmanager" => (
            (
                ProtocolFamily::Json,
                vec!["secretsmanager".into(), "put-secret-value".into(), "--secret-id".into(), "bench/secret-{{run_id}}".into(), "--secret-string".into(), "v1".into()],
            ),
            (ProtocolFamily::Json, vec!["secretsmanager".into(), "list-secrets".into()]),
        ),
        "ses" => (
            (
                ProtocolFamily::QueryXml,
                vec!["ses".into(), "verify-email-identity".into(), "--email-address".into(), "bench-{{run_id}}@example.com".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["ses".into(), "list-identities".into(), "--max-items".into(), "100".into()]),
        ),
        "sns" => (
            (
                ProtocolFamily::QueryXml,
                vec!["sns".into(), "publish".into(), "--topic-arn".into(), "arn:aws:sns:us-east-1:000000000000:bench-topic-{{run_id}}".into(), "--message".into(), "bench".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["sns".into(), "list-topics".into()]),
        ),
        "sqs" => (
            (
                ProtocolFamily::QueryXml,
                vec!["sqs".into(), "send-message".into(), "--queue-url".into(), "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}".into(), "--message-body".into(), "bench-{{run_id}}".into()],
            ),
            (
                ProtocolFamily::QueryXml,
                vec!["sqs".into(), "receive-message".into(), "--queue-url".into(), "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}".into(), "--max-number-of-messages".into(), "1".into()],
            ),
        ),
        "ssm" => (
            (
                ProtocolFamily::Json,
                vec!["ssm".into(), "put-parameter".into(), "--name".into(), "/bench/param-{{run_id}}".into(), "--value".into(), "v1".into(), "--type".into(), "String".into(), "--overwrite".into()],
            ),
            (ProtocolFamily::Json, vec!["ssm".into(), "describe-parameters".into(), "--max-results".into(), "50".into()]),
        ),
        "states" => (
            (
                ProtocolFamily::Json,
                vec!["stepfunctions".into(), "start-execution".into(), "--state-machine-arn".into(), "arn:aws:states:us-east-1:000000000000:stateMachine:bench-{{run_id}}".into(), "--name".into(), "exec-{{run_id}}".into(), "--input".into(), "{}".into()],
            ),
            (ProtocolFamily::Json, vec!["stepfunctions".into(), "list-state-machines".into()]),
        ),
        "sts" => (
            (
                ProtocolFamily::QueryXml,
                vec!["sts".into(), "assume-role".into(), "--role-arn".into(), "arn:aws:iam::000000000000:role/bench-role".into(), "--role-session-name".into(), "bench-{{run_id}}".into()],
            ),
            (ProtocolFamily::QueryXml, vec!["sts".into(), "get-caller-identity".into()]),
        ),
        _ => return None,
    };

    Some(pair)
}

async fn preflight_symmetric_runtime(
    config: &BenchmarkConfig,
    openstack_container_id: &str,
    localstack_container_id: &str,
    openstack_endpoint: &str,
    localstack_endpoint: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    wait_for_health(
        openstack_endpoint,
        timeout,
        "openstack",
        Some(openstack_container_id),
    )
    .await?;
    wait_for_health(
        localstack_endpoint,
        timeout,
        "localstack",
        Some(localstack_container_id),
    )
    .await?;

    let inspect = |id: &str| -> anyhow::Result<(Option<String>, Option<String>, Option<String>)> {
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.HostConfig.NetworkMode}}",
                id,
            ])
            .output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "failed to inspect container {id}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let mut parts = line.split('|');
        let nano_cpus = parts
            .next()
            .filter(|s| !s.is_empty() && *s != "0")
            .map(ToString::to_string);
        let memory = parts
            .next()
            .filter(|s| !s.is_empty() && *s != "0")
            .map(ToString::to_string);
        let network_mode = parts
            .next()
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        Ok((nano_cpus, memory, network_mode))
    };

    let openstack_limits = inspect(openstack_container_id)?;
    let localstack_limits = inspect(localstack_container_id)?;

    if openstack_limits != localstack_limits {
        return Err(anyhow::anyhow!(
            "symmetric runtime preflight failed: openstack and localstack container limits differ"
        ));
    }

    if let Some(expected_network) = Some(config.docker_network_mode.as_str())
        && openstack_limits.2.as_deref() != Some(expected_network)
    {
        return Err(anyhow::anyhow!(
            "symmetric runtime preflight failed: expected network mode {expected_network}, got {:?}",
            openstack_limits.2
        ));
    }

    Ok(())
}

async fn wait_for_health(
    endpoint: &str,
    timeout: Duration,
    target: &str,
    container_id: Option<&str>,
) -> anyhow::Result<()> {
    let health = format!("{endpoint}/_localstack/health");
    let deadline = Instant::now() + timeout;
    let mut attempts = 0usize;
    let mut last_error = String::new();

    loop {
        attempts += 1;
        if Instant::now() > deadline {
            let debug_context = container_id
                .map(container_debug_context)
                .unwrap_or_else(|| "container diagnostics unavailable".to_string());
            return Err(anyhow::anyhow!(
                "timed out waiting for benchmark target health at {health} (target={target}, attempts={attempts}, last_error={last_error})\n{debug_context}"
            ));
        }

        match reqwest::get(&health).await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(());
                }

                let body = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed-to-read-body>".to_string());
                let truncated = truncate_debug(&body, 400);
                last_error = format!("http_status={status}; body={truncated}");
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn container_id_from_output(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn container_debug_context(container_id: &str) -> String {
    let inspect = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "status={{.State.Status}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}n/a{{end}} started={{.State.StartedAt}}",
            container_id,
        ])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "inspect unavailable".to_string());

    let logs = Command::new("docker")
        .args(["logs", "--tail", "120", container_id])
        .output()
        .ok()
        .map(|out| {
            let mut combined = String::new();
            if !out.stdout.is_empty() {
                combined.push_str(&String::from_utf8_lossy(&out.stdout));
            }
            if !out.stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            let trimmed = combined.trim();
            if trimmed.is_empty() {
                "<empty>".to_string()
            } else {
                truncate_debug(trimmed, 2000)
            }
        })
        .unwrap_or_else(|| "logs unavailable".to_string());

    let state_json = Command::new("docker")
        .args(["inspect", "--format", "{{json .State}}", container_id])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .map(|json| truncate_debug(&json, 2000))
        .unwrap_or_else(|| "state unavailable".to_string());

    format!(
        "container_id={container_id}; inspect=[{inspect}]; state={state_json}; recent_logs=[{logs}]"
    )
}

fn truncate_debug(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }

    let mut end = max_len;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...<truncated>", &input[..end])
}

fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn inspect_container_memory_usage(container_id: &str) -> Option<String> {
    let output = Command::new("docker")
        .args([
            "stats",
            "--no-stream",
            "--format",
            "{{.MemUsage}}",
            container_id,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn inspect_container_rss_bytes(container_id: &str) -> Option<u64> {
    let raw = inspect_container_memory_usage(container_id)?;
    let used = raw.split('/').next()?.trim();
    parse_docker_mem_value_to_bytes(used)
}

fn parse_docker_mem_value_to_bytes(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    let mut idx = 0usize;
    for (i, c) in trimmed.char_indices() {
        if !(c.is_ascii_digit() || c == '.') {
            idx = i;
            break;
        }
    }
    if idx == 0 {
        return trimmed.parse::<u64>().ok();
    }
    let number = trimmed[..idx].trim().parse::<f64>().ok()?;
    let unit = trimmed[idx..].trim().to_ascii_uppercase();
    let multiplier = if unit.starts_with("KIB") {
        1024f64
    } else if unit.starts_with("MIB") {
        1024f64 * 1024f64
    } else if unit.starts_with("GIB") {
        1024f64 * 1024f64 * 1024f64
    } else if unit.starts_with("B") {
        1f64
    } else {
        return None;
    };
    Some((number * multiplier) as u64)
}

async fn execute_scenario(
    endpoint: &str,
    scenario: &BenchmarkScenario,
    run_config: &BenchmarkRunConfig,
    lane_mode: BenchmarkLaneMode,
    execution_driver: BenchmarkExecutionDriver,
) -> BenchmarkMetrics {
    let mut context = HashMap::new();

    for step in &scenario.setup {
        let _ = execute_step(endpoint, step, &mut context).await;
    }

    for _ in 0..run_config.warmup_iterations {
        let _ = run_iteration(
            endpoint,
            scenario,
            run_config,
            &context,
            lane_mode,
            execution_driver,
        )
        .await;
    }

    let mut latencies = Vec::new();
    let mut operation_count = 0usize;
    let mut error_count = 0usize;
    let started = Instant::now();

    for _ in 0..run_config.measured_iterations {
        let iter = run_iteration(
            endpoint,
            scenario,
            run_config,
            &context,
            lane_mode,
            execution_driver,
        )
        .await;
        latencies.extend(iter.latencies_ms);
        operation_count += iter.operation_count;
        error_count += iter.error_count;
    }
    let elapsed_secs = started.elapsed().as_secs_f64().max(0.001);

    for step in &scenario.cleanup {
        let _ = execute_step(endpoint, step, &mut context).await;
    }

    BenchmarkMetrics {
        latency_p50_ms: percentile(&latencies, 0.50),
        latency_p95_ms: percentile(&latencies, 0.95),
        latency_p99_ms: percentile(&latencies, 0.99),
        latency_min_ms: min_value(&latencies),
        latency_max_ms: max_value(&latencies),
        latency_stddev_ms: stddev(&latencies),
        throughput_ops_per_sec: operation_count as f64 / elapsed_secs,
        operation_count,
        error_count,
        success_rate: if operation_count == 0 {
            0.0
        } else {
            (operation_count.saturating_sub(error_count)) as f64 / operation_count as f64
        },
        timeout_count: 0,
        retry_count: 0,
        total_duration_ms: elapsed_secs * 1000.0,
    }
}

#[derive(Debug)]
struct IterationResult {
    latencies_ms: Vec<f64>,
    operation_count: usize,
    error_count: usize,
}

async fn run_iteration(
    endpoint: &str,
    scenario: &BenchmarkScenario,
    run_config: &BenchmarkRunConfig,
    context: &HashMap<String, String>,
    lane_mode: BenchmarkLaneMode,
    execution_driver: BenchmarkExecutionDriver,
) -> IterationResult {
    let mut remaining = run_config.operations_per_iteration;
    let mut latencies_ms = Vec::with_capacity(run_config.operations_per_iteration);
    let mut operation_count = 0usize;
    let mut error_count = 0usize;

    while remaining > 0 {
        let batch = remaining.min(run_config.concurrency);
        let mut tasks = tokio::task::JoinSet::new();

        for _ in 0..batch {
            let endpoint_owned = endpoint.to_string();
            let step = scenario.operation.clone();
            let context_owned = context.clone();
            tasks.spawn(async move {
                execute_operation_step(
                    &endpoint_owned,
                    &step,
                    &context_owned,
                    lane_mode,
                    execution_driver,
                )
                .await
            });
        }

        while let Some(joined) = tasks.join_next().await {
            let result = joined.unwrap_or(StepExecution {
                elapsed_ms: 0.0,
                success: false,
            });
            operation_count += 1;
            latencies_ms.push(result.elapsed_ms);
            if !result.success {
                error_count += 1;
            }
        }

        remaining -= batch;
    }

    IterationResult {
        latencies_ms,
        operation_count,
        error_count,
    }
}

#[derive(Debug, Clone, Copy)]
struct StepExecution {
    elapsed_ms: f64,
    success: bool,
}

async fn execute_step(
    endpoint: &str,
    step: &ScenarioStep,
    context: &mut HashMap<String, String>,
) -> StepExecution {
    let rendered = render_command(&step.command, context);
    let (elapsed_ms, output) =
        execute_aws_command(endpoint, rendered, BenchmarkLaneMode::HarnessInfluenced).await;

    match output {
        Ok(out) => {
            if let Some(capture) = &step.capture_json {
                let stdout = String::from_utf8_lossy(&out.stdout);
                capture_output_value(&stdout, context, capture);
            }
            StepExecution {
                elapsed_ms,
                success: out.status.success() == step.expect_success,
            }
        }
        Err(_) => StepExecution {
            elapsed_ms,
            success: false,
        },
    }
}

async fn execute_operation_step(
    endpoint: &str,
    step: &ScenarioStep,
    context: &HashMap<String, String>,
    lane_mode: BenchmarkLaneMode,
    execution_driver: BenchmarkExecutionDriver,
) -> StepExecution {
    let command = render_command(&step.command, context);
    match execution_driver {
        BenchmarkExecutionDriver::AwsCli => {
            let (elapsed_ms, output) = execute_aws_command(endpoint, command, lane_mode).await;
            match output {
                Ok(out) => StepExecution {
                    elapsed_ms,
                    success: out.status.success() == step.expect_success,
                },
                Err(_) => StepExecution {
                    elapsed_ms,
                    success: false,
                },
            }
        }
        BenchmarkExecutionDriver::DirectHttp => {
            let (elapsed_ms, success) = execute_direct_http_command(endpoint, &command).await;
            if let Some(success) = success {
                StepExecution {
                    elapsed_ms,
                    success: success == step.expect_success,
                }
            } else {
                let (fallback_elapsed, output) =
                    execute_aws_command(endpoint, command, lane_mode).await;
                match output {
                    Ok(out) => StepExecution {
                        elapsed_ms: elapsed_ms + fallback_elapsed,
                        success: out.status.success() == step.expect_success,
                    },
                    Err(_) => StepExecution {
                        elapsed_ms: elapsed_ms + fallback_elapsed,
                        success: false,
                    },
                }
            }
        }
    }
}

async fn execute_direct_http_command(endpoint: &str, command: &[String]) -> (f64, Option<bool>) {
    let started = Instant::now();
    let client = reqwest::Client::new();

    let response = match command {
        [svc, op] if svc == "dynamodb" && op == "list-tables" => {
            client
                .post(endpoint)
                .header("x-amz-target", "DynamoDB_20120810.ListTables")
                .header("content-type", "application/x-amz-json-1.0")
                .body("{}")
                .send()
                .await
        }
        [svc, op] if svc == "firehose" && op == "list-delivery-streams" => {
            client
                .post(endpoint)
                .header("x-amz-target", "Firehose_20150804.ListDeliveryStreams")
                .header("content-type", "application/x-amz-json-1.1")
                .body("{}")
                .send()
                .await
        }
        [svc, op] if svc == "kinesis" && op == "list-streams" => {
            client
                .post(endpoint)
                .header("x-amz-target", "Kinesis_20131202.ListStreams")
                .header("content-type", "application/x-amz-json-1.1")
                .body("{}")
                .send()
                .await
        }
        [svc, op] if svc == "secretsmanager" && op == "list-secrets" => {
            client
                .post(endpoint)
                .header("x-amz-target", "secretsmanager.ListSecrets")
                .header("content-type", "application/x-amz-json-1.1")
                .body("{}")
                .send()
                .await
        }
        [svc, op] if svc == "iam" && op == "list-users" => {
            client
                .post(endpoint)
                .header("content-type", "application/x-www-form-urlencoded")
                .body("Action=ListUsers&Version=2010-05-08")
                .send()
                .await
        }
        [svc, op] if svc == "sns" && op == "list-topics" => {
            client
                .post(endpoint)
                .header("content-type", "application/x-www-form-urlencoded")
                .body("Action=ListTopics&Version=2010-03-31")
                .send()
                .await
        }
        [svc, op] if svc == "sts" && op == "get-caller-identity" => {
            client
                .post(endpoint)
                .header("content-type", "application/x-www-form-urlencoded")
                .body("Action=GetCallerIdentity&Version=2011-06-15")
                .send()
                .await
        }
        [svc, op] if svc == "s3api" && op == "list-buckets" => {
            client.get(format!("{endpoint}/")).send().await
        }
        _ => {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            return (elapsed_ms, None);
        }
    };

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let success = response.map(|resp| resp.status().is_success()).ok();
    (elapsed_ms, success)
}

async fn execute_aws_command(
    endpoint: &str,
    command: Vec<String>,
    lane_mode: BenchmarkLaneMode,
) -> (f64, std::io::Result<std::process::Output>) {
    let mut full = vec![
        "--endpoint-url".to_string(),
        endpoint.to_string(),
        "--region".to_string(),
        "us-east-1".to_string(),
        "--no-sign-request".to_string(),
    ];
    full.extend(command);
    if lane_mode == BenchmarkLaneMode::LowOverhead {
        full.push("--cli-read-timeout".to_string());
        full.push("2".to_string());
        full.push("--cli-connect-timeout".to_string());
        full.push("2".to_string());
    }

    let started = Instant::now();
    let aws_bin = resolve_aws_cli_binary();
    let output =
        tokio::task::spawn_blocking(move || Command::new(&aws_bin).args(&full).output()).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    match output {
        Ok(out) => (elapsed_ms, out),
        Err(err) => (elapsed_ms, Err(std::io::Error::other(err.to_string()))),
    }
}

fn resolve_aws_cli_binary() -> String {
    static AWS_BIN: OnceLock<String> = OnceLock::new();
    AWS_BIN
        .get_or_init(|| {
            if let Ok(path) = std::env::var("AWS_CLI_PATH")
                && !path.trim().is_empty()
                && Command::new(&path).arg("--version").output().is_ok()
            {
                return path;
            }
            let candidates = [
                "aws".to_string(),
                "/home/runner/.local/bin/aws".to_string(),
                "/home/jesse/.local/bin/aws".to_string(),
            ];
            for candidate in candidates {
                if Command::new(&candidate).arg("--version").output().is_ok() {
                    return candidate;
                }
            }
            "aws".to_string()
        })
        .clone()
}

fn capture_output_value(
    stdout: &str,
    context: &mut HashMap<String, String>,
    capture: &crate::parity::CaptureJson,
) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout)
        && let Some(value) = json.pointer(&capture.json_pointer)
    {
        if let Some(as_str) = value.as_str() {
            context.insert(capture.output_key.clone(), as_str.to_string());
        } else {
            context.insert(capture.output_key.clone(), value.to_string());
        }
    }
}

fn render_command(raw: &[String], context: &HashMap<String, String>) -> Vec<String> {
    raw.iter()
        .map(|part| {
            let mut rendered = part.clone();
            for (key, value) in context {
                rendered = rendered.replace(&format!("{{{{{key}}}}}"), value);
            }
            rendered
        })
        .collect()
}

fn compare_metrics(
    openstack: &BenchmarkMetrics,
    localstack: &BenchmarkMetrics,
) -> BenchmarkComparison {
    BenchmarkComparison {
        latency_p50_ratio: safe_ratio(openstack.latency_p50_ms, localstack.latency_p50_ms),
        latency_p95_ratio: safe_ratio(openstack.latency_p95_ms, localstack.latency_p95_ms),
        throughput_ratio: safe_ratio(
            openstack.throughput_ops_per_sec,
            localstack.throughput_ops_per_sec,
        ),
        latency_p50_delta_ms: openstack.latency_p50_ms - localstack.latency_p50_ms,
        latency_p95_delta_ms: openstack.latency_p95_ms - localstack.latency_p95_ms,
        throughput_delta_ops_per_sec: openstack.throughput_ops_per_sec
            - localstack.throughput_ops_per_sec,
    }
}

fn safe_ratio(left: f64, right: f64) -> Option<f64> {
    if right <= f64::EPSILON {
        return None;
    }
    Some(left / right)
}

fn summarize_results(results: &[BenchmarkScenarioResult]) -> BenchmarkSummary {
    let workload_matrix = service_workload_matrix();
    let mut p50_ratios = Vec::new();
    let mut p95_ratios = Vec::new();
    let mut p99_ratios = Vec::new();
    let mut throughput_ratios = Vec::new();
    let mut openstack_error_count = 0usize;
    let mut localstack_error_count = 0usize;
    let mut performance_scenarios = 0usize;
    let mut valid_performance_scenarios = 0usize;
    let mut invalid_performance_scenarios = 0usize;
    let mut coverage_scenarios = 0usize;
    let mut skipped_scenarios = 0usize;
    let mut invalid_reasons = Vec::new();
    let mut missing_required_role_count = 0usize;
    let mut per_service: BTreeMap<String, Vec<&BenchmarkScenarioResult>> = BTreeMap::new();

    for result in results {
        per_service
            .entry(result.service.clone())
            .or_default()
            .push(result);

        match result.scenario_class {
            BenchmarkScenarioClass::Coverage => coverage_scenarios += 1,
            BenchmarkScenarioClass::Performance => {
                performance_scenarios += 1;
                if result.valid_for_performance {
                    valid_performance_scenarios += 1;
                } else {
                    invalid_performance_scenarios += 1;
                    if let Some(reason) = &result.invalid_reason {
                        invalid_reasons.push(format!("{}: {}", result.scenario_id, reason));
                    }
                }
            }
        }
        if result.skipped {
            skipped_scenarios += 1;
        }

        openstack_error_count += result.openstack.metrics.error_count;
        localstack_error_count += result.localstack.metrics.error_count;

        if result.valid_for_performance {
            if let Some(value) = result.comparison.latency_p50_ratio {
                p50_ratios.push(value);
            }
            if let Some(value) = result.comparison.latency_p95_ratio {
                p95_ratios.push(value);
            }
            if let Some(value) = safe_ratio(
                result.openstack.metrics.latency_p99_ms,
                result.localstack.metrics.latency_p99_ms,
            ) {
                p99_ratios.push(value);
            }
            if let Some(value) = result.comparison.throughput_ratio {
                throughput_ratios.push(value);
            }
        }
    }

    let mut per_service_summary = BTreeMap::new();
    for (service, service_results) in per_service {
        let mut service_p50 = Vec::new();
        let mut service_p95 = Vec::new();
        let mut service_p99 = Vec::new();
        let mut service_tp = Vec::new();
        let mut service_skipped = 0usize;
        let mut service_openstack_errors = 0usize;
        let mut service_localstack_errors = 0usize;
        let required_roles = workload_matrix
            .get(&service)
            .map(|entry| entry.required_roles.clone())
            .unwrap_or_default();
        let mut covered_role_set = std::collections::BTreeSet::new();
        let mut role_exclusions = BTreeMap::new();
        let service_class = service_results
            .iter()
            .find_map(|result| result.service_execution_class);
        let service_durability = service_results
            .iter()
            .find_map(|result| result.service_durability_class);
        let mut class_envelope_breaches = Vec::new();

        for result in &service_results {
            if result.skipped {
                service_skipped += 1;
            }
            service_openstack_errors += result.openstack.metrics.error_count;
            service_localstack_errors += result.localstack.metrics.error_count;

            if result.valid_for_performance {
                covered_role_set.insert(result.scenario_role);
                if let Some(v) = result.comparison.latency_p50_ratio {
                    service_p50.push(v);
                }
                if let Some(v) = result.comparison.latency_p95_ratio {
                    service_p95.push(v);
                }
                let p99 = safe_ratio(
                    result.openstack.metrics.latency_p99_ms,
                    result.localstack.metrics.latency_p99_ms,
                );
                if let Some(v) = p99 {
                    service_p99.push(v);
                }
                if let Some(v) = result.comparison.throughput_ratio {
                    service_tp.push(v);
                }
            }

            if let Some(reason) = &result.invalid_reason
                && reason.starts_with("class-envelope-")
            {
                class_envelope_breaches.push(format!("{}:{}", result.scenario_id, reason));
            }

            if result.skipped
                && let Some(reason) = &result.skip_reason
                && !reason.is_empty()
            {
                role_exclusions.insert(
                    format!("{:?}", result.scenario_role).to_ascii_lowercase(),
                    reason.clone(),
                );
            }
        }

        let covered_roles = covered_role_set.into_iter().collect::<Vec<_>>();
        let missing_roles = required_roles
            .iter()
            .copied()
            .filter(|role| !covered_roles.contains(role))
            .collect::<Vec<_>>();
        missing_required_role_count += missing_roles.len();

        per_service_summary.insert(
            service,
            BenchmarkServiceSummary {
                service_execution_class: service_class,
                service_durability_class: service_durability,
                total_scenarios: service_results.len(),
                skipped_scenarios: service_skipped,
                openstack_error_count: service_openstack_errors,
                localstack_error_count: service_localstack_errors,
                avg_latency_p50_ratio: average(&service_p50),
                avg_latency_p95_ratio: average(&service_p95),
                avg_latency_p99_ratio: average(&service_p99),
                avg_throughput_ratio: average(&service_tp),
                required_roles,
                covered_roles,
                missing_roles,
                role_exclusions,
                class_envelope_breaches,
            },
        );
    }

    BenchmarkSummary {
        total_scenarios: results.len(),
        performance_scenarios,
        valid_performance_scenarios,
        invalid_performance_scenarios,
        coverage_scenarios,
        skipped_scenarios,
        lane_interpretable: valid_performance_scenarios > 0,
        invalid_reasons,
        openstack_error_count,
        localstack_error_count,
        avg_latency_p50_ratio: average(&p50_ratios),
        avg_latency_p95_ratio: average(&p95_ratios),
        avg_latency_p99_ratio: average(&p99_ratios),
        avg_throughput_ratio: average(&throughput_ratios),
        missing_required_role_count,
        per_service: per_service_summary,
    }
}

fn enforce_required_role_completeness(summary: &mut BenchmarkSummary) {
    let mut missing_count = 0usize;
    let mut coverage_reasons = Vec::new();

    for (service, service_summary) in &summary.per_service {
        for missing_role in &service_summary.missing_roles {
            missing_count += 1;
            coverage_reasons.push(format!(
                "{service}: missing {:?} role coverage",
                missing_role
            ));
        }
    }

    summary.missing_required_role_count = missing_count;
    if missing_count > 0 {
        summary.lane_interpretable = false;
        summary.invalid_reasons.extend(coverage_reasons);
    }
}

fn service_workload_matrix() -> BTreeMap<String, ServiceWorkloadMatrixEntry> {
    all_service_names()
        .into_iter()
        .map(|service| {
            (
                service,
                ServiceWorkloadMatrixEntry {
                    required_roles: vec![BenchmarkScenarioRole::Write, BenchmarkScenarioRole::Read],
                },
            )
        })
        .collect()
}

fn validate_service_workload_matrix(services: &[String]) -> anyhow::Result<()> {
    let matrix = service_workload_matrix();
    let supported_services = all_service_names();
    let mut missing_services = Vec::new();
    let mut invalid_entries = Vec::new();

    for service in &supported_services {
        if !matrix.contains_key(service) {
            missing_services.push(service.clone());
        }
    }

    for service in services {
        let Some(entry) = matrix.get(service) else {
            missing_services.push(service.clone());
            continue;
        };

        if entry.required_roles.is_empty() {
            invalid_entries.push(format!("{service}: no required roles configured"));
            continue;
        }

        let has_write = entry.required_roles.contains(&BenchmarkScenarioRole::Write);
        let has_read = entry.required_roles.contains(&BenchmarkScenarioRole::Read);
        if !has_write || !has_read {
            invalid_entries.push(format!(
                "{service}: required roles must include write and read (got: {:?})",
                entry.required_roles
            ));
        }
    }

    if missing_services.is_empty() && invalid_entries.is_empty() {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "invalid service workload matrix (missing services: {:?}, invalid entries: {:?})",
        missing_services,
        invalid_entries
    ))
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn min_value(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::min).unwrap_or(0.0)
}

fn max_value(values: &[f64]) -> f64 {
    values.iter().copied().reduce(f64::max).unwrap_or(0.0)
}

fn stddev(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let rank = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn localstack_version_from_image(image: &str) -> Option<String> {
    image.split(':').nth(1).map(|v| v.to_string())
}

fn parse_heavy_s3_size_bytes(scenario_id: &str) -> Option<u64> {
    if !scenario_id.starts_with("s3-heavy-") {
        return None;
    }
    let size_part = scenario_id.strip_prefix("s3-heavy-")?;
    parse_size_bytes(size_part)
}

fn all_service_names() -> Vec<String> {
    vec![
        "s3",
        "sqs",
        "sns",
        "dynamodb",
        "iam",
        "sts",
        "kms",
        "secretsmanager",
        "ssm",
        "acm",
        "kinesis",
        "firehose",
        "cloudwatch",
        "events",
        "states",
        "apigateway",
        "ec2",
        "route53",
        "ses",
        "ecr",
        "opensearch",
        "redshift",
        "cloudformation",
        "lambda",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}

fn load_profile_scenarios(profile_name: &str, run_id: &str) -> Vec<BenchmarkScenario> {
    let mut scenarios = default_benchmark_scenarios(run_id);
    let path = resolve_scenarios_path(profile_name);
    if path.exists()
        && let Ok(content) = std::fs::read_to_string(&path)
        && let Ok(mut external_scenarios) = serde_json::from_str::<Vec<BenchmarkScenario>>(&content)
    {
        let default_ids = scenarios
            .iter()
            .map(|scenario| scenario.id.clone())
            .collect::<std::collections::HashSet<_>>();

        for scenario in &mut external_scenarios {
            inject_run_context(scenario, run_id);
            normalize_scenario_metadata(scenario);
        }

        scenarios.extend(
            external_scenarios
                .into_iter()
                .filter(|scenario| !default_ids.contains(&scenario.id)),
        );
    } else if profile_name == "fair-extreme" {
        let fallback = resolve_scenarios_path("fair-extreme");
        if fallback.exists()
            && let Ok(content) = std::fs::read_to_string(&fallback)
            && let Ok(mut external_scenarios) =
                serde_json::from_str::<Vec<BenchmarkScenario>>(&content)
        {
            for scenario in &mut external_scenarios {
                inject_run_context(scenario, run_id);
                normalize_scenario_metadata(scenario);
            }
            scenarios = external_scenarios;
        }
    }

    for scenario in &mut scenarios {
        normalize_scenario_metadata(scenario);
    }

    scenarios
}

fn resolve_scenarios_path(profile_name: &str) -> PathBuf {
    let relative = format!("tests/benchmark/scenarios/{profile_name}.json");
    let direct = PathBuf::from(&relative);
    if direct.exists() {
        return direct;
    }

    let workspace_relative = PathBuf::from(format!("../../../{relative}"));
    if workspace_relative.exists() {
        return workspace_relative;
    }

    direct
}

fn profile_matches(selected: &str, scenario_profile: &str) -> bool {
    match selected {
        "all-services-smoke-fast" => {
            scenario_profile == "all-services-smoke"
                || scenario_profile == "all-services-smoke-fast"
                || scenario_profile == "all-services-realistic"
        }
        "fair-low" => {
            scenario_profile == "all-services-smoke"
                || scenario_profile == "all-services-smoke-fast"
                || scenario_profile == "all-services-realistic"
        }
        "fair-low-core" => {
            scenario_profile == "all-services-smoke"
                || scenario_profile == "all-services-smoke-fast"
                || scenario_profile == "fair-low-core"
        }
        "fair-medium" => {
            scenario_profile == "all-services-smoke"
                || scenario_profile == "all-services-smoke-fast"
                || scenario_profile == "all-services-realistic"
        }
        "fair-medium-core" => {
            scenario_profile == "all-services-smoke"
                || scenario_profile == "all-services-smoke-fast"
                || scenario_profile == "fair-medium-core"
        }
        "fair-high" => {
            scenario_profile == "hot-path-deep" || scenario_profile == "all-services-smoke"
        }
        "fair-extreme" => scenario_profile == "hot-path-deep" || scenario_profile == "fair-extreme",
        _ => selected == scenario_profile,
    }
}

fn inject_run_context(scenario: &mut BenchmarkScenario, run_id: &str) {
    let replacements = [
        ("{{run_id}}", run_id.to_string()),
        ("{{bucket}}", format!("bench-bucket-{run_id}")),
        ("{{queue}}", format!("bench-queue-{run_id}")),
        ("{{table}}", format!("bench-table-{run_id}")),
    ];

    for step in scenario
        .setup
        .iter_mut()
        .chain(std::iter::once(&mut scenario.operation))
        .chain(scenario.cleanup.iter_mut())
    {
        for part in &mut step.command {
            for (needle, value) in &replacements {
                *part = part.replace(needle, value);
            }
        }
    }
}

fn normalize_scenario_metadata(scenario: &mut BenchmarkScenario) {
    apply_heavy_object_path_override(scenario);

    if scenario.scenario_role == BenchmarkScenarioRole::Aux {
        let id = scenario.id.to_ascii_lowercase();
        scenario.scenario_role = if id.contains("list")
            || id.contains("get")
            || id.contains("describe")
            || id.contains("read")
            || id.contains("query")
        {
            BenchmarkScenarioRole::Read
        } else if id.contains("create")
            || id.contains("put")
            || id.contains("send")
            || id.contains("write")
            || id.contains("publish")
            || id.contains("start")
            || id.contains("delete")
            || id.contains("update")
        {
            BenchmarkScenarioRole::Write
        } else {
            BenchmarkScenarioRole::Aux
        };
    }

    if scenario.id.contains("probe") {
        scenario.scenario_class = BenchmarkScenarioClass::Coverage;
        if scenario.load_tier == BenchmarkLoadTier::Medium {
            scenario.load_tier = BenchmarkLoadTier::Low;
        }
        return;
    }

    if scenario.profile.contains("realistic") {
        scenario.scenario_class = BenchmarkScenarioClass::Performance;
        if scenario.load_tier == BenchmarkLoadTier::Medium {
            scenario.load_tier = BenchmarkLoadTier::Low;
        }
        return;
    }

    if scenario.id.contains("heavy") {
        scenario.scenario_class = BenchmarkScenarioClass::Performance;
        scenario.load_tier = BenchmarkLoadTier::Extreme;
        return;
    }

    if scenario.profile.contains("fast") {
        scenario.scenario_class = BenchmarkScenarioClass::Performance;
        if scenario.load_tier == BenchmarkLoadTier::Medium {
            scenario.load_tier = BenchmarkLoadTier::Low;
        }
        return;
    }

    if scenario.profile.contains("core") {
        scenario.scenario_class = BenchmarkScenarioClass::Performance;
        if scenario.load_tier == BenchmarkLoadTier::Medium {
            scenario.load_tier = BenchmarkLoadTier::Low;
        }
        return;
    }

    if scenario.profile.contains("deep") {
        scenario.scenario_class = BenchmarkScenarioClass::Performance;
        if scenario.load_tier == BenchmarkLoadTier::Medium {
            scenario.load_tier = BenchmarkLoadTier::High;
        }
    }
}

fn apply_heavy_object_path_override(scenario: &mut BenchmarkScenario) {
    let Some(size_bytes) = parse_heavy_s3_size_bytes(&scenario.id) else {
        return;
    };
    let override_path = benchmark_file_for_size(size_bytes);
    if let Some(body_idx) = scenario
        .operation
        .command
        .iter()
        .position(|part| part == "--body")
        && let Some(path_arg) = scenario.operation.command.get_mut(body_idx + 1)
    {
        *path_arg = override_path;
    }
}

fn scenario_step(id: &str, protocol: ProtocolFamily, command: Vec<String>) -> ScenarioStep {
    ScenarioStep {
        id: id.to_string(),
        protocol,
        command,
        expect_success: true,
        capture_json: None,
    }
}

pub fn default_benchmark_scenarios(_run_id: &str) -> Vec<BenchmarkScenario> {
    let mut scenarios = Vec::new();

    for service in all_service_names() {
        let Some(((write_protocol, write_command), (read_protocol, read_command))) =
            default_read_write_commands_for_service(&service)
        else {
            continue;
        };

        let (setup, cleanup) = setup_cleanup_for_service(&service);

        scenarios.push(BenchmarkScenario {
            id: format!("{service}-write-performance"),
            profile: "all-services-realistic".to_string(),
            service: service.clone(),
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::Low,
            scenario_role: BenchmarkScenarioRole::Write,
            protocol: write_protocol.clone(),
            setup: setup.clone(),
            operation: scenario_step("service-write-op", write_protocol, write_command),
            cleanup: cleanup.clone(),
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        });

        scenarios.push(BenchmarkScenario {
            id: format!("{service}-read-performance"),
            profile: "all-services-realistic".to_string(),
            service,
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::Low,
            scenario_role: BenchmarkScenarioRole::Read,
            protocol: read_protocol.clone(),
            setup,
            operation: scenario_step("service-read-op", read_protocol, read_command),
            cleanup,
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        });
    }

    scenarios
}

fn setup_cleanup_for_service(service: &str) -> (Vec<ScenarioStep>, Vec<ScenarioStep>) {
    match service {
        "s3" => (
            vec![scenario_step(
                "s3-create-bucket",
                ProtocolFamily::RestXml,
                vec![
                    "s3api".into(),
                    "create-bucket".into(),
                    "--bucket".into(),
                    "{{bucket}}".into(),
                ],
            )],
            vec![scenario_step(
                "s3-delete-bucket",
                ProtocolFamily::RestXml,
                vec![
                    "s3api".into(),
                    "delete-bucket".into(),
                    "--bucket".into(),
                    "{{bucket}}".into(),
                ],
            )],
        ),
        "dynamodb" => (
            vec![scenario_step(
                "ddb-create-table",
                ProtocolFamily::Json,
                vec![
                    "dynamodb".into(),
                    "create-table".into(),
                    "--table-name".into(),
                    "{{table}}".into(),
                    "--attribute-definitions".into(),
                    "AttributeName=id,AttributeType=S".into(),
                    "--key-schema".into(),
                    "AttributeName=id,KeyType=HASH".into(),
                    "--billing-mode".into(),
                    "PAY_PER_REQUEST".into(),
                ],
            )],
            vec![scenario_step(
                "ddb-delete-table",
                ProtocolFamily::Json,
                vec![
                    "dynamodb".into(),
                    "delete-table".into(),
                    "--table-name".into(),
                    "{{table}}".into(),
                ],
            )],
        ),
        "sqs" => (
            vec![scenario_step(
                "sqs-create-queue",
                ProtocolFamily::QueryXml,
                vec![
                    "sqs".into(),
                    "create-queue".into(),
                    "--queue-name".into(),
                    "{{queue}}".into(),
                ],
            )],
            vec![],
        ),
        "sns" => (
            vec![scenario_step(
                "sns-create-topic",
                ProtocolFamily::QueryXml,
                vec![
                    "sns".into(),
                    "create-topic".into(),
                    "--name".into(),
                    "bench-topic-{{run_id}}".into(),
                ],
            )],
            vec![],
        ),
        _ => (vec![], vec![]),
    }
}

pub fn ensure_profile_scenarios(profile_name: &str) -> anyhow::Result<()> {
    let path = resolve_scenarios_path(profile_name);
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let scenarios = match profile_name {
        "all-services-smoke" => default_benchmark_scenarios("{{run_id}}"),
        "hot-path-deep" => vec![
            BenchmarkScenario {
                id: "s3-put-large-object".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "s3".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::High,
                scenario_role: BenchmarkScenarioRole::Write,
                protocol: ProtocolFamily::RestXml,
                setup: vec![scenario_step(
                    "s3-create-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "create-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                operation: scenario_step(
                    "s3-put-object-large",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "put-object".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                        "--key".into(),
                        "deep-{{run_id}}.txt".into(),
                        "--body".into(),
                        "README.md".into(),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "s3-delete-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "delete-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                warmup_iterations: Some(3),
                measured_iterations: Some(8),
                operations_per_iteration: Some(50),
                concurrency: Some(8),
            },
            BenchmarkScenario {
                id: "sqs-send-burst".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "sqs".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::High,
                scenario_role: BenchmarkScenarioRole::Write,
                protocol: ProtocolFamily::QueryXml,
                setup: vec![scenario_step(
                    "sqs-create-queue",
                    ProtocolFamily::QueryXml,
                    vec![
                        "sqs".into(),
                        "create-queue".into(),
                        "--queue-name".into(),
                        "{{queue}}".into(),
                    ],
                )],
                operation: scenario_step(
                    "sqs-send-message-burst",
                    ProtocolFamily::QueryXml,
                    vec![
                        "sqs".into(),
                        "send-message".into(),
                        "--queue-url".into(),
                        "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}".into(),
                        "--message-body".into(),
                        "deep-{{run_id}}".into(),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "sqs-delete-queue",
                    ProtocolFamily::QueryXml,
                    vec![
                        "sqs".into(),
                        "delete-queue".into(),
                        "--queue-url".into(),
                        "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}".into(),
                    ],
                )],
                warmup_iterations: Some(2),
                measured_iterations: Some(8),
                operations_per_iteration: Some(40),
                concurrency: Some(10),
            },
            BenchmarkScenario {
                id: "ddb-read-hot-key".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "dynamodb".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::High,
                scenario_role: BenchmarkScenarioRole::Read,
                protocol: ProtocolFamily::Json,
                setup: vec![
                    scenario_step(
                        "ddb-create-table",
                        ProtocolFamily::Json,
                        vec![
                            "dynamodb".into(),
                            "create-table".into(),
                            "--table-name".into(),
                            "{{table}}".into(),
                            "--attribute-definitions".into(),
                            "AttributeName=pk,AttributeType=S".into(),
                            "--key-schema".into(),
                            "AttributeName=pk,KeyType=HASH".into(),
                            "--billing-mode".into(),
                            "PAY_PER_REQUEST".into(),
                        ],
                    ),
                    scenario_step(
                        "ddb-put-item",
                        ProtocolFamily::Json,
                        vec![
                            "dynamodb".into(),
                            "put-item".into(),
                            "--table-name".into(),
                            "{{table}}".into(),
                            "--item".into(),
                            "{\"pk\":{\"S\":\"hot\"},\"value\":{\"S\":\"v\"}}".into(),
                        ],
                    ),
                ],
                operation: scenario_step(
                    "ddb-get-item-hot",
                    ProtocolFamily::Json,
                    vec![
                        "dynamodb".into(),
                        "get-item".into(),
                        "--table-name".into(),
                        "{{table}}".into(),
                        "--key".into(),
                        "{\"pk\":{\"S\":\"hot\"}}".into(),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "ddb-delete-table",
                    ProtocolFamily::Json,
                    vec![
                        "dynamodb".into(),
                        "delete-table".into(),
                        "--table-name".into(),
                        "{{table}}".into(),
                    ],
                )],
                warmup_iterations: Some(3),
                measured_iterations: Some(8),
                operations_per_iteration: Some(50),
                concurrency: Some(10),
            },
            BenchmarkScenario {
                id: "s3-heavy-1gb".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "s3".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::Extreme,
                scenario_role: BenchmarkScenarioRole::Write,
                protocol: ProtocolFamily::RestXml,
                setup: vec![scenario_step(
                    "s3-create-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "create-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                operation: scenario_step(
                    "s3-put-object-heavy-1gb",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "put-object".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                        "--key".into(),
                        "heavy-1gb-{{run_id}}.bin".into(),
                        "--body".into(),
                        benchmark_file_for_size(1024 * 1024 * 1024),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "s3-delete-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "delete-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                warmup_iterations: Some(0),
                measured_iterations: Some(1),
                operations_per_iteration: Some(1),
                concurrency: Some(1),
            },
            BenchmarkScenario {
                id: "s3-heavy-5gb".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "s3".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::Extreme,
                scenario_role: BenchmarkScenarioRole::Write,
                protocol: ProtocolFamily::RestXml,
                setup: vec![scenario_step(
                    "s3-create-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "create-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                operation: scenario_step(
                    "s3-put-object-heavy-5gb",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "put-object".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                        "--key".into(),
                        "heavy-5gb-{{run_id}}.bin".into(),
                        "--body".into(),
                        benchmark_file_for_size(5 * 1024 * 1024 * 1024),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "s3-delete-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "delete-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                warmup_iterations: Some(0),
                measured_iterations: Some(1),
                operations_per_iteration: Some(1),
                concurrency: Some(1),
            },
            BenchmarkScenario {
                id: "s3-heavy-10gb".to_string(),
                profile: "hot-path-deep".to_string(),
                service: "s3".to_string(),
                scenario_class: BenchmarkScenarioClass::Performance,
                load_tier: BenchmarkLoadTier::Extreme,
                scenario_role: BenchmarkScenarioRole::Write,
                protocol: ProtocolFamily::RestXml,
                setup: vec![scenario_step(
                    "s3-create-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "create-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                operation: scenario_step(
                    "s3-put-object-heavy-10gb",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "put-object".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                        "--key".into(),
                        "heavy-10gb-{{run_id}}.bin".into(),
                        "--body".into(),
                        benchmark_file_for_size(10 * 1024 * 1024 * 1024),
                    ],
                ),
                cleanup: vec![scenario_step(
                    "s3-delete-bucket",
                    ProtocolFamily::RestXml,
                    vec![
                        "s3api".into(),
                        "delete-bucket".into(),
                        "--bucket".into(),
                        "{{bucket}}".into(),
                    ],
                )],
                warmup_iterations: Some(0),
                measured_iterations: Some(1),
                operations_per_iteration: Some(1),
                concurrency: Some(1),
            },
        ],
        other => return Err(anyhow::anyhow!("unknown profile for scenario generation: {other}")),
    };

    let json = serde_json::to_string_pretty(&scenarios)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        BenchmarkLoadTier, BenchmarkMetrics, BenchmarkScenarioClass, BenchmarkScenarioRole,
        all_service_names, compare_metrics, default_benchmark_scenarios, load_profile_scenarios,
        parse_docker_mem_value_to_bytes, parse_heavy_s3_size_bytes, percentile,
        performance_invalid_reason, summarize_results,
    };
    use crate::benchmark::{
        BenchmarkComparison, BenchmarkConfig, BenchmarkMemorySample, BenchmarkMemorySummary,
        BenchmarkRunConfig, BenchmarkScenarioResult, BenchmarkServiceSummary,
        BenchmarkStartupSample, BenchmarkStartupSummary, BenchmarkSummary, BenchmarkTargetMetadata,
        BenchmarkTargetResult, enforce_required_role_completeness,
    };
    use crate::classification::{PersistenceMode, ServiceExecutionClass};

    #[test]
    fn computes_percentiles() {
        let values = vec![1.0, 5.0, 2.0, 9.0, 7.0];
        assert_eq!(percentile(&values, 0.50), 5.0);
        assert_eq!(percentile(&values, 0.95), 9.0);
    }

    #[test]
    fn computes_comparative_ratios_and_deltas() {
        let openstack = BenchmarkMetrics {
            latency_p50_ms: 10.0,
            latency_p95_ms: 20.0,
            latency_p99_ms: 30.0,
            latency_min_ms: 5.0,
            latency_max_ms: 31.0,
            latency_stddev_ms: 2.0,
            throughput_ops_per_sec: 50.0,
            operation_count: 100,
            error_count: 0,
            success_rate: 1.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 1000.0,
        };
        let localstack = BenchmarkMetrics {
            latency_p50_ms: 8.0,
            latency_p95_ms: 16.0,
            latency_p99_ms: 24.0,
            latency_min_ms: 4.0,
            latency_max_ms: 26.0,
            latency_stddev_ms: 1.5,
            throughput_ops_per_sec: 40.0,
            operation_count: 100,
            error_count: 1,
            success_rate: 0.99,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 1000.0,
        };
        let comparison = compare_metrics(&openstack, &localstack);
        assert_eq!(comparison.latency_p50_ratio, Some(1.25));
        assert_eq!(comparison.latency_p95_ratio, Some(1.25));
        assert_eq!(comparison.throughput_ratio, Some(1.25));
        assert_eq!(comparison.latency_p50_delta_ms, 2.0);
        assert_eq!(comparison.throughput_delta_ops_per_sec, 10.0);
    }

    #[test]
    fn summarizes_across_scenarios() {
        let metadata = BenchmarkTargetMetadata {
            endpoint: "http://127.0.0.1:4566".to_string(),
            runtime: "docker".to_string(),
            image: Some("ghcr.io/jessekoldewijn/openstack:latest".to_string()),
            cpu_limit: Some("2".to_string()),
            memory_limit: Some("4g".to_string()),
            network_mode: Some("bridge".to_string()),
            localstack_image: Some("localstack/localstack:3.7.2".to_string()),
            localstack_version: Some("3.7.2".to_string()),
        };
        let run_config = BenchmarkRunConfig {
            warmup_iterations: 1,
            measured_iterations: 2,
            operations_per_iteration: 3,
            concurrency: 1,
        };
        let results = vec![BenchmarkScenarioResult {
            scenario_id: "x".to_string(),
            service: "s3".to_string(),
            service_execution_class: None,
            service_durability_class: None,
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::Low,
            scenario_role: BenchmarkScenarioRole::Read,
            skipped: false,
            skip_reason: None,
            valid_for_performance: true,
            invalid_reason: None,
            run_config,
            openstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: BenchmarkMetrics {
                    latency_p50_ms: 10.0,
                    latency_p95_ms: 20.0,
                    latency_p99_ms: 25.0,
                    latency_min_ms: 8.0,
                    latency_max_ms: 28.0,
                    latency_stddev_ms: 2.0,
                    throughput_ops_per_sec: 30.0,
                    operation_count: 6,
                    error_count: 1,
                    success_rate: 5.0 / 6.0,
                    timeout_count: 0,
                    retry_count: 0,
                    total_duration_ms: 500.0,
                },
            },
            localstack: BenchmarkTargetResult {
                metadata,
                metrics: BenchmarkMetrics {
                    latency_p50_ms: 8.0,
                    latency_p95_ms: 18.0,
                    latency_p99_ms: 22.0,
                    latency_min_ms: 6.0,
                    latency_max_ms: 25.0,
                    latency_stddev_ms: 1.5,
                    throughput_ops_per_sec: 27.0,
                    operation_count: 6,
                    error_count: 2,
                    success_rate: 4.0 / 6.0,
                    timeout_count: 0,
                    retry_count: 0,
                    total_duration_ms: 520.0,
                },
            },
            comparison: BenchmarkComparison {
                latency_p50_ratio: Some(1.25),
                latency_p95_ratio: Some(1.11),
                throughput_ratio: Some(1.11),
                latency_p50_delta_ms: 2.0,
                latency_p95_delta_ms: 2.0,
                throughput_delta_ops_per_sec: 3.0,
            },
        }];

        let summary = summarize_results(&results);
        assert_eq!(summary.total_scenarios, 1);
        assert_eq!(summary.performance_scenarios, 1);
        assert_eq!(summary.valid_performance_scenarios, 1);
        assert_eq!(summary.invalid_performance_scenarios, 0);
        assert_eq!(summary.coverage_scenarios, 0);
        assert_eq!(summary.skipped_scenarios, 0);
        assert!(summary.lane_interpretable);
        assert_eq!(summary.openstack_error_count, 1);
        assert_eq!(summary.localstack_error_count, 2);
        assert_eq!(summary.avg_latency_p50_ratio, Some(1.25));
        assert_eq!(summary.missing_required_role_count, 1);
        assert!(summary.avg_latency_p99_ratio.is_some());
        let s3 = summary
            .per_service
            .get("s3")
            .expect("s3 service summary should exist");
        assert_eq!(s3.total_scenarios, 1);
        assert_eq!(s3.avg_latency_p95_ratio, Some(1.11));
    }

    #[test]
    fn summary_excludes_coverage_and_skipped_from_ratio_rollups() {
        let metadata = BenchmarkTargetMetadata {
            endpoint: "http://127.0.0.1:4566".to_string(),
            runtime: "docker".to_string(),
            image: None,
            cpu_limit: None,
            memory_limit: None,
            network_mode: Some("bridge".to_string()),
            localstack_image: Some("localstack/localstack:3.7.2".to_string()),
            localstack_version: Some("3.7.2".to_string()),
        };

        let metrics = BenchmarkMetrics {
            latency_p50_ms: 10.0,
            latency_p95_ms: 20.0,
            latency_p99_ms: 30.0,
            latency_min_ms: 5.0,
            latency_max_ms: 31.0,
            latency_stddev_ms: 2.0,
            throughput_ops_per_sec: 50.0,
            operation_count: 100,
            error_count: 0,
            success_rate: 1.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 1000.0,
        };

        let perf = BenchmarkScenarioResult {
            scenario_id: "perf".to_string(),
            service: "s3".to_string(),
            service_execution_class: None,
            service_durability_class: None,
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::High,
            scenario_role: BenchmarkScenarioRole::Read,
            skipped: false,
            skip_reason: None,
            valid_for_performance: true,
            invalid_reason: None,
            run_config: BenchmarkRunConfig {
                warmup_iterations: 1,
                measured_iterations: 2,
                operations_per_iteration: 3,
                concurrency: 1,
            },
            openstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: metrics.clone(),
            },
            localstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: BenchmarkMetrics {
                    throughput_ops_per_sec: 25.0,
                    latency_p50_ms: 5.0,
                    latency_p95_ms: 10.0,
                    ..metrics.clone()
                },
            },
            comparison: BenchmarkComparison {
                latency_p50_ratio: Some(2.0),
                latency_p95_ratio: Some(2.0),
                throughput_ratio: Some(2.0),
                latency_p50_delta_ms: 5.0,
                latency_p95_delta_ms: 10.0,
                throughput_delta_ops_per_sec: 25.0,
            },
        };

        let mut coverage = perf.clone();
        coverage.scenario_id = "coverage".to_string();
        coverage.scenario_class = BenchmarkScenarioClass::Coverage;
        coverage.valid_for_performance = false;
        coverage.invalid_reason = None;
        coverage.comparison.latency_p50_ratio = Some(100.0);
        coverage.comparison.latency_p95_ratio = Some(100.0);
        coverage.comparison.throughput_ratio = Some(100.0);

        let mut skipped = perf.clone();
        skipped.scenario_id = "skipped".to_string();
        skipped.skipped = true;
        skipped.skip_reason = Some("fixture missing".to_string());
        skipped.valid_for_performance = false;
        skipped.invalid_reason = Some("fixture missing".to_string());
        skipped.comparison.latency_p50_ratio = Some(100.0);
        skipped.comparison.latency_p95_ratio = Some(100.0);
        skipped.comparison.throughput_ratio = Some(100.0);

        let summary: BenchmarkSummary = summarize_results(&[perf, coverage, skipped]);
        assert_eq!(summary.total_scenarios, 3);
        assert_eq!(summary.performance_scenarios, 2);
        assert_eq!(summary.valid_performance_scenarios, 1);
        assert_eq!(summary.invalid_performance_scenarios, 1);
        assert_eq!(summary.coverage_scenarios, 1);
        assert_eq!(summary.skipped_scenarios, 1);
        assert!(summary.lane_interpretable);
        assert_eq!(summary.invalid_reasons.len(), 1);
        assert_eq!(summary.avg_latency_p50_ratio, Some(2.0));
        assert_eq!(summary.avg_latency_p95_ratio, Some(2.0));
        assert_eq!(summary.avg_latency_p99_ratio, Some(1.0));
        assert_eq!(summary.avg_throughput_ratio, Some(2.0));
        assert_eq!(summary.per_service.len(), 1);
        let s3 = summary
            .per_service
            .get("s3")
            .expect("s3 service summary should exist");
        assert_eq!(s3.total_scenarios, 3);
        assert_eq!(s3.skipped_scenarios, 1);
        assert_eq!(s3.avg_latency_p50_ratio, Some(2.0));
        assert_eq!(s3.avg_latency_p95_ratio, Some(2.0));
        assert_eq!(s3.avg_latency_p99_ratio, Some(1.0));
        assert_eq!(s3.avg_throughput_ratio, Some(2.0));
    }

    #[test]
    fn per_service_ratios_exclude_invalid_performance_scenarios() {
        let metadata = BenchmarkTargetMetadata {
            endpoint: "http://127.0.0.1:4566".to_string(),
            runtime: "docker".to_string(),
            image: None,
            cpu_limit: None,
            memory_limit: None,
            network_mode: Some("bridge".to_string()),
            localstack_image: Some("localstack/localstack:3.7.2".to_string()),
            localstack_version: Some("3.7.2".to_string()),
        };

        let valid = BenchmarkScenarioResult {
            scenario_id: "valid".to_string(),
            service: "s3".to_string(),
            service_execution_class: None,
            service_durability_class: None,
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::Low,
            scenario_role: BenchmarkScenarioRole::Read,
            skipped: false,
            skip_reason: None,
            valid_for_performance: true,
            invalid_reason: None,
            run_config: BenchmarkRunConfig {
                warmup_iterations: 1,
                measured_iterations: 1,
                operations_per_iteration: 2,
                concurrency: 1,
            },
            openstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: BenchmarkMetrics {
                    latency_p50_ms: 10.0,
                    latency_p95_ms: 20.0,
                    latency_p99_ms: 30.0,
                    throughput_ops_per_sec: 50.0,
                    operation_count: 4,
                    error_count: 0,
                    ..BenchmarkMetrics::default()
                },
            },
            localstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: BenchmarkMetrics {
                    latency_p50_ms: 5.0,
                    latency_p95_ms: 10.0,
                    latency_p99_ms: 15.0,
                    throughput_ops_per_sec: 25.0,
                    operation_count: 4,
                    error_count: 0,
                    ..BenchmarkMetrics::default()
                },
            },
            comparison: BenchmarkComparison {
                latency_p50_ratio: Some(2.0),
                latency_p95_ratio: Some(2.0),
                throughput_ratio: Some(2.0),
                latency_p50_delta_ms: 5.0,
                latency_p95_delta_ms: 10.0,
                throughput_delta_ops_per_sec: 25.0,
            },
        };

        let mut invalid = valid.clone();
        invalid.scenario_id = "invalid".to_string();
        invalid.valid_for_performance = false;
        invalid.invalid_reason = Some("all operations failed".to_string());
        invalid.comparison.latency_p50_ratio = Some(100.0);
        invalid.comparison.latency_p95_ratio = Some(100.0);
        invalid.comparison.throughput_ratio = Some(100.0);
        invalid.openstack.metrics.latency_p99_ms = 3000.0;
        invalid.localstack.metrics.latency_p99_ms = 1.0;

        let summary = summarize_results(&[valid, invalid]);
        let s3 = summary
            .per_service
            .get("s3")
            .expect("s3 service summary should exist");

        assert_eq!(summary.valid_performance_scenarios, 1);
        assert_eq!(summary.invalid_performance_scenarios, 1);
        assert_eq!(s3.avg_latency_p50_ratio, Some(2.0));
        assert_eq!(s3.avg_latency_p95_ratio, Some(2.0));
        assert_eq!(s3.avg_latency_p99_ratio, Some(2.0));
        assert_eq!(s3.avg_throughput_ratio, Some(2.0));
    }

    #[test]
    fn parses_heavy_s3_size_ids() {
        assert_eq!(
            parse_heavy_s3_size_bytes("s3-heavy-1gb"),
            Some(1024 * 1024 * 1024)
        );
        assert_eq!(
            parse_heavy_s3_size_bytes("s3-heavy-5gb"),
            Some(5 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            parse_heavy_s3_size_bytes("s3-heavy-10gb"),
            Some(10 * 1024 * 1024 * 1024)
        );
        assert_eq!(parse_heavy_s3_size_bytes("s3-put-small-object"), None);
    }

    #[test]
    fn fair_extreme_profile_defines_1gb_5gb_10gb_scenarios() {
        let scenarios = load_profile_scenarios("fair-extreme", "test-run");
        let ids = scenarios
            .iter()
            .map(|scenario| scenario.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        assert!(ids.contains("s3-heavy-1gb"));
        assert!(ids.contains("s3-heavy-5gb"));
        assert!(ids.contains("s3-heavy-10gb"));
    }

    #[test]
    fn all_services_have_default_performance_scenarios() {
        let scenarios = default_benchmark_scenarios("seed");
        let services_in_scenarios = scenarios
            .iter()
            .map(|scenario| scenario.service.as_str())
            .collect::<std::collections::HashSet<_>>();

        for service in all_service_names() {
            assert!(
                services_in_scenarios.contains(service.as_str()),
                "missing performance scenario for service {service}"
            );
        }
    }

    #[test]
    fn all_services_have_default_write_and_read_scenarios() {
        let scenarios = default_benchmark_scenarios("seed");

        let mut per_service_roles: std::collections::HashMap<
            &str,
            std::collections::HashSet<BenchmarkScenarioRole>,
        > = std::collections::HashMap::new();
        for scenario in &scenarios {
            per_service_roles
                .entry(scenario.service.as_str())
                .or_default()
                .insert(scenario.scenario_role);
        }

        for service in all_service_names() {
            let roles = per_service_roles
                .get(service.as_str())
                .unwrap_or_else(|| panic!("missing scenarios for service {service}"));
            assert!(
                roles.contains(&BenchmarkScenarioRole::Write),
                "missing write scenario for service {service}"
            );
            assert!(
                roles.contains(&BenchmarkScenarioRole::Read),
                "missing read scenario for service {service}"
            );
        }
    }

    #[test]
    fn unknown_scenario_role_is_invalid_for_performance() {
        let reason = performance_invalid_reason(
            BenchmarkScenarioClass::Performance,
            BenchmarkScenarioRole::Aux,
            false,
            None,
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                ..BenchmarkMetrics::default()
            },
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                ..BenchmarkMetrics::default()
            },
            Some(ServiceExecutionClass::InProcStateful),
            &BenchmarkConfig::default(),
        );

        assert_eq!(reason.as_deref(), Some("unknown scenario role"));
    }

    #[test]
    fn role_completeness_enforcement_marks_lane_non_interpretable() {
        let mut summary = BenchmarkSummary {
            total_scenarios: 1,
            performance_scenarios: 1,
            valid_performance_scenarios: 1,
            invalid_performance_scenarios: 0,
            coverage_scenarios: 0,
            skipped_scenarios: 0,
            lane_interpretable: true,
            invalid_reasons: Vec::new(),
            openstack_error_count: 0,
            localstack_error_count: 0,
            avg_latency_p50_ratio: None,
            avg_latency_p95_ratio: None,
            avg_latency_p99_ratio: None,
            avg_throughput_ratio: None,
            missing_required_role_count: 0,
            per_service: {
                let mut map = BTreeMap::new();
                map.insert(
                    "s3".to_string(),
                    BenchmarkServiceSummary {
                        service_execution_class: Some(ServiceExecutionClass::InProcStateful),
                        service_durability_class: None,
                        total_scenarios: 1,
                        skipped_scenarios: 0,
                        openstack_error_count: 0,
                        localstack_error_count: 0,
                        avg_latency_p50_ratio: None,
                        avg_latency_p95_ratio: None,
                        avg_latency_p99_ratio: None,
                        avg_throughput_ratio: None,
                        required_roles: vec![
                            BenchmarkScenarioRole::Write,
                            BenchmarkScenarioRole::Read,
                        ],
                        covered_roles: vec![BenchmarkScenarioRole::Read],
                        missing_roles: vec![BenchmarkScenarioRole::Write],
                        role_exclusions: BTreeMap::new(),
                        class_envelope_breaches: Vec::new(),
                    },
                );
                map
            },
        };

        enforce_required_role_completeness(&mut summary);
        assert!(!summary.lane_interpretable);
        assert_eq!(summary.missing_required_role_count, 1);
        assert!(
            summary
                .invalid_reasons
                .iter()
                .any(|r| r.contains("missing Write role coverage")
                    || r.contains("missing write role coverage"))
        );
    }

    #[test]
    fn lane_not_interpretable_when_no_valid_performance_scenarios() {
        let metadata = BenchmarkTargetMetadata {
            endpoint: "http://127.0.0.1:4566".to_string(),
            runtime: "docker".to_string(),
            image: None,
            cpu_limit: None,
            memory_limit: None,
            network_mode: Some("bridge".to_string()),
            localstack_image: Some("localstack/localstack:3.7.2".to_string()),
            localstack_version: Some("3.7.2".to_string()),
        };
        let result = BenchmarkScenarioResult {
            scenario_id: "x".to_string(),
            service: "s3".to_string(),
            service_execution_class: None,
            service_durability_class: None,
            scenario_class: BenchmarkScenarioClass::Performance,
            load_tier: BenchmarkLoadTier::Low,
            scenario_role: BenchmarkScenarioRole::Read,
            skipped: true,
            skip_reason: Some("missing fixture".to_string()),
            valid_for_performance: false,
            invalid_reason: Some("missing fixture".to_string()),
            run_config: BenchmarkRunConfig {
                warmup_iterations: 0,
                measured_iterations: 1,
                operations_per_iteration: 1,
                concurrency: 1,
            },
            openstack: BenchmarkTargetResult {
                metadata: metadata.clone(),
                metrics: BenchmarkMetrics::default(),
            },
            localstack: BenchmarkTargetResult {
                metadata,
                metrics: BenchmarkMetrics::default(),
            },
            comparison: BenchmarkComparison {
                latency_p50_ratio: None,
                latency_p95_ratio: None,
                throughput_ratio: None,
                latency_p50_delta_ms: 0.0,
                latency_p95_delta_ms: 0.0,
                throughput_delta_ops_per_sec: 0.0,
            },
        };

        let summary = summarize_results(&[result]);
        assert!(!summary.lane_interpretable);
        assert_eq!(summary.valid_performance_scenarios, 0);
        assert_eq!(summary.invalid_performance_scenarios, 1);
    }

    #[test]
    fn invalid_when_only_one_target_has_successes() {
        let reason = performance_invalid_reason(
            BenchmarkScenarioClass::Performance,
            BenchmarkScenarioRole::Read,
            false,
            None,
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 8,
                ..BenchmarkMetrics::default()
            },
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                ..BenchmarkMetrics::default()
            },
            Some(ServiceExecutionClass::InProcStateful),
            &BenchmarkConfig::default(),
        );

        assert_eq!(
            reason.as_deref(),
            Some("insufficient cross-target successful operations")
        );
    }

    #[test]
    fn invalid_on_mode_mismatch() {
        let cfg = BenchmarkConfig {
            openstack_persistence_mode: PersistenceMode::Durable,
            localstack_persistence_mode: PersistenceMode::NonDurable,
            ..BenchmarkConfig::default()
        };

        let reason = performance_invalid_reason(
            BenchmarkScenarioClass::Performance,
            BenchmarkScenarioRole::Read,
            false,
            None,
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                latency_p95_ms: 10.0,
                latency_p99_ms: 15.0,
                throughput_ops_per_sec: 100.0,
                ..BenchmarkMetrics::default()
            },
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                latency_p95_ms: 10.0,
                latency_p99_ms: 15.0,
                throughput_ops_per_sec: 100.0,
                ..BenchmarkMetrics::default()
            },
            Some(ServiceExecutionClass::InProcStateful),
            &cfg,
        );

        assert_eq!(reason.as_deref(), Some("mode_mismatch"));
    }

    #[test]
    fn invalid_on_missing_service_class() {
        let cfg = BenchmarkConfig::default();
        let reason = performance_invalid_reason(
            BenchmarkScenarioClass::Performance,
            BenchmarkScenarioRole::Read,
            false,
            None,
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                ..BenchmarkMetrics::default()
            },
            &BenchmarkMetrics {
                operation_count: 8,
                error_count: 0,
                ..BenchmarkMetrics::default()
            },
            None,
            &cfg,
        );

        assert_eq!(reason.as_deref(), Some("missing_service_class"));
    }

    #[test]
    fn serializes_report_json() {
        let metrics = BenchmarkMetrics {
            latency_p50_ms: 1.0,
            latency_p95_ms: 2.0,
            latency_p99_ms: 3.0,
            latency_min_ms: 0.5,
            latency_max_ms: 4.0,
            latency_stddev_ms: 0.25,
            throughput_ops_per_sec: 3.0,
            operation_count: 4,
            error_count: 0,
            success_rate: 1.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 100.0,
        };
        let json = serde_json::to_string(&metrics).expect("metrics should serialize");
        assert!(json.contains("latency_p50_ms"));
        assert!(json.contains("throughput_ops_per_sec"));
    }

    #[test]
    fn startup_and_memory_summaries_serialize() {
        let startup = BenchmarkStartupSummary {
            openstack_avg_ms: Some(10.0),
            localstack_avg_ms: Some(20.0),
            startup_ratio_openstack_over_localstack: Some(0.5),
            samples: vec![BenchmarkStartupSample {
                target: "openstack".to_string(),
                startup_ms: 10.0,
            }],
            missing_targets: vec!["localstack".to_string()],
        };
        let memory = BenchmarkMemorySummary {
            openstack_idle_rss_bytes: Some(10),
            localstack_idle_rss_bytes: Some(20),
            openstack_rss_bytes: Some(30),
            localstack_rss_bytes: Some(40),
            rss_ratio_openstack_over_localstack: Some(0.75),
            missing_targets: vec![],
            samples: vec![BenchmarkMemorySample {
                target: "openstack".to_string(),
                rss_bytes: Some(30),
                raw_value: Some("30MiB".to_string()),
            }],
        };

        let startup_json = serde_json::to_string(&startup).expect("startup should serialize");
        let memory_json = serde_json::to_string(&memory).expect("memory should serialize");
        assert!(startup_json.contains("startup_ratio_openstack_over_localstack"));
        assert!(memory_json.contains("openstack_idle_rss_bytes"));
        assert!(memory_json.contains("missing_targets"));
    }

    #[test]
    fn parses_docker_memory_values() {
        assert_eq!(parse_docker_mem_value_to_bytes("512B"), Some(512));
        assert_eq!(parse_docker_mem_value_to_bytes("1.0KiB"), Some(1024));
        assert_eq!(
            parse_docker_mem_value_to_bytes("2.0MiB"),
            Some(2 * 1024 * 1024)
        );
        assert_eq!(
            parse_docker_mem_value_to_bytes("1.5GiB"),
            Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64)
        );
    }
}
