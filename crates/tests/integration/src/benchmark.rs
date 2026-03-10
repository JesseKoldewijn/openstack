use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::parity::{ProtocolFamily, ScenarioStep, TargetManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub localstack_image: String,
    pub report_dir: PathBuf,
    pub profiles: HashMap<String, BenchmarkProfile>,
    pub openstack_endpoint: Option<String>,
    pub localstack_endpoint: Option<String>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        let mut profiles = HashMap::new();
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

        Self {
            localstack_image: std::env::var("PARITY_LOCALSTACK_IMAGE")
                .unwrap_or_else(|_| "localstack/localstack:3.7.2".to_string()),
            report_dir: PathBuf::from("target/benchmark-reports"),
            profiles,
            openstack_endpoint: std::env::var("PARITY_OPENSTACK_ENDPOINT").ok(),
            localstack_endpoint: std::env::var("PARITY_LOCALSTACK_ENDPOINT").ok(),
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
    pub localstack_image: Option<String>,
    pub localstack_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub run_config: BenchmarkRunConfig,
    pub openstack: BenchmarkTargetResult,
    pub localstack: BenchmarkTargetResult,
    pub comparison: BenchmarkComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub total_scenarios: usize,
    pub openstack_error_count: usize,
    pub localstack_error_count: usize,
    pub avg_latency_p50_ratio: Option<f64>,
    pub avg_latency_p95_ratio: Option<f64>,
    pub avg_throughput_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub profile: String,
    pub run_id: String,
    pub generated_at: String,
    pub openstack_target: BenchmarkTargetMetadata,
    pub localstack_target: BenchmarkTargetMetadata,
    pub results: Vec<BenchmarkScenarioResult>,
    pub summary: BenchmarkSummary,
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

    let mut parity_config = crate::parity::ParityConfig::default();
    parity_config.localstack_image = config.localstack_image.clone();
    parity_config.openstack_endpoint = config.openstack_endpoint.clone();
    parity_config.localstack_endpoint = config.localstack_endpoint.clone();
    parity_config.target_services = Some(profile.services.clone());

    let mut manager = TargetManager::start(&parity_config).await?;
    let openstack_meta = BenchmarkTargetMetadata {
        endpoint: manager.openstack.endpoint.clone(),
        localstack_image: None,
        localstack_version: None,
    };
    let localstack_meta = BenchmarkTargetMetadata {
        endpoint: manager.localstack.endpoint.clone(),
        localstack_image: Some(config.localstack_image.clone()),
        localstack_version: localstack_version_from_image(&config.localstack_image),
    };

    let mut results = Vec::new();
    for scenario in scenarios {
        let scenario_run = scenario_run_config(&profile, &scenario);
        let openstack_metrics =
            execute_scenario(&manager.openstack.endpoint, &scenario, &scenario_run).await;
        let localstack_metrics =
            execute_scenario(&manager.localstack.endpoint, &scenario, &scenario_run).await;
        let comparison = compare_metrics(&openstack_metrics, &localstack_metrics);

        results.push(BenchmarkScenarioResult {
            scenario_id: scenario.id.clone(),
            service: scenario.service.clone(),
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

    let summary = summarize_results(&results);
    let report = BenchmarkReport {
        profile: profile_name.to_string(),
        run_id: run_id.clone(),
        generated_at: Utc::now().to_rfc3339(),
        openstack_target: openstack_meta,
        localstack_target: localstack_meta,
        results,
        summary,
    };

    let output_path =
        output_override.unwrap_or_else(|| config.report_dir.join(format!("{run_id}.json")));
    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(output_path, report_json)?;

    manager.stop().await;
    Ok(report)
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

async fn execute_scenario(
    endpoint: &str,
    scenario: &BenchmarkScenario,
    run_config: &BenchmarkRunConfig,
) -> BenchmarkMetrics {
    let mut context = HashMap::new();

    for step in &scenario.setup {
        let _ = execute_step(endpoint, step, &mut context).await;
    }

    for _ in 0..run_config.warmup_iterations {
        let _ = run_iteration(endpoint, scenario, run_config, &context).await;
    }

    let mut latencies = Vec::new();
    let mut operation_count = 0usize;
    let mut error_count = 0usize;
    let started = Instant::now();

    for _ in 0..run_config.measured_iterations {
        let iter = run_iteration(endpoint, scenario, run_config, &context).await;
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
                execute_operation_step(&endpoint_owned, &step, &context_owned).await
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
    let (elapsed_ms, output) = execute_aws_command(endpoint, rendered).await;

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
) -> StepExecution {
    let command = render_command(&step.command, context);
    let (elapsed_ms, output) = execute_aws_command(endpoint, command).await;

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

async fn execute_aws_command(
    endpoint: &str,
    command: Vec<String>,
) -> (f64, std::io::Result<std::process::Output>) {
    let mut full = vec![
        "--endpoint-url".to_string(),
        endpoint.to_string(),
        "--region".to_string(),
        "us-east-1".to_string(),
        "--no-sign-request".to_string(),
    ];
    full.extend(command);

    let started = Instant::now();
    let output =
        tokio::task::spawn_blocking(move || Command::new("aws").args(&full).output()).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    match output {
        Ok(out) => (elapsed_ms, out),
        Err(err) => (elapsed_ms, Err(std::io::Error::other(err.to_string()))),
    }
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
    let mut p50_ratios = Vec::new();
    let mut p95_ratios = Vec::new();
    let mut throughput_ratios = Vec::new();
    let mut openstack_error_count = 0usize;
    let mut localstack_error_count = 0usize;

    for result in results {
        openstack_error_count += result.openstack.metrics.error_count;
        localstack_error_count += result.localstack.metrics.error_count;

        if let Some(value) = result.comparison.latency_p50_ratio {
            p50_ratios.push(value);
        }
        if let Some(value) = result.comparison.latency_p95_ratio {
            p95_ratios.push(value);
        }
        if let Some(value) = result.comparison.throughput_ratio {
            throughput_ratios.push(value);
        }
    }

    BenchmarkSummary {
        total_scenarios: results.len(),
        openstack_error_count,
        localstack_error_count,
        avg_latency_p50_ratio: average(&p50_ratios),
        avg_latency_p95_ratio: average(&p95_ratios),
        avg_throughput_ratio: average(&throughput_ratios),
    }
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

fn all_service_names() -> Vec<String> {
    vec![
        "acm",
        "apigateway",
        "cloudformation",
        "cloudwatch",
        "dynamodb",
        "ec2",
        "ecr",
        "events",
        "firehose",
        "iam",
        "kinesis",
        "kms",
        "lambda",
        "opensearch",
        "redshift",
        "route53",
        "s3",
        "secretsmanager",
        "ses",
        "sns",
        "sqs",
        "ssm",
        "states",
        "sts",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}

fn load_profile_scenarios(profile_name: &str, run_id: &str) -> Vec<BenchmarkScenario> {
    let mut scenarios = default_benchmark_scenarios(run_id);
    let path = PathBuf::from(format!("tests/benchmark/scenarios/{profile_name}.json"));
    if path.exists()
        && let Ok(content) = std::fs::read_to_string(&path)
        && let Ok(mut external_scenarios) = serde_json::from_str::<Vec<BenchmarkScenario>>(&content)
    {
        for scenario in &mut external_scenarios {
            inject_run_context(scenario, run_id);
        }
        scenarios = external_scenarios;
    }

    scenarios
}

fn profile_matches(selected: &str, scenario_profile: &str) -> bool {
    if selected == "all-services-smoke-fast" {
        return scenario_profile == "all-services-smoke"
            || scenario_profile == "all-services-smoke-fast";
    }

    selected == scenario_profile
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
    vec![
        BenchmarkScenario {
            id: "s3-put-small-object".to_string(),
            profile: "all-services-smoke".to_string(),
            service: "s3".to_string(),
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
                "s3-put-object",
                ProtocolFamily::RestXml,
                vec![
                    "s3api".into(),
                    "put-object".into(),
                    "--bucket".into(),
                    "{{bucket}}".into(),
                    "--key".into(),
                    "item-{{run_id}}.txt".into(),
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
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        },
        BenchmarkScenario {
            id: "sqs-send-message".to_string(),
            profile: "all-services-smoke".to_string(),
            service: "sqs".to_string(),
            protocol: ProtocolFamily::QueryXml,
            setup: vec![
                scenario_step(
                    "sqs-create-queue",
                    ProtocolFamily::QueryXml,
                    vec![
                        "sqs".into(),
                        "create-queue".into(),
                        "--queue-name".into(),
                        "{{queue}}".into(),
                    ],
                ),
                scenario_step(
                    "sqs-get-queue-url",
                    ProtocolFamily::QueryXml,
                    vec![
                        "sqs".into(),
                        "get-queue-url".into(),
                        "--queue-name".into(),
                        "{{queue}}".into(),
                    ],
                ),
            ],
            operation: scenario_step(
                "sqs-send-message",
                ProtocolFamily::QueryXml,
                vec![
                    "sqs".into(),
                    "send-message".into(),
                    "--queue-url".into(),
                    "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}"
                        .into(),
                    "--message-body".into(),
                    "bench-{{run_id}}".into(),
                ],
            ),
            cleanup: vec![scenario_step(
                "sqs-delete-queue",
                ProtocolFamily::QueryXml,
                vec![
                    "sqs".into(),
                    "delete-queue".into(),
                    "--queue-url".into(),
                    "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{{queue}}"
                        .into(),
                ],
            )],
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        },
        BenchmarkScenario {
            id: "ddb-get-item".to_string(),
            profile: "all-services-smoke".to_string(),
            service: "dynamodb".to_string(),
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
                        "{\"pk\":{\"S\":\"k1\"},\"value\":{\"S\":\"v1\"}}".into(),
                    ],
                ),
            ],
            operation: scenario_step(
                "ddb-get-item",
                ProtocolFamily::Json,
                vec![
                    "dynamodb".into(),
                    "get-item".into(),
                    "--table-name".into(),
                    "{{table}}".into(),
                    "--key".into(),
                    "{\"pk\":{\"S\":\"k1\"}}".into(),
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
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        },
        BenchmarkScenario {
            id: "sts-get-caller-identity".to_string(),
            profile: "all-services-smoke".to_string(),
            service: "sts".to_string(),
            protocol: ProtocolFamily::QueryXml,
            setup: vec![],
            operation: scenario_step(
                "sts-get-caller-identity",
                ProtocolFamily::QueryXml,
                vec!["sts".into(), "get-caller-identity".into()],
            ),
            cleanup: vec![],
            warmup_iterations: None,
            measured_iterations: None,
            operations_per_iteration: None,
            concurrency: None,
        },
    ]
}

pub fn ensure_profile_scenarios(profile_name: &str) -> anyhow::Result<()> {
    let path = Path::new("tests/benchmark/scenarios").join(format!("{profile_name}.json"));
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
        ],
        other => return Err(anyhow::anyhow!("unknown profile for scenario generation: {other}")),
    };

    let json = serde_json::to_string_pretty(&scenarios)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BenchmarkMetrics, compare_metrics, percentile, summarize_results};
    use crate::benchmark::{
        BenchmarkComparison, BenchmarkRunConfig, BenchmarkScenarioResult, BenchmarkTargetMetadata,
        BenchmarkTargetResult,
    };

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
        assert_eq!(summary.openstack_error_count, 1);
        assert_eq!(summary.localstack_error_count, 2);
        assert_eq!(summary.avg_latency_p50_ratio, Some(1.25));
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
}
