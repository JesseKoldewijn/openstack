## ADDED Requirements

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
