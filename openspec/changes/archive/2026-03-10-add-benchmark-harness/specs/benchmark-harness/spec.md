## ADDED Requirements

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark scenario against both openstack and LocalStack targets using equivalent request inputs and benchmark configuration.

#### Scenario: Equivalent scenario workload is executed on both targets
- **WHEN** a benchmark scenario is selected for execution
- **THEN** the harness SHALL run the same setup, workload, and cleanup steps against openstack and LocalStack with only endpoint/runtime connection settings varying by target

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run starts
- **THEN** the harness SHALL record target metadata including endpoint and LocalStack image/version (when available) in the benchmark report

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support benchmark execution profiles that include broad all-services smoke coverage and deeper workloads for selected high-impact services.

#### Scenario: All-services smoke profile covers enabled service set
- **WHEN** the all-services smoke benchmark profile is requested
- **THEN** the harness SHALL execute at least one representative benchmark scenario for each configured benchmark service

#### Scenario: Deep profile targets high-impact service workloads
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute additional workload scenarios for designated high-impact services with larger payloads and/or higher operation volume

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each scenario and target, and SHALL compute comparative metrics between openstack and LocalStack.

#### Scenario: Per-scenario metrics are captured
- **WHEN** a benchmark scenario completes
- **THEN** the report SHALL include latency distribution metrics (including p50 and p95), throughput, operation count, and error count for each target

#### Scenario: Comparative deltas are emitted
- **WHEN** benchmark results for both targets are available for a scenario
- **THEN** the report SHALL include openstack-versus-localstack delta and ratio metrics for key latency and throughput measures

### Requirement: Reproducibility and fairness controls
The system SHALL provide benchmark execution controls that improve reproducibility and reduce biased comparisons.

#### Scenario: Warmup is excluded from measured results
- **WHEN** a scenario defines warmup iterations
- **THEN** the harness SHALL execute warmup operations before measurement and SHALL exclude warmup timing from reported benchmark metrics

#### Scenario: Controlled iteration and concurrency settings are applied
- **WHEN** benchmark configuration specifies iteration count and concurrency level
- **THEN** the harness SHALL apply those settings identically for both targets during measured execution

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in a machine-readable format suitable for automation and trend analysis.

#### Scenario: Benchmark report is written to disk
- **WHEN** a benchmark run completes
- **THEN** the harness SHALL write a JSON report containing run metadata, profile name, per-scenario metrics, and aggregate summary metrics

#### Scenario: CI can publish benchmark artifacts
- **WHEN** benchmark mode is executed in CI
- **THEN** the generated report SHALL be available as a build artifact for downstream analysis
