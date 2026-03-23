## MODIFIED Requirements

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each scenario and target using native HTTP execution, SHALL compute comparative metrics between openstack and LocalStack, and SHALL emit service-level optimization summaries suitable for remediation tracking.

#### Scenario: Per-scenario metrics are captured
- **WHEN** a benchmark scenario completes
- **THEN** the report SHALL include latency distribution metrics (including p50 and p95), throughput, operation count, and error count for each target

#### Scenario: Comparative deltas are emitted
- **WHEN** benchmark results for both targets are available for a scenario
- **THEN** the report SHALL include openstack-versus-localstack delta and ratio metrics for key latency and throughput measures

#### Scenario: Service-level optimization summary is available
- **WHEN** a benchmark run summary is emitted
- **THEN** the report SHALL include per-service comparison aggregates that can be used to track remediation progress over time

#### Scenario: Client process overhead is excluded from benchmark transport
- **WHEN** benchmark measurements are collected for supported native scenarios
- **THEN** the execution path SHALL NOT include spawned AWS CLI process overhead in measured operation timing

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support benchmark execution profiles that include broad all-services realistic coverage and deeper workloads for selected high-impact services, and SHALL maintain valid native HTTP write and read performance scenarios for each supported service in required broad coverage lanes across the 24 services listed in `README.md`.

#### Scenario: All-services realistic profile covers enabled service set
- **WHEN** the all-services realistic benchmark profile is requested
- **THEN** the harness SHALL execute representative realistic benchmark scenarios for every configured benchmark service

#### Scenario: Every service includes write and read realistic scenarios
- **WHEN** required all-services realistic lanes run
- **THEN** each supported service SHALL have at least one measured write or mutate scenario and one measured read, query, list, or describe scenario result, or an explicit machine-readable exclusion

#### Scenario: Deep profile targets high-impact service workloads
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute additional workload scenarios for designated high-impact services with larger payloads and or higher operation volume

#### Scenario: Broad lane scenario validity is enforced
- **WHEN** all-services benchmark lanes run
- **THEN** each supported service SHALL have valid realistic performance scenario coverage for required write and read roles or an explicit machine-readable exclusion reason

#### Scenario: README service baseline remains visible during migration
- **WHEN** a broad all-services benchmark lane completes
- **THEN** the report SHALL account for each service listed in `README.md`, including services that currently require follow-up due to missing native transport support or invalid workload semantics

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark scenario against both openstack and LocalStack targets using equivalent native HTTP request inputs and benchmark configuration. Benchmark execution SHALL include explicit persistence-mode metadata, SHALL reject non-equivalent mode comparisons for interpretable performance claims, and in CI-managed runtime mode SHALL consume a deterministic run-scoped OpenStack runtime image reference rather than a floating image tag.

#### Scenario: Equivalent scenario workload is executed on both targets
- **WHEN** a benchmark scenario is selected for execution
- **THEN** the harness SHALL run the same setup, workload, and cleanup steps against openstack and LocalStack with only endpoint/runtime connection settings varying by target

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run starts
- **THEN** the harness SHALL record target metadata including endpoint and LocalStack image or version when available in the benchmark report

#### Scenario: Non-equivalent persistence modes are marked invalid
- **WHEN** openstack and LocalStack are configured with non-equivalent persistence modes for a comparative lane
- **THEN** the lane SHALL be marked non-interpretable with `mode_mismatch` diagnostics

#### Scenario: CI-managed runtime mode uses deterministic openstack image reference
- **WHEN** benchmark execution starts in CI-managed runtime mode
- **THEN** the harness SHALL launch OpenStack benchmark targets using the immutable runtime image reference produced for that workflow run and SHALL NOT resolve the image from a floating `latest` tag

#### Scenario: AWS CLI is not required for benchmark execution
- **WHEN** a benchmark lane is executed with supported native translators available
- **THEN** the harness SHALL execute benchmark workloads without spawning AWS CLI processes
