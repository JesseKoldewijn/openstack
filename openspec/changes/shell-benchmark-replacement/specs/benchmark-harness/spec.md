## MODIFIED Requirements

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each operation and target using an external HTTP benchmarking tool (oha or hey), SHALL report raw per-operation comparative metrics between openstack and LocalStack, and SHALL NOT compute weighted averages or cross-service aggregate summaries.

#### Scenario: Per-operation metrics are captured
- **WHEN** a benchmark operation completes
- **THEN** the report SHALL include raw latency distribution metrics (p50, p95, p99), throughput in requests per second, error count, and total request count for each target

#### Scenario: Comparative metrics are per-operation
- **WHEN** benchmark results for both targets are available for an operation
- **THEN** the report SHALL include the raw metrics for both targets side-by-side, without computing weighted or aggregated ratios across operations

#### Scenario: No weighted averages are produced
- **WHEN** a benchmark run summary is emitted
- **THEN** the report SHALL NOT include weighted cross-service aggregate metrics, service-level comparison aggregates, or any summary statistics that combine different operations or services

#### Scenario: External HTTP tool is used for measurement
- **WHEN** benchmark measurements are collected
- **THEN** the execution path SHALL use oha (preferred) or hey (fallback) as the HTTP benchmarking tool, NOT in-process Rust code or AWS CLI processes

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support three benchmark execution profiles (smoke, standard, deep) that control service scope and load parameters. All profiles that include a service SHALL exercise at least one write and one read operation for that service.

#### Scenario: Smoke profile covers core services with light load
- **WHEN** the smoke benchmark profile is requested
- **THEN** the harness SHALL execute benchmark operations for the 8 core parity services with reduced request count and concurrency

#### Scenario: Standard profile covers all services with medium load
- **WHEN** the standard benchmark profile is requested
- **THEN** the harness SHALL execute benchmark operations for all 24 services with moderate request count and concurrency

#### Scenario: Deep profile covers all services with heavy load
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute benchmark operations for all 24 services with high request count and increased concurrency

#### Scenario: Every benchmarked service includes write and read operations
- **WHEN** any profile runs
- **THEN** each included service SHALL have at least one write/mutate operation and one read/query/list operation benchmarked

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark operation against both openstack and LocalStack using equivalent HTTP requests. In Docker mode, both targets SHALL run with identical resource constraints. In binary mode, openstack runs as a bare process while LocalStack runs in Docker.

#### Scenario: Docker mode applies equivalent constraints to both targets
- **WHEN** a benchmark run starts in Docker mode
- **THEN** both openstack and LocalStack SHALL run in Docker with identical configured CPU and memory limits

#### Scenario: Binary mode runs openstack as bare process
- **WHEN** a benchmark run starts in binary mode
- **THEN** openstack SHALL run as a bare binary process and LocalStack SHALL run in Docker

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run completes
- **THEN** the report SHALL record target metadata including runtime mode, container images (if Docker), and resource limits

### Requirement: Reproducibility and fairness controls
The system SHALL provide benchmark execution controls that improve reproducibility. Request count and concurrency SHALL be configurable and applied identically to both targets.

#### Scenario: Same request count and concurrency applied to both targets
- **WHEN** benchmark configuration specifies request count and concurrency
- **THEN** the harness SHALL apply those settings identically for both targets during execution

#### Scenario: Each service seeds its own prerequisite resources
- **WHEN** a service benchmark section begins
- **THEN** the script SHALL create prerequisite resources before measured operations and handle seed failures gracefully

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in JSON format containing raw per-operation metrics, run metadata, and memory measurements. Reports SHALL NOT include weighted averages or cross-service aggregation.

#### Scenario: Benchmark report is written as JSON
- **WHEN** a benchmark run completes
- **THEN** the harness SHALL write a JSON report containing run metadata, per-operation raw metrics for each target, and memory measurements

#### Scenario: CI can publish benchmark artifacts
- **WHEN** benchmark mode is executed in CI
- **THEN** the generated JSON report SHALL be available as a build artifact

#### Scenario: CI publishes markdown summary as PR comment
- **WHEN** benchmark operations complete in a CI run
- **THEN** CI SHALL generate a markdown summary with raw per-operation metrics and post it as a PR comment

### Requirement: Symmetric benchmark runtime for dual-target comparison
The benchmark system SHALL execute openstack and LocalStack in equivalent containerized runtime environments for comparative benchmark runs in Docker mode.

#### Scenario: Both targets run with equivalent resource constraints in Docker mode
- **WHEN** a Docker-mode benchmark run is started
- **THEN** openstack and LocalStack SHALL each run in Docker with identical configured CPU and memory limits

#### Scenario: Benchmark run records runtime metadata
- **WHEN** a Docker-mode benchmark run completes
- **THEN** the benchmark report SHALL include container image, CPU limit, memory limit, and network mode for both targets

## REMOVED Requirements

### Requirement: Tiered load profiles across benchmark services
**Reason**: Replaced by the simplified three-profile system (smoke/standard/deep). The per-service load-tier metadata and per-scenario load-tier fields are no longer needed — profiles define load parameters uniformly across all included services.
**Migration**: Use `--profile smoke|standard|deep` or `--requests`/`--concurrency` flags for load control.

### Requirement: Separation of coverage probes and performance scenarios
**Reason**: The new shell-based system does not distinguish coverage vs performance scenarios. All operations are measured with the same HTTP benchmarking tool. Coverage-only probes were an artifact of the Rust engine's classification system.
**Migration**: All operations produce performance metrics. No separate classification needed.

### Requirement: S3 heavy-object benchmark validation
**Reason**: Deferred to future work. The 1GB/5GB/10GB S3 object benchmarks require significant disk/memory resources and add complexity to the shell script. Can be added as a separate `--heavy-objects` flag later.
**Migration**: Not available in the new system initially. Track as future enhancement.
