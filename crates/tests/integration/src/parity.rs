use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::Utc;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static RE_S3_OWNER: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<Owner>.*?</Owner>"#).expect("valid owner regex"));
static RE_S3_EMPTY_BUCKETS: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r#"<Buckets\s*/>"#).expect("valid buckets regex"));

use crate::classification::{
    PersistenceMode, ServiceDurabilityClass, ServiceExecutionClass, parse_persistence_mode,
    service_durability_class, service_execution_class,
};
use crate::harness::TestHarness;
use crate::native_http::{NativeExecutionStatus, StepTrace};

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
    pub openstack_image: Option<String>,
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
        Self {
            openstack_image: std::env::var("PARITY_OPENSTACK_IMAGE").ok(),
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
        "cloudtrail",
        "cloudwatch",
        "cognito",
        "events",
        "states",
        "apigateway",
        "ec2",
        "ecs",
        "elasticache",
        "route53",
        "ses",
        "ecr",
        "opensearch",
        "rds",
        "redshift",
        "cloudformation",
        "lambda",
        "ecs",
        "rds",
        "cognito",
        "elasticache",
        "cloudtrail",
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
pub struct ScenarioResult {
    pub scenario_id: String,
    pub service: String,
    pub service_execution_class: Option<ServiceExecutionClass>,
    pub service_durability_class: Option<ServiceDurabilityClass>,
    pub passed: bool,
    pub follow_up_required: bool,
    pub native_coverage_status: String,
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
    pub follow_up_required: usize,
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
        let result = run_scenario(
            &mut manager,
            &scenario,
            profile_name,
            &known_differences,
            &run_config,
        )
        .await;
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

    let profile_path = config
        .report_dir
        .join(format!("{profile_name}-latest.json"));
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
                follow_up_required: 0,
            });
        score.total += 1;
        if result.follow_up_required {
            score.follow_up_required += 1;
        }

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
    selected_profile: &str,
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
            .post(format!(
                "{}/_localstack/health",
                manager.localstack.endpoint
            ))
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
                follow_up_required: true,
                native_coverage_status: "follow-up-required".to_string(),
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
    )
    .await;

    let localstack_traces = run_steps(
        &manager.localstack.endpoint,
        scenario,
        &mut localstack_context,
        config,
    )
    .await;

    let mut mismatches = compare_traces(
        scenario,
        selected_profile,
        &openstack_traces,
        &localstack_traces,
    );
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

    let baseline_follow_up_required = scenario.profile == "all-services-smoke";

    let follow_up_required = openstack_traces
        .iter()
        .chain(localstack_traces.iter())
        .any(|trace| trace.execution_status != NativeExecutionStatus::Executed)
        || mismatches
            .iter()
            .any(|mismatch| mismatch.kind == "native_follow_up_required")
        || (baseline_follow_up_required && unaccepted > 0);

    ScenarioResult {
        scenario_id: scenario.id.clone(),
        service: scenario.service.clone(),
        service_execution_class,
        service_durability_class,
        passed: unaccepted == 0,
        follow_up_required,
        native_coverage_status: if follow_up_required {
            "follow-up-required".to_string()
        } else {
            "native-http".to_string()
        },
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
        trace.error.contains("failed to execute aws cli")
            || trace.error.contains("Unable to locate credentials")
    });
    let localstack_env = localstack.iter().find(|trace| {
        trace.error.contains("failed to execute aws cli")
            || trace.error.contains("Unable to locate credentials")
    });

    if openstack_env.is_some() || localstack_env.is_some() {
        let openstack_msg = openstack_env
            .map(|trace| trace.error.trim().to_string())
            .unwrap_or_default();
        let localstack_msg = localstack_env
            .map(|trace| trace.error.trim().to_string())
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

async fn run_steps(
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
        let trace = crate::native_http::execute_step(
            endpoint,
            step,
            context,
            config.timeout,
            config.retries,
        )
        .await;
        traces.push(trace);
    }

    traces
}

fn compare_traces(
    scenario: &Scenario,
    selected_profile: &str,
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

        if o.execution_status != l.execution_status {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "execution_status".to_string(),
                kind: "execution_status_mismatch".to_string(),
                openstack: format!("{:?}", o.execution_status),
                localstack: format!("{:?}", l.execution_status),
                accepted_difference_id: None,
            });
        }

        if o.execution_status != NativeExecutionStatus::Executed
            || l.execution_status != NativeExecutionStatus::Executed
        {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "native_http".to_string(),
                kind: "native_follow_up_required".to_string(),
                openstack: format!("{:?}: {}", o.execution_status, o.error),
                localstack: format!("{:?}: {}", l.execution_status, l.error),
                accepted_difference_id: None,
            });
        }

        if o.status_code != l.status_code {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "status_code".to_string(),
                kind: "status_code_mismatch".to_string(),
                openstack: o
                    .status_code
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                localstack: l
                    .status_code
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                accepted_difference_id: None,
            });
        }

        let openstack_headers = normalized_headers_for_comparison(
            scenario,
            selected_profile,
            &o.step_id,
            &o.normalized_response_headers,
        );
        let localstack_headers = normalized_headers_for_comparison(
            scenario,
            selected_profile,
            &l.step_id,
            &l.normalized_response_headers,
        );
        if openstack_headers != localstack_headers {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "headers".to_string(),
                kind: "header_mismatch".to_string(),
                openstack: serde_json::to_string(&openstack_headers).unwrap_or_default(),
                localstack: serde_json::to_string(&localstack_headers).unwrap_or_default(),
                accepted_difference_id: None,
            });
        }

        let openstack_body = normalized_body_for_comparison(
            scenario,
            selected_profile,
            &o.step_id,
            &o.normalized_body,
        );
        let localstack_body = normalized_body_for_comparison(
            scenario,
            selected_profile,
            &l.step_id,
            &l.normalized_body,
        );
        if openstack_body != localstack_body {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: if !o.success && !l.success {
                    "error_body".to_string()
                } else {
                    "body".to_string()
                },
                kind: if !o.success && !l.success {
                    "error_body_mismatch".to_string()
                } else {
                    "body_mismatch".to_string()
                },
                openstack: openstack_body,
                localstack: localstack_body,
                accepted_difference_id: None,
            });
        }

        if !o.success && !l.success && o.error != l.error {
            mismatches.push(Mismatch {
                scenario_id: scenario.id.clone(),
                service: scenario.service.clone(),
                step_id: o.step_id.clone(),
                path: "error".to_string(),
                kind: "transport_error_mismatch".to_string(),
                openstack: o.error.clone(),
                localstack: l.error.clone(),
                accepted_difference_id: None,
            });
        }
    }

    mismatches
}

fn normalized_headers_for_comparison(
    scenario: &Scenario,
    selected_profile: &str,
    step_id: &str,
    headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    if !uses_broader_s3_normalization(scenario, selected_profile) {
        return headers.clone();
    }

    let mut normalized = headers.clone();
    if let Some(content_type) = normalized.get_mut("content-type") {
        if step_id == "s3-head-object"
            && matches!(
                content_type.as_str(),
                "application/octet-stream" | "binary/octet-stream"
            )
        {
            *content_type = "application/octet-stream".to_string();
        }

        if step_id == "s3-list-buckets"
            && matches!(content_type.as_str(), "text/xml" | "application/xml")
        {
            *content_type = "application/xml".to_string();
        }
    }

    normalized
}

fn normalized_body_for_comparison(
    scenario: &Scenario,
    selected_profile: &str,
    step_id: &str,
    body: &str,
) -> String {
    if !uses_broader_s3_normalization(scenario, selected_profile) || step_id != "s3-list-buckets" {
        return body.to_string();
    }

    let without_owner = RE_S3_OWNER.replace_all(body, "<Owner></Owner>").to_string();
    RE_S3_EMPTY_BUCKETS
        .replace_all(&without_owner, "<Buckets></Buckets>")
        .to_string()
}

fn uses_broader_s3_normalization(scenario: &Scenario, selected_profile: &str) -> bool {
    selected_profile == "extended" && scenario.service == "s3"
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
    openstack_container_id: Option<String>,
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

        let (openstack, openstack_harness, openstack_container_id) =
            if let Some(endpoint) = &config.openstack_endpoint {
                (
                    ManagedTarget {
                        endpoint: endpoint.clone(),
                    },
                    None,
                    None,
                )
            } else if let Some(image) = &config.openstack_image {
                let port = free_port()?;
                let endpoint = format!("http://127.0.0.1:{port}");
                let output = Command::new("docker")
                    .args([
                        "run",
                        "-d",
                        "-p",
                        &format!("127.0.0.1:{port}:4566"),
                        "-e",
                        &format!("SERVICES={services}"),
                        "-e",
                        "DEBUG=1",
                        image,
                    ])
                    .output()?;

                if !output.status.success() {
                    return Err(anyhow::anyhow!(
                        "failed to start openstack image target: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                wait_for_health(&endpoint, Duration::from_secs(60)).await?;
                (ManagedTarget { endpoint }, None, Some(container_id))
            } else {
                let harness = TestHarness::start_services(&services).await;
                let endpoint = harness.base_url.clone();
                (ManagedTarget { endpoint }, Some(harness), None)
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
            openstack_container_id,
            localstack_container_id: container_id,
            openstack_harness,
        })
    }

    pub async fn stop(&mut self) {
        if let Some(container_id) = &self.openstack_container_id {
            let _ = Command::new("docker")
                .args(["rm", "-f", container_id])
                .output();
        }
        self.openstack_container_id = None;

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
    use std::collections::BTreeSet;

    use super::{Mismatch, ScenarioResult, summarize_results};
    use crate::classification::{ServiceDurabilityClass, ServiceExecutionClass};
    use crate::native_http::normalize_xml;

    #[test]
    fn normalize_xml_removes_additional_error_details_footer() {
        let raw = "aws: [ERROR]: An error occurred (InternalFailure) when calling the ListTopics operation: Service 'sns' is not enabled. Please check your 'SERVICES' configuration variable.\n\nAdditional error details:\nType: Sender\n";

        let normalized = normalize_xml(raw);

        assert!(normalized.contains("Service 'sns' is not enabled"));
        assert!(!normalized.contains('\n'));
    }

    #[test]
    fn summarize_results_collects_persistence_failure_classes() {
        let result = ScenarioResult {
            scenario_id: "s3-persistence-restart".to_string(),
            service: "s3".to_string(),
            service_execution_class: Some(ServiceExecutionClass::InProcStateful),
            service_durability_class: Some(ServiceDurabilityClass::Durable),
            passed: false,
            follow_up_required: false,
            native_coverage_status: "native-http".to_string(),
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
            summary
                .persistence_failure_classes
                .get("persistence_mode_mismatch"),
            Some(&1)
        );
    }

    #[test]
    fn readme_supported_services_match_all_services_smoke_inventory() {
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repo root should resolve");
        let readme = std::fs::read_to_string(repo_root.join("README.md"))
            .expect("README should be readable");
        let mut readme_services = BTreeSet::new();

        for line in readme.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("| ")
                || trimmed.starts_with("| Service ")
                || trimmed.starts_with("|---")
            {
                continue;
            }

            let parts = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
            if parts.len() < 3 {
                continue;
            }

            let service = match parts[1] {
                "S3" => "s3",
                "Simple Queue Service (SQS)" => "sqs",
                "Simple Notification Service (SNS)" => "sns",
                "DynamoDB" => "dynamodb",
                // DynamoDB Streams is served by the same crate/service ID
                "DynamoDB Streams" => "dynamodb",
                "Lambda" => "lambda",
                "Identity and Access Management (IAM)" => "iam",
                "Security Token Service (STS)" => "sts",
                "Key Management Service (KMS)" => "kms",
                "Secrets Manager" => "secretsmanager",
                "Systems Manager (SSM)" => "ssm",
                "Certificate Manager (ACM)" => "acm",
                "Kinesis Data Streams" => "kinesis",
                "Data Firehose" => "firehose",
                "CloudFormation" => "cloudformation",
                "CloudTrail" => "cloudtrail",
                // Both CloudWatch rows (metrics+alarms and Logs) share one service ID;
                // the BTreeSet deduplicates the second insertion automatically.
                "CloudWatch (metrics + alarms)" => "cloudwatch",
                "CloudWatch Logs" => "cloudwatch",
                "Cognito" => "cognito",
                "EventBridge" => "events",
                "Step Functions" => "states",
                "API Gateway" => "apigateway",
                "EC2" => "ec2",
                "Elastic Container Service (ECS)" => "ecs",
                "ElastiCache" => "elasticache",
                "Route 53" => "route53",
                "Simple Email Service (SES)" => "ses",
                "ECR" => "ecr",
                "OpenSearch Service" => "opensearch",
                "Relational Database Service (RDS)" => "rds",
                "Redshift" => "redshift",
                _ => continue,
            };
            readme_services.insert(service.to_string());
        }

        let smoke = std::fs::read_to_string(
            repo_root.join("tests/parity/scenarios/all-services-smoke.json"),
        )
        .expect("all-services-smoke scenario file should be readable");
        let scenarios: Vec<super::Scenario> =
            serde_json::from_str(&smoke).expect("all-services-smoke scenarios should parse");
        let smoke_services = scenarios
            .into_iter()
            .map(|scenario| scenario.service)
            .collect::<BTreeSet<_>>();
        let configured_services = super::all_service_names()
            .into_iter()
            .collect::<BTreeSet<_>>();

        assert_eq!(configured_services, smoke_services);
        assert_eq!(readme_services, smoke_services);
    }
}
