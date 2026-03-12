## MODIFIED Requirements

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each scenario and target, SHALL compute comparative metrics between openstack and LocalStack, and SHALL emit service-level optimization summaries suitable for remediation tracking.

#### Scenario: Per-scenario metrics are captured
- **WHEN** a benchmark scenario completes
- **THEN** the report SHALL include latency distribution metrics (including p50 and p95), throughput, operation count, and error count for each target

#### Scenario: Comparative deltas are emitted
- **WHEN** benchmark results for both targets are available for a scenario
- **THEN** the report SHALL include openstack-versus-localstack delta and ratio metrics for key latency and throughput measures

#### Scenario: Service-level optimization summary is available
- **WHEN** a benchmark run summary is emitted
- **THEN** the report SHALL include per-service comparison aggregates that can be used to track remediation progress over time

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support benchmark execution profiles that include broad all-services realistic coverage and deeper workloads for selected high-impact services, and SHALL maintain valid write and read performance scenarios for each supported service in required broad coverage lanes.

#### Scenario: All-services realistic profile covers enabled service set
- **WHEN** the all-services realistic benchmark profile is requested
- **THEN** the harness SHALL execute representative realistic benchmark scenarios for every configured benchmark service

#### Scenario: Every service includes write and read realistic scenarios
- **WHEN** required all-services realistic lanes run
- **THEN** each supported service SHALL have at least one measured write/mutate scenario and one measured read/query/list/describe scenario result, or an explicit machine-readable exclusion

#### Scenario: Deep profile targets high-impact service workloads
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute additional workload scenarios for designated high-impact services with larger payloads and/or higher operation volume

#### Scenario: Broad lane scenario validity is enforced
- **WHEN** all-services benchmark lanes run
- **THEN** each supported service SHALL have valid realistic performance scenario coverage for required write/read roles or an explicit machine-readable exclusion reason

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark scenario against both openstack and LocalStack targets using equivalent request inputs and benchmark configuration. Benchmark execution SHALL include explicit persistence-mode metadata and SHALL reject non-equivalent mode comparisons for interpretable performance claims. In CI-managed runtime mode, benchmark runs SHALL consume a deterministic run-scoped OpenStack runtime image reference rather than a floating image tag.

#### Scenario: Equivalent scenario workload is executed on both targets
- **WHEN** a benchmark scenario is selected for execution
- **THEN** the harness SHALL run the same setup, workload, and cleanup steps against openstack and LocalStack with only endpoint/runtime connection settings varying by target

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run starts
- **THEN** the harness SHALL record target metadata including endpoint and LocalStack image/version (when available) in the benchmark report

#### Scenario: Non-equivalent persistence modes are marked invalid
- **WHEN** openstack and LocalStack are configured with non-equivalent persistence modes for a comparative lane
- **THEN** the lane SHALL be marked non-interpretable with `mode_mismatch` diagnostics

#### Scenario: CI-managed runtime mode uses deterministic openstack image reference
- **WHEN** benchmark execution starts in CI-managed runtime mode
- **THEN** the harness SHALL launch OpenStack benchmark targets using the immutable runtime image reference produced for that workflow run and SHALL NOT resolve the image from a floating `latest` tag

### Requirement: Reproducibility and fairness controls
The system SHALL provide benchmark execution controls that improve reproducibility and reduce biased comparisons.

#### Scenario: Warmup is excluded from measured results
- **WHEN** a scenario defines warmup iterations
- **THEN** the harness SHALL execute warmup operations before measurement and SHALL exclude warmup timing from reported benchmark metrics

#### Scenario: Controlled iteration and concurrency settings are applied
- **WHEN** benchmark configuration specifies iteration count and concurrency level
- **THEN** the harness SHALL apply those settings identically for both targets during measured execution

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in a machine-readable format suitable for automation and trend analysis, and SHALL publish readable consolidated CI summaries across benchmark lanes. Reports SHALL include per-service class, persistence mode, and lane interpretability fields.

#### Scenario: Benchmark report is written to disk
- **WHEN** a benchmark run completes
- **THEN** the harness SHALL write a JSON report containing run metadata, profile name, per-scenario metrics, and aggregate summary metrics

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

### Requirement: Symmetric benchmark runtime for dual-target comparison
The benchmark system SHALL execute openstack and LocalStack in equivalent containerized runtime environments for comparative benchmark runs.

#### Scenario: Both targets run with equivalent resource constraints
- **WHEN** a fairness-mode benchmark run is started
- **THEN** openstack and LocalStack SHALL each run in Docker with identical configured CPU and memory limits before scenarios are executed

#### Scenario: Benchmark run records fairness runtime metadata
- **WHEN** a fairness-mode benchmark run completes
- **THEN** the benchmark report SHALL include target runtime metadata including container image/tag, CPU limit, memory limit, and network mode for both targets

### Requirement: Tiered load profiles across benchmark services
The benchmark system SHALL support low, medium, high, and extreme load tiers so benchmarked services can be evaluated across a broad operating range.

#### Scenario: Tiered profiles define per-service workload parameters
- **WHEN** a benchmark profile is resolved
- **THEN** each included service scenario SHALL define load-tier-specific iteration count, operation count, concurrency, and payload or record-size parameters

#### Scenario: Report includes load tier for each scenario result
- **WHEN** scenario results are emitted
- **THEN** each result SHALL include the load tier identifier used for that scenario

### Requirement: Separation of coverage probes and performance scenarios
The benchmark system SHALL classify scenarios as coverage or performance and SHALL compute performance comparison summaries using only performance-classified scenarios.

#### Scenario: Coverage scenarios are excluded from comparative performance summary
- **WHEN** a benchmark report summary is generated
- **THEN** scenarios classified as coverage SHALL NOT contribute to aggregate latency-ratio or throughput-ratio performance summary metrics

#### Scenario: Scenario class is captured in results
- **WHEN** an individual scenario result is recorded
- **THEN** the result SHALL include a scenario class field with value `coverage` or `performance`

### Requirement: S3 heavy-object benchmark validation
The benchmark system SHALL include S3 performance scenarios that validate object handling at 1 GB, 5 GB, and 10 GB sizes.

#### Scenario: S3 1 GB benchmark scenario executes successfully
- **WHEN** the S3 heavy-object benchmark tier is run
- **THEN** the harness SHALL execute a 1 GB object put/get validation scenario against both openstack and LocalStack and record comparative metrics

#### Scenario: S3 5 GB benchmark scenario executes successfully
- **WHEN** the S3 heavy-object benchmark tier is run
- **THEN** the harness SHALL execute a 5 GB object put/get validation scenario against both openstack and LocalStack and record comparative metrics

#### Scenario: S3 10 GB benchmark scenario executes successfully
- **WHEN** the S3 heavy-object benchmark tier is run
- **THEN** the harness SHALL execute a 10 GB object put/get validation scenario against both openstack and LocalStack and record comparative metrics

#### Scenario: Large-object tests are guard-railed by execution policy
- **WHEN** the benchmark environment does not meet configured runtime requirements for heavy-object tiers
- **THEN** the harness SHALL mark 1 GB, 5 GB, and 10 GB S3 scenarios as skipped with explicit skip reasons in the report
