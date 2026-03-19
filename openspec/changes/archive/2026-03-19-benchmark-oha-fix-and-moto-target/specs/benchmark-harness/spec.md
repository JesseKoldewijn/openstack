## MODIFIED Requirements

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each scenario and target using native HTTP execution via `oha` with `--output-format json`, SHALL extract latency percentiles from `latencyPercentiles.p50`, `latencyPercentiles.p95`, `latencyPercentiles.p99` and throughput from `summary.requestsPerSec` in oha's JSON output, SHALL compute comparative metrics between openstack and all active comparison targets (LocalStack and/or moto), and SHALL emit service-level optimization summaries suitable for remediation tracking.

#### Scenario: Per-scenario metrics are captured
- **WHEN** a benchmark scenario completes
- **THEN** the report SHALL include latency distribution metrics (including p50, p95, and p99), throughput, operation count, and error count for each active target

#### Scenario: Comparative deltas are emitted
- **WHEN** benchmark results for openstack and at least one comparison target are available for a scenario
- **THEN** the report SHALL include openstack-versus-comparison-target delta and ratio metrics for key latency and throughput measures for each active comparison target

#### Scenario: Service-level optimization summary is available
- **WHEN** a benchmark run summary is emitted
- **THEN** the report SHALL include per-service comparison aggregates that can be used to track remediation progress over time

#### Scenario: Client process overhead is excluded from benchmark transport
- **WHEN** benchmark measurements are collected for supported native scenarios
- **THEN** the execution path SHALL NOT include spawned AWS CLI process overhead in measured operation timing

#### Scenario: oha is invoked with correct output format flag
- **WHEN** the harness runs oha to benchmark an operation
- **THEN** the harness SHALL pass `--output-format json` (not `--json`) to oha and SHALL extract p50 from `.latencyPercentiles.p50`, p95 from `.latencyPercentiles.p95`, p99 from `.latencyPercentiles.p99`, and throughput from `.summary.requestsPerSec`

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark scenario against openstack and all active comparison targets (LocalStack and/or moto as determined by `--targets`) using equivalent native HTTP request inputs and benchmark configuration. A `bench_targets()` function SHALL iterate over all active targets, invoking the bench function for each. Benchmark execution SHALL include explicit persistence-mode metadata, SHALL reject non-equivalent mode comparisons for interpretable performance claims, and in CI-managed runtime mode SHALL consume a deterministic run-scoped OpenStack runtime image reference rather than a floating image tag.

#### Scenario: Equivalent scenario workload is executed on all active targets
- **WHEN** a benchmark scenario is selected for execution
- **THEN** the harness SHALL run the same setup, workload, and cleanup steps against openstack and each active comparison target with only endpoint/runtime connection settings varying by target

#### Scenario: bench_targets iterates over active targets
- **WHEN** a per-service benchmark section calls `bench_targets()`
- **THEN** the function SHALL invoke `bench()` for each target present in the `TARGETS` variable, passing the appropriate URL for that target, and SHALL skip targets not in the active set

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run starts
- **THEN** the harness SHALL record target metadata including endpoint and image/version (when available) for each active target in the benchmark report

#### Scenario: Non-equivalent persistence modes are marked invalid
- **WHEN** openstack and a comparison target are configured with non-equivalent persistence modes for a comparative lane
- **THEN** the lane SHALL be marked non-interpretable with `mode_mismatch` diagnostics

#### Scenario: CI-managed runtime mode uses deterministic openstack image reference
- **WHEN** benchmark execution starts in CI-managed runtime mode
- **THEN** the harness SHALL launch OpenStack benchmark targets using the immutable runtime image reference produced for that workflow run and SHALL NOT resolve the image from a floating `latest` tag

#### Scenario: AWS CLI is not required for benchmark execution
- **WHEN** a benchmark lane is executed with supported native translators available
- **THEN** the harness SHALL execute benchmark workloads without spawning AWS CLI processes

### Requirement: Symmetric benchmark runtime for dual-target comparison
The benchmark system SHALL execute openstack and all active comparison targets in equivalent containerized runtime environments for comparative benchmark runs.

#### Scenario: All active targets run with equivalent resource constraints
- **WHEN** a fairness-mode benchmark run is started
- **THEN** openstack and each active comparison target SHALL each run in Docker with identical configured CPU and memory limits before scenarios are executed

#### Scenario: Benchmark run records fairness runtime metadata
- **WHEN** a fairness-mode benchmark run completes
- **THEN** the benchmark report SHALL include target runtime metadata including container image/tag, CPU limit, memory limit, and network mode for each active target

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in a machine-readable format suitable for automation and trend analysis, and SHALL publish readable consolidated CI summaries across benchmark lanes. Reports SHALL include per-service class, persistence mode, and lane interpretability fields, and SHALL distinguish product/runtime behavior gaps from harness limitations, configuration defects, and unsound scenario contracts.

#### Scenario: Benchmark report is written to disk
- **WHEN** a benchmark run completes
- **THEN** the harness SHALL write a JSON report containing run metadata, profile name, per-scenario metrics for all active targets, and aggregate summary metrics

#### Scenario: Missing runtime evidence remains explicit
- **WHEN** a benchmark lane cannot collect runtime evidence such as memory RSS for one target
- **THEN** the report SHALL record the missing target explicitly rather than implying full observability coverage

#### Scenario: In-process runtime limitations are classified explicitly
- **WHEN** OpenStack runs in-process and container-based memory inspection is unavailable
- **THEN** the benchmark report SHALL classify the missing memory evidence as an explicit runtime-observability limitation rather than a silent omission or generic product failure

#### Scenario: Invalid scenario reason distinguishes contract defects
- **WHEN** a scenario is invalidated due to unsound setup, missing seeded state, or non-portable target-specific identifiers
- **THEN** the report SHALL record a machine-readable invalid reason that distinguishes scenario-contract defects from product behavior failures

#### Scenario: CI can publish benchmark artifacts
- **WHEN** benchmark mode is executed in CI
- **THEN** the generated report SHALL be available as a build artifact for downstream analysis

#### Scenario: CI publishes consolidated benchmark summary
- **WHEN** one or more fairness benchmark lanes complete in a CI run
- **THEN** CI SHALL generate a single consolidated markdown summary artifact that includes each available fairness lane and its key benchmark metrics

#### Scenario: Consolidated summary reports gate outcomes for required lanes
- **WHEN** required benchmark lane gate evaluation completes
- **THEN** the consolidated summary SHALL include explicit gate pass/fail outcomes with threshold context for each required lane

#### Scenario: Summary includes class and mode context
- **WHEN** consolidated reporting is generated
- **THEN** each required lane summary SHALL include service class and persistence mode context for interpreted metrics
