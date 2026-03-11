use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::classification::{
    PersistenceMode, ServiceDurabilityClass, ServiceExecutionClass, parse_persistence_mode,
    service_durability_class, service_execution_class,
};
use crate::harness::TestHarness;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolFamily {
    Json,
    QueryXml,
    RestXml,
    RestJson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityConfig {
    pub localstack_image: String,
    pub report_dir: PathBuf,
    pub known_differences_path: PathBuf,
    pub timeout: Duration,
    pub retries: u8,
    pub profiles: HashMap<String, ProfileConfig>,
    pub openstack_endpoint: Option<String>,
    pub localstack_endpoint: Option<String>,
    pub target_services: Option<Vec<String>>,
    pub openstack_persistence_mode: PersistenceMode,
    pub localstack_persistence_mode: PersistenceMode,
}

impl Default for ParityConfig {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert(
            "core".to_string(),
            ProfileConfig {
                name: "core".to_string(),
                services: vec![
                    "s3".into(),
                    "sqs".into(),
                    "dynamodb".into(),
                    "sts".into(),
                    "compatibility".into(),
                ],
            },
        );
        profiles.insert(
            "extended".to_string(),
            ProfileConfig {
                name: "extended".to_string(),
                services: vec![
                    "s3".into(),
                    "sqs".into(),
                    "dynamodb".into(),
                    "sts".into(),
                    "compatibility".into(),
                ],
            },
        );
        profiles.insert(
            "all-services-smoke".to_string(),
            ProfileConfig {
                name: "all-services-smoke".to_string(),
                services: all_service_names(),
            },
        );
        profiles.insert(
            "all-services-smoke-fast".to_string(),
            ProfileConfig {
                name: "all-services-smoke-fast".to_string(),
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

        Self {
            localstack_image: std::env::var("PARITY_LOCALSTACK_IMAGE")
                .unwrap_or_else(|_| "localstack/localstack:3.7.2".to_string()),
            report_dir: PathBuf::from("target/parity-reports"),
            known_differences_path: PathBuf::from("tests/parity/known_differences.json"),
            timeout: Duration::from_secs(20),
            retries: 2,
            profiles,
            openstack_endpoint: std::env::var("PARITY_OPENSTACK_ENDPOINT").ok(),
            localstack_endpoint: std::env::var("PARITY_LOCALSTACK_ENDPOINT").ok(),
            target_services: None,
            openstack_persistence_mode: std::env::var("PARITY_OPENSTACK_PERSISTENCE_MODE")
                .ok()
                .and_then(|v| parse_persistence_mode(&v))
                .unwrap_or(PersistenceMode::NonDurable),
            localstack_persistence_mode: std::env::var("PARITY_LOCALSTACK_PERSISTENCE_MODE")
                .ok()
                .and_then(|v| parse_persistence_mode(&v))
                .unwrap_or(PersistenceMode::NonDurable),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub name: String,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub profile: String,
    pub service: String,
    pub setup: Vec<ScenarioStep>,
    pub steps: Vec<ScenarioStep>,
    pub assertions: Vec<ScenarioStep>,
    pub cleanup: Vec<ScenarioStep>,
    #[serde(default)]
    pub requires_restart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioStep {
    pub id: String,
    pub protocol: ProtocolFamily,
    pub command: Vec<String>,
    pub expect_success: bool,
    pub capture_json: Option<CaptureJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureJson {
    pub output_key: String,
    pub json_pointer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTrace {
    pub step_id: String,
    pub command: Vec<String>,
    pub exit_code: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub normalized_stdout: String,
    pub normalized_stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario_id: String,
    pub service: String,
    pub service_execution_class: Option<ServiceExecutionClass>,
    pub service_durability_class: Option<ServiceDurabilityClass>,
    pub passed: bool,
    pub accepted_differences: usize,
    pub mismatches: Vec<Mismatch>,
    pub openstack_traces: Vec<StepTrace>,
    pub localstack_traces: Vec<StepTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mismatch {
    pub scenario_id: String,
    pub service: String,
    pub step_id: String,
    pub path: String,
    pub kind: String,
    pub openstack: String,
    pub localstack: String,
    pub accepted_difference_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParitySummary {
    pub total_scenarios: usize,
    pub passed: usize,
    pub failed: usize,
    pub accepted_differences: usize,
    pub per_service_score: BTreeMap<String, ServiceScore>,
    pub persistence_failure_classes: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceScore {
    pub service_execution_class: Option<ServiceExecutionClass>,
    pub service_durability_class: Option<ServiceDurabilityClass>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityReport {
    pub profile: String,
    pub run_id: String,
    pub generated_at: String,
    pub openstack_endpoint: String,
    pub localstack_endpoint: String,
    pub openstack_persistence_mode: PersistenceMode,
    pub localstack_persistence_mode: PersistenceMode,
    pub persistence_mode_equivalent: bool,
    pub summary: ParitySummary,
    pub results: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDifferenceRule {
    pub id: String,
    pub service: String,
    pub scenario_id: String,
    pub step_id: String,
    pub path: String,
    pub rationale: String,
    pub owner: String,
    pub reviewer: String,
    pub review_date: String,
    pub expires_on: String,
}

pub async fn run_profile(
    config: &ParityConfig,
    profile_name: &str,
) -> anyhow::Result<ParityReport> {
    let profile = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {profile_name}"))?
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
            "profile '{}' has no scenarios configured",
            profile_name
        ));
    }

    let known_differences = load_known_differences(&config.known_differences_path)?;
    validate_known_differences(&known_differences)?;

    let mut run_config = config.clone();
    run_config.target_services = Some(
        profile
            .services
            .iter()
            .filter(|service| service.as_str() != "compatibility")
            .cloned()
            .collect(),
    );

    let mut manager = TargetManager::start(&run_config).await?;
    let mut results = Vec::new();

    for scenario in scenarios {
        let result = run_scenario(&mut manager, &scenario, &known_differences, &run_config).await;
        results.push(result);
    }

    let summary = summarize_results(&results);
    let report = ParityReport {
        profile: profile_name.to_string(),
        run_id: run_id.clone(),
        generated_at: Utc::now().to_rfc3339(),
        openstack_endpoint: manager.openstack.endpoint.clone(),
        localstack_endpoint: manager.localstack.endpoint.clone(),
        openstack_persistence_mode: config.openstack_persistence_mode,
        localstack_persistence_mode: config.localstack_persistence_mode,
        persistence_mode_equivalent: config.openstack_persistence_mode
            == config.localstack_persistence_mode,
        summary,
        results,
    };

    let profile_path = config.report_dir.join(format!("{profile_name}-latest.json"));
    let profile_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(profile_path, profile_json)?;

    let report_path = config.report_dir.join(format!("{run_id}.json"));
    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(report_path, report_json)?;

    manager.stop().await;
    Ok(report)
}

fn load_profile_scenarios(profile_name: &str, run_id: &str) -> Vec<Scenario> {
    let mut scenarios = default_scenarios(run_id);
    let path = PathBuf::from(format!("tests/parity/scenarios/{profile_name}.json"));
    if path.exists()
        && let Ok(content) = std::fs::read_to_string(&path)
        && let Ok(mut external_scenarios) = serde_json::from_str::<Vec<Scenario>>(&content)
    {
        let external_ids = external_scenarios
            .iter()
            .map(|scenario| scenario.id.clone())
            .collect::<HashSet<_>>();

        for scenario in &mut external_scenarios {
            inject_run_context(scenario, run_id);
        }

        scenarios.retain(|scenario| !external_ids.contains(&scenario.id));
        scenarios.extend(external_scenarios);
    }

    scenarios
}

fn profile_matches(selected: &str, scenario_profile: &str) -> bool {
    if selected == "extended" {
        return scenario_profile == "extended" || scenario_profile == "core";
    }

    if selected == "all-services-smoke-fast" {
        return scenario_profile == "all-services-smoke"
            || scenario_profile == "all-services-smoke-fast";
    }

    selected == scenario_profile
}

fn inject_run_context(scenario: &mut Scenario, run_id: &str) {
    let replacements = [
        ("{{run_id}}", run_id.to_string()),
        ("{{bucket}}", format!("parity-bucket-{run_id}")),
        ("{{queue}}", format!("parity-queue-{run_id}")),
        ("{{table}}", format!("parity-table-{run_id}")),
    ];

    for step in scenario
        .setup
        .iter_mut()
        .chain(scenario.steps.iter_mut())
        .chain(scenario.assertions.iter_mut())
        .chain(scenario.cleanup.iter_mut())
    {
        for part in &mut step.command {
            for (needle, value) in &replacements {
                *part = part.replace(needle, value);
            }
        }
    }
}

fn summarize_results(results: &[ScenarioResult]) -> ParitySummary {
    let mut per_service_score: BTreeMap<String, ServiceScore> = BTreeMap::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut accepted_differences = 0usize;
    let mut persistence_failure_classes: BTreeMap<String, usize> = BTreeMap::new();

    for result in results {
        let score = per_service_score
            .entry(result.service.clone())
            .or_insert(ServiceScore {
                service_execution_class: result.service_execution_class,
                service_durability_class: result.service_durability_class,
                total: 0,
                passed: 0,
                failed: 0,
            });
        score.total += 1;

        for mismatch in &result.mismatches {
            if mismatch.kind == "persistence_mode_mismatch"
                || mismatch.kind == "persistence_recovery_inconsistency"
                || mismatch.kind == "persistence_durability_mismatch"
            {
                *persistence_failure_classes
                    .entry(mismatch.kind.clone())
                    .or_insert(0) += 1;
            }
        }

        accepted_differences += result.accepted_differences;
        if result.passed {
            passed += 1;
            score.passed += 1;
        } else {
            failed += 1;
            score.failed += 1;
        }
    }

    ParitySummary {
        total_scenarios: results.len(),
        passed,
        failed,
        accepted_differences,
        per_service_score,
        persistence_failure_classes,
    }
}

async fn run_scenario(
    manager: &mut TargetManager,
    scenario: &Scenario,
    known_differences: &[KnownDifferenceRule],
    config: &ParityConfig,
) -> ScenarioResult {
    let service_execution_class = service_execution_class(&scenario.service);
    let service_durability_class = service_durability_class(&scenario.service);

    let mut openstack_context = HashMap::new();
    let mut localstack_context = HashMap::new();

    if scenario.requires_restart {
        let openstack_restart = reqwest::Client::new()
            .post(format!("{}/_localstack/health", manager.openstack.endpoint))
            .send()
            .await;
        let localstack_restart = reqwest::Client::new()
            .post(format!("{}/_localstack/health", manager.localstack.endpoint))
            .send()
            .await;

        if openstack_restart.is_err() || localstack_restart.is_err() {
            let mut mismatches = vec![Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: "restart".to_string(),
                path: "lifecycle".to_string(),
                kind: "persistence_recovery_inconsistency".to_string(),
                openstack: format!("{:?}", openstack_restart.err()),
                localstack: format!("{:?}", localstack_restart.err()),
                accepted_difference_id: None,
            }];
            dedupe_mismatches(&mut mismatches);
            return ScenarioResult {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                service_execution_class,
                service_durability_class,
                passed: false,
                accepted_differences: 0,
                mismatches,
                openstack_traces: Vec::new(),
                localstack_traces: Vec::new(),
            };
        }
    }

    let openstack_traces = run_steps(
        &manager.openstack.endpoint,
        scenario,
        &mut openstack_context,
        config,
    );

    let localstack_traces = run_steps(
        &manager.localstack.endpoint,
        scenario,
        &mut localstack_context,
        config,
    );

    let mut mismatches = compare_traces(scenario, &openstack_traces, &localstack_traces);
    let environment_errors =
        collect_environment_errors(scenario, &openstack_traces, &localstack_traces);
    if !environment_errors.is_empty() {
        mismatches.extend(environment_errors);
    }

    apply_expectation_mismatches(
        scenario,
        &openstack_traces,
        &localstack_traces,
        &mut mismatches,
    );

    if config.openstack_persistence_mode != config.localstack_persistence_mode {
        mismatches.push(Mismatch {
            scenario_id: scenario.id.clone(),
            service: scenario.service.clone(),
            step_id: "persistence".to_string(),
            path: "mode".to_string(),
            kind: "persistence_mode_mismatch".to_string(),
            openstack: format!("{:?}", config.openstack_persistence_mode),
            localstack: format!("{:?}", config.localstack_persistence_mode),
            accepted_difference_id: None,
        });
    }

    dedupe_mismatches(&mut mismatches);
    for mismatch in &mut mismatches {
        if let Some(rule) = match_known_difference(mismatch, known_differences) {
            mismatch.accepted_difference_id = Some(rule.id.clone());
        }
    }

    let accepted_differences = mismatches
        .iter()
        .filter(|m| m.accepted_difference_id.is_some())
        .count();

    let unaccepted = mismatches
        .iter()
        .filter(|m| m.accepted_difference_id.is_none())
        .count();

    ScenarioResult {
        scenario_id: scenario.id.clone(),
        service: scenario.service.clone(),
        service_execution_class,
        service_durability_class,
        passed: unaccepted == 0,
        accepted_differences,
        mismatches,
        openstack_traces,
        localstack_traces,
    }
}

fn apply_expectation_mismatches(
    scenario: &Scenario,
    openstack: &[StepTrace],
    localstack: &[StepTrace],
    mismatches: &mut Vec<Mismatch>,
) {
    let all_steps = scenario
        .setup
        .iter()
        .chain(scenario.steps.iter())
        .chain(scenario.assertions.iter())
        .chain(scenario.cleanup.iter())
        .collect::<Vec<_>>();

    for (idx, step) in all_steps.iter().enumerate() {
        let openstack_ok = openstack
            .get(idx)
            .map(|trace| trace.success == step.expect_success)
            .unwrap_or(false);
        let localstack_ok = localstack
            .get(idx)
            .map(|trace| trace.success == step.expect_success)
            .unwrap_or(false);

        if !openstack_ok && localstack_ok {
            let openstack_actual = openstack
                .get(idx)
                .map(|trace| trace.success.to_string())
                .unwrap_or_else(|| "missing".to_string());
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: step.id.clone(),
                path: "expect_success".to_string(),
                kind: "expected_outcome_mismatch".to_string(),
                openstack: openstack_actual,
                localstack: step.expect_success.to_string(),
                accepted_difference_id: None,
            });
        }
    }
}

fn collect_environment_errors(
    scenario: &Scenario,
    openstack: &[StepTrace],
    localstack: &[StepTrace],
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();

    let openstack_env = openstack.iter().find(|trace| {
        trace.stderr.contains("failed to execute aws cli")
            || trace.stderr.contains("Unable to locate credentials")
    });
    let localstack_env = localstack.iter().find(|trace| {
        trace.stderr.contains("failed to execute aws cli")
            || trace.stderr.contains("Unable to locate credentials")
    });

    if openstack_env.is_some() || localstack_env.is_some() {
        let openstack_msg = openstack_env
            .map(|trace| trace.stderr.trim().to_string())
            .unwrap_or_default();
        let localstack_msg = localstack_env
            .map(|trace| trace.stderr.trim().to_string())
            .unwrap_or_default();
        mismatches.push(Mismatch {
            scenario_id: scenario.id.clone(),
            service: scenario.service.clone(),
            step_id: "environment".to_string(),
            path: "preflight".to_string(),
            kind: "environment_error".to_string(),
            openstack: openstack_msg,
            localstack: localstack_msg,
            accepted_difference_id: None,
        });
    }

    mismatches
}

fn dedupe_mismatches(mismatches: &mut Vec<Mismatch>) {
    let mut seen = HashSet::new();
    mismatches.retain(|mismatch| {
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            mismatch.scenario_id,
            mismatch.service,
            mismatch.step_id,
            mismatch.path,
            mismatch.kind,
            mismatch.openstack,
            mismatch.localstack
        );
        seen.insert(key)
    });
}

fn run_steps(
    endpoint: &str,
    scenario: &Scenario,
    context: &mut HashMap<String, String>,
    config: &ParityConfig,
) -> Vec<StepTrace> {
    let mut traces = Vec::new();

    for step in scenario
        .setup
        .iter()
        .chain(scenario.steps.iter())
        .chain(scenario.assertions.iter())
        .chain(scenario.cleanup.iter())
    {
        if !context.contains_key("queue_name")
            && let Some(queue_name) = extract_flag_value(&step.command, "--queue-name")
        {
            context.insert("queue_name".to_string(), queue_name);
        }

        if !context.contains_key("queue_url")
            && let Some(queue_name) = context.get("queue_name")
        {
            let queue_url = format!(
                "{}/000000000000/{}",
                endpoint.trim_end_matches('/'),
                queue_name
            );
            context.insert("queue_url".to_string(), queue_url);
        }

        if !context.contains_key("bucket_name")
            && let Some(bucket_name) = extract_flag_value(&step.command, "--bucket")
        {
            context.insert("bucket_name".to_string(), bucket_name);
        }

        if !context.contains_key("bucket_host_url")
            && let Some(bucket_name) = context.get("bucket_name")
        {
            let endpoint_trimmed = endpoint
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            let bucket_host_url = format!("http://{}.{}", bucket_name, endpoint_trimmed);
            context.insert("bucket_host_url".to_string(), bucket_host_url);
        }

        if !context.contains_key("queue_hostname_url")
            && let Some(queue_name) = context.get("queue_name")
        {
            let queue_hostname_url = format!(
                "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/{}",
                queue_name
            );
            context.insert("queue_hostname_url".to_string(), queue_hostname_url);
        }

        let command = render_command(&step.command, context);
        let mut full = vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "--region".to_string(),
            "us-east-1".to_string(),
            "--no-sign-request".to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];
        full.extend(command);

        let mut output = None;
        let mut elapsed = Duration::from_secs(0);
        for attempt in 0..=config.retries {
            let started = Instant::now();
            let out = Command::new("aws").args(&full).output();
            elapsed = started.elapsed();

            let should_retry = matches!(&out, Ok(inner) if !inner.status.success() && step.expect_success)
                && attempt < config.retries;
            if should_retry {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            output = Some(out);
            break;
        }

        let trace =
            match output.unwrap_or_else(|| Err(io::Error::other("no command output captured"))) {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let exit_code = out.status.code().unwrap_or(1);
                    let success = out.status.success();

                    if elapsed > config.timeout {
                        StepTrace {
                            step_id: step.id.clone(),
                            command: full,
                            exit_code,
                            success: false,
                            stdout,
                            stderr: format!("timed out after {:?}", config.timeout),
                            normalized_stdout: String::new(),
                            normalized_stderr: String::new(),
                        }
                    } else {
                        if let Some(capture) = &step.capture_json {
                            capture_output_value(&stdout, context, capture);
                        }

                        let normalized_stdout = normalize_payload(&stdout, &step.protocol);
                        let normalized_stderr = normalize_payload(&stderr, &step.protocol);

                        StepTrace {
                            step_id: step.id.clone(),
                            command: full,
                            exit_code,
                            success,
                            stdout,
                            stderr,
                            normalized_stdout,
                            normalized_stderr,
                        }
                    }
                }
                Err(err) => StepTrace {
                    step_id: step.id.clone(),
                    command: full,
                    exit_code: 127,
                    success: false,
                    stdout: String::new(),
                    stderr: format!("failed to execute aws cli: {err}"),
                    normalized_stdout: String::new(),
                    normalized_stderr: String::new(),
                },
            };

        traces.push(trace);
    }

    traces
}

fn extract_flag_value(command: &[String], flag: &str) -> Option<String> {
    for idx in 0..command.len() {
        if command[idx] == flag {
            return command.get(idx + 1).cloned();
        }
    }
    None
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

fn capture_output_value(
    stdout: &str,
    context: &mut HashMap<String, String>,
    capture: &CaptureJson,
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

fn normalize_payload(raw: &str, protocol: &ProtocolFamily) -> String {
    let trimmed = raw.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return normalize_json(trimmed);
    }

    match protocol {
        ProtocolFamily::Json | ProtocolFamily::RestJson => normalize_json(raw),
        ProtocolFamily::QueryXml | ProtocolFamily::RestXml => normalize_xml(raw),
    }
}

fn normalize_json(raw: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(raw);
    match parsed {
        Ok(value) => {
            let mut normalized = value;
            scrub_dynamic_json(&mut normalized);
            serde_json::to_string(&normalized).unwrap_or_else(|_| raw.trim().to_string())
        }
        Err(_) => raw.trim().to_string(),
    }
}

fn scrub_dynamic_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for key in ["RequestId", "requestId", "ResponseMetadata"] {
                map.remove(key);
            }

            for key in [
                "QueueUrl",
                "ReceiptHandle",
                "MessageId",
                "MD5OfBody",
                "MD5OfMessageBody",
                "ChecksumCRC32",
                "ServerSideEncryption",
                "AcceptRanges",
                "PackedPolicySize",
                "AccessKeyId",
                "SecretAccessKey",
                "SessionToken",
                "Expiration",
                "AssumedRoleId",
                "LastModified",
                "ContentType",
                "DisplayName",
                "ID",
            ] {
                map.remove(key);
            }

            if let Some(table) = map.get_mut("TableDescription")
                && let Some(table_map) = table.as_object_mut()
            {
                for key in [
                    "CreationDateTime",
                    "TableId",
                    "ProvisionedThroughput",
                    "DeletionProtectionEnabled",
                    "TableSizeBytes",
                    "StreamSpecification",
                ] {
                    table_map.remove(key);
                }

                if let Some(billing) = table_map.get_mut("BillingModeSummary")
                    && let Some(billing_map) = billing.as_object_mut()
                {
                    billing_map.remove("LastUpdateToPayPerRequestDateTime");
                }
            }

            for val in map.values_mut() {
                scrub_dynamic_json(val);
            }
        }
        serde_json::Value::Array(values) => {
            for item in values {
                scrub_dynamic_json(item);
            }
        }
        _ => {}
    }
}

fn normalize_xml(raw: &str) -> String {
    let mut text = raw.trim().replace('\n', "");
    for token in [
        "<RequestId>",
        "</RequestId>",
        "<ResponseMetadata>",
        "</ResponseMetadata>",
    ] {
        text = text.replace(token, "");
    }
    text = text.replace("\t", "");
    while text.contains("  ") {
        text = text.replace("  ", " ");
    }

    if let Ok(re) = regex::Regex::new(r#"[A-Za-z0-9_-]+core-[0-9]{14}"#) {
        text = re.replace_all(&text, "<run-id>").to_string();
    }

    if let Ok(re) = regex::Regex::new(r#"<RequestId>[^<]+</RequestId>"#) {
        text = re
            .replace_all(&text, "<RequestId><request-id></RequestId>")
            .to_string();
    }
    if let Ok(re) = regex::Regex::new(r#"<Message>Queue does not exist</Message>"#) {
        text = re
            .replace_all(
                &text,
                "<Message>The specified queue does not exist.</Message>",
            )
            .to_string();
    }
    if let Ok(re) = regex::Regex::new(r#"http://[a-z0-9\.-]+:[0-9]+/000000000000/"#) {
        text = re
            .replace_all(
                &text,
                "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/",
            )
            .to_string();
    }

    if text.contains("(reached max retries:")
        && let Ok(re) = regex::Regex::new(r#" \(reached max retries: [0-9]+\)"#)
    {
        text = re.replace_all(&text, "").to_string();
    }

    if let Ok(re) =
        regex::Regex::new(r#"Cannot do operations on a non-existent table: [A-Za-z0-9_-]+"#)
    {
        text = re
            .replace_all(&text, "Cannot do operations on a non-existent table")
            .to_string();
    }

    if let Ok(re) = regex::Regex::new(r#"Additional error details:\s*Type:\s*[A-Za-z]+"#) {
        text = re.replace_all(&text, "").to_string();
    }

    text
}

fn compare_traces(
    scenario: &Scenario,
    openstack: &[StepTrace],
    localstack: &[StepTrace],
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    let len = std::cmp::max(openstack.len(), localstack.len());

    for idx in 0..len {
        let Some(o) = openstack.get(idx) else {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: format!("index-{idx}"),
                path: "trace".to_string(),
                kind: "missing_openstack_trace".to_string(),
                openstack: String::new(),
                localstack: "trace present".to_string(),
                accepted_difference_id: None,
            });
            continue;
        };
        let Some(l) = localstack.get(idx) else {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "trace".to_string(),
                kind: "missing_localstack_trace".to_string(),
                openstack: "trace present".to_string(),
                localstack: String::new(),
                accepted_difference_id: None,
            });
            continue;
        };

        if o.success != l.success {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "success".to_string(),
                kind: "success_mismatch".to_string(),
                openstack: o.success.to_string(),
                localstack: l.success.to_string(),
                accepted_difference_id: None,
            });
        }

        if o.normalized_stdout != l.normalized_stdout {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "stdout".to_string(),
                kind: "stdout_mismatch".to_string(),
                openstack: o.normalized_stdout.clone(),
                localstack: l.normalized_stdout.clone(),
                accepted_difference_id: None,
            });
        }

        if !o.success && !l.success && o.normalized_stderr != l.normalized_stderr {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "stderr".to_string(),
                kind: "stderr_mismatch".to_string(),
                openstack: o.normalized_stderr.clone(),
                localstack: l.normalized_stderr.clone(),
                accepted_difference_id: None,
            });
        }
    }

    mismatches
}

fn match_known_difference<'a>(
    mismatch: &Mismatch,
    rules: &'a [KnownDifferenceRule],
) -> Option<&'a KnownDifferenceRule> {
    rules.iter().find(|rule| {
        matches_or_wildcard(&rule.service, &mismatch.service)
            && matches_or_wildcard(&rule.scenario_id, &mismatch.scenario_id)
            && matches_or_wildcard(&rule.step_id, &mismatch.step_id)
            && matches_or_wildcard(&rule.path, &mismatch.path)
    })
}

fn matches_or_wildcard(rule: &str, actual: &str) -> bool {
    rule == "*" || rule == actual
}

fn load_known_differences(path: &Path) -> anyhow::Result<Vec<KnownDifferenceRule>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    let rules = serde_json::from_str::<Vec<KnownDifferenceRule>>(&content)?;
    Ok(rules)
}

fn validate_known_differences(rules: &[KnownDifferenceRule]) -> anyhow::Result<()> {
    let today = Utc::now().date_naive();
    for rule in rules {
        if rule.id.trim().is_empty()
            || rule.rationale.trim().is_empty()
            || rule.owner.trim().is_empty()
            || rule.reviewer.trim().is_empty()
        {
            return Err(anyhow::anyhow!(
                "known difference '{}' is missing required metadata",
                rule.id
            ));
        }

        let expires = chrono::NaiveDate::parse_from_str(&rule.expires_on, "%Y-%m-%d")?;
        if expires < today {
            return Err(anyhow::anyhow!(
                "known difference '{}' expired on {}",
                rule.id,
                rule.expires_on
            ));
        }

        chrono::NaiveDate::parse_from_str(&rule.review_date, "%Y-%m-%d")?;
    }

    Ok(())
}

pub struct TargetManager {
    pub openstack: ManagedTarget,
    pub localstack: ManagedTarget,
    localstack_container_id: Option<String>,
    openstack_harness: Option<TestHarness>,
}

pub struct ManagedTarget {
    pub endpoint: String,
}

impl TargetManager {
    pub async fn start(config: &ParityConfig) -> anyhow::Result<Self> {
        let target_services = config
            .target_services
            .clone()
            .unwrap_or_else(|| vec!["s3".into(), "sqs".into(), "dynamodb".into(), "sts".into()]);
        let services = target_services.join(",");

        let (openstack, openstack_harness) = if let Some(endpoint) = &config.openstack_endpoint {
            (
                ManagedTarget {
                    endpoint: endpoint.clone(),
                },
                None,
            )
        } else {
            let harness = TestHarness::start_services(&services).await;
            let endpoint = harness.base_url.clone();
            (ManagedTarget { endpoint }, Some(harness))
        };

        let (localstack, container_id) = if let Some(endpoint) = &config.localstack_endpoint {
            (
                ManagedTarget {
                    endpoint: endpoint.clone(),
                },
                None,
            )
        } else {
            let port = free_port()?;
            let endpoint = format!("http://127.0.0.1:{port}");
            let localstack_services = target_services
                .iter()
                .map(|service| map_service_for_localstack(service))
                .collect::<Vec<_>>()
                .join(",");
            let output = Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "--rm",
                    "-p",
                    &format!("127.0.0.1:{port}:4566"),
                    "-e",
                    &format!("SERVICES={}", localstack_services),
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

            let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            wait_for_health(&endpoint, Duration::from_secs(60)).await?;
            (ManagedTarget { endpoint }, Some(container_id))
        };

        Ok(Self {
            openstack,
            localstack,
            localstack_container_id: container_id,
            openstack_harness,
        })
    }

    pub async fn stop(&mut self) {
        if let Some(container_id) = &self.localstack_container_id {
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .output();
        }
        self.localstack_container_id = None;
        if let Some(harness) = self.openstack_harness.take() {
            harness.shutdown();
        }
    }
}

fn map_service_for_localstack(service: &str) -> String {
    match service {
        "events" => "eventbridge".to_string(),
        "states" => "stepfunctions".to_string(),
        _ => service.to_string(),
    }
}

async fn wait_for_health(endpoint: &str, timeout: Duration) -> anyhow::Result<()> {
    let health = format!("{endpoint}/_localstack/health");
    let deadline = Instant::now() + timeout;

    loop {
        if Instant::now() > deadline {
            return Err(anyhow::anyhow!(
                "timed out waiting for localstack health at {health}"
            ));
        }

        if let Ok(resp) = reqwest::get(&health).await
            && resp.status().is_success()
        {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

pub fn default_scenarios(run_id: &str) -> Vec<Scenario> {
    let bucket = format!("parity-bucket-{run_id}");
    let queue = format!("parity-queue-{run_id}");
    let table = format!("parity-table-{run_id}");

    vec![
        Scenario {
            id: "s3-basic-lifecycle".to_string(),
            profile: "core".to_string(),
            service: "s3".to_string(),
            setup: vec![ScenarioStep {
                id: "s3-create-bucket".to_string(),
                protocol: ProtocolFamily::RestXml,
                command: vec![
                    "s3api".into(),
                    "create-bucket".into(),
                    "--bucket".into(),
                    bucket.clone(),
                ],
                expect_success: true,
                capture_json: None,
            }],
            steps: vec![
                ScenarioStep {
                    id: "s3-put-object".to_string(),
                    protocol: ProtocolFamily::RestXml,
                    command: vec![
                        "s3api".into(),
                        "put-object".into(),
                        "--bucket".into(),
                        bucket.clone(),
                        "--key".into(),
                        "item.txt".into(),
                        "--body".into(),
                        "README.md".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "s3-head-object".to_string(),
                    protocol: ProtocolFamily::RestXml,
                    command: vec![
                        "s3api".into(),
                        "head-object".into(),
                        "--bucket".into(),
                        bucket.clone(),
                        "--key".into(),
                        "item.txt".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "s3-error-missing-object".to_string(),
                    protocol: ProtocolFamily::RestXml,
                    command: vec![
                        "s3api".into(),
                        "head-object".into(),
                        "--bucket".into(),
                        bucket.clone(),
                        "--key".into(),
                        "missing.txt".into(),
                    ],
                    expect_success: false,
                    capture_json: None,
                },
            ],
            assertions: vec![],
            cleanup: vec![
                ScenarioStep {
                    id: "s3-delete-object".to_string(),
                    protocol: ProtocolFamily::RestXml,
                    command: vec![
                        "s3api".into(),
                        "delete-object".into(),
                        "--bucket".into(),
                        bucket.clone(),
                        "--key".into(),
                        "item.txt".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "s3-delete-bucket".to_string(),
                    protocol: ProtocolFamily::RestXml,
                    command: vec![
                        "s3api".into(),
                        "delete-bucket".into(),
                        "--bucket".into(),
                        bucket,
                    ],
                    expect_success: true,
                    capture_json: None,
                },
            ],
            requires_restart: false,
        },
        Scenario {
            id: "sqs-basic-lifecycle".to_string(),
            profile: "core".to_string(),
            service: "sqs".to_string(),
            setup: vec![
                ScenarioStep {
                    id: "sqs-create-queue".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "create-queue".into(),
                        "--queue-name".into(),
                        queue.clone(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "sqs-get-queue-url".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "get-queue-url".into(),
                        "--queue-name".into(),
                        queue.clone(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
            ],
            steps: vec![
                ScenarioStep {
                    id: "sqs-send-message".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "send-message".into(),
                        "--queue-url".into(),
                        "{{queue_hostname_url}}".into(),
                        "--message-body".into(),
                        format!("hello-{run_id}"),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "sqs-receive-message".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "receive-message".into(),
                        "--queue-url".into(),
                        "{{queue_hostname_url}}".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "sqs-error-missing-queue".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "get-queue-url".into(),
                        "--queue-name".into(),
                        format!("missing-{run_id}"),
                    ],
                    expect_success: false,
                    capture_json: None,
                },
            ],
            assertions: vec![],
            cleanup: vec![ScenarioStep {
                id: "sqs-delete-queue".to_string(),
                protocol: ProtocolFamily::QueryXml,
                command: vec![
                    "sqs".into(),
                    "delete-queue".into(),
                    "--queue-url".into(),
                    "{{queue_hostname_url}}".into(),
                ],
                expect_success: true,
                capture_json: None,
            }],
            requires_restart: false,
        },
        Scenario {
            id: "dynamodb-basic-lifecycle".to_string(),
            profile: "core".to_string(),
            service: "dynamodb".to_string(),
            setup: vec![ScenarioStep {
                id: "ddb-create-table".to_string(),
                protocol: ProtocolFamily::Json,
                command: vec![
                    "dynamodb".into(),
                    "create-table".into(),
                    "--table-name".into(),
                    table.clone(),
                    "--attribute-definitions".into(),
                    "AttributeName=pk,AttributeType=S".into(),
                    "--key-schema".into(),
                    "AttributeName=pk,KeyType=HASH".into(),
                    "--billing-mode".into(),
                    "PAY_PER_REQUEST".into(),
                ],
                expect_success: true,
                capture_json: None,
            }],
            steps: vec![
                ScenarioStep {
                    id: "ddb-put-item".to_string(),
                    protocol: ProtocolFamily::Json,
                    command: vec![
                        "dynamodb".into(),
                        "put-item".into(),
                        "--table-name".into(),
                        table.clone(),
                        "--item".into(),
                        "{\"pk\":{\"S\":\"k1\"},\"value\":{\"S\":\"v1\"}}".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "ddb-get-item".to_string(),
                    protocol: ProtocolFamily::Json,
                    command: vec![
                        "dynamodb".into(),
                        "get-item".into(),
                        "--table-name".into(),
                        table.clone(),
                        "--key".into(),
                        "{\"pk\":{\"S\":\"k1\"}}".into(),
                    ],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "ddb-error-missing-table".to_string(),
                    protocol: ProtocolFamily::Json,
                    command: vec![
                        "dynamodb".into(),
                        "describe-table".into(),
                        "--table-name".into(),
                        format!("missing-{run_id}"),
                    ],
                    expect_success: false,
                    capture_json: None,
                },
            ],
            assertions: vec![],
            cleanup: vec![ScenarioStep {
                id: "ddb-delete-table".to_string(),
                protocol: ProtocolFamily::Json,
                command: vec![
                    "dynamodb".into(),
                    "delete-table".into(),
                    "--table-name".into(),
                    table,
                ],
                expect_success: true,
                capture_json: None,
            }],
            requires_restart: false,
        },
        Scenario {
            id: "sts-identity-and-error".to_string(),
            profile: "core".to_string(),
            service: "sts".to_string(),
            setup: vec![],
            steps: vec![
                ScenarioStep {
                    id: "sts-get-caller-identity".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec!["sts".into(), "get-caller-identity".into()],
                    expect_success: true,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "sts-error-assume-role".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sts".into(),
                        "assume-role".into(),
                        "--role-arn".into(),
                        "arn:aws:iam::000000000000:role/does-not-exist".into(),
                        "--role-session-name".into(),
                        format!("parity-{run_id}"),
                    ],
                    expect_success: false,
                    capture_json: None,
                },
            ],
            assertions: vec![],
            cleanup: vec![],
            requires_restart: false,
        },
        Scenario {
            id: "compat-services-env-behavior".to_string(),
            profile: "core".to_string(),
            service: "compatibility".to_string(),
            setup: vec![],
            steps: vec![
                ScenarioStep {
                    id: "services-env-restricts-disabled-service".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec!["sns".into(), "list-topics".into()],
                    expect_success: false,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "url-host-format-sqs".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec![
                        "sqs".into(),
                        "get-queue-url".into(),
                        "--queue-name".into(),
                        format!("missing-host-check-{run_id}"),
                    ],
                    expect_success: false,
                    capture_json: None,
                },
                ScenarioStep {
                    id: "identity-health-check".to_string(),
                    protocol: ProtocolFamily::QueryXml,
                    command: vec!["sts".into(), "get-caller-identity".into()],
                    expect_success: true,
                    capture_json: None,
                },
            ],
            assertions: vec![],
            cleanup: vec![],
            requires_restart: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{normalize_xml, summarize_results, Mismatch, ScenarioResult};
    use crate::classification::{ServiceDurabilityClass, ServiceExecutionClass};

    #[test]
    fn normalize_xml_removes_additional_error_details_footer() {
        let raw = "aws: [ERROR]: An error occurred (InternalFailure) when calling the ListTopics operation: Service 'sns' is not enabled. Please check your 'SERVICES' configuration variable.\n\nAdditional error details:\nType: Sender\n";

        let normalized = normalize_xml(raw);

        assert_eq!(
            normalized,
            "aws: [ERROR]: An error occurred (InternalFailure) when calling the ListTopics operation: Service 'sns' is not enabled. Please check your 'SERVICES' configuration variable.",
        );
    }

    #[test]
    fn summarize_results_collects_persistence_failure_classes() {
        let result = ScenarioResult {
            scenario_id: "s3-persistence-restart".to_string(),
            service: "s3".to_string(),
            service_execution_class: Some(ServiceExecutionClass::InProcStateful),
            service_durability_class: Some(ServiceDurabilityClass::Durable),
            passed: false,
            accepted_differences: 0,
            mismatches: vec![Mismatch {
                scenario_id: "s3-persistence-restart".to_string(),
                service: "s3".to_string(),
                step_id: "persistence".to_string(),
                path: "mode".to_string(),
                kind: "persistence_mode_mismatch".to_string(),
                openstack: "Durable".to_string(),
                localstack: "NonDurable".to_string(),
                accepted_difference_id: None,
            }],
            openstack_traces: Vec::new(),
            localstack_traces: Vec::new(),
        };

        let summary = summarize_results(&[result]);
        assert_eq!(summary.failed, 1);
        assert_eq!(
            summary.persistence_failure_classes.get("persistence_mode_mismatch"),
            Some(&1)
        );
    }
}
