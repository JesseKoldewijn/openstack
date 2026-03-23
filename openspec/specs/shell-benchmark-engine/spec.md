## Purpose
Shell-based benchmark engine that executes HTTP-based benchmarks against the 8 core AWS parity services using oha/hey.

## Requirements

### Requirement: Shell-based benchmark script SHALL benchmark the 8 core parity services
The benchmark system SHALL provide a single shell script (`tests/bench/bench_services.sh`) that executes HTTP-based benchmarks against the 8 core parity services (dynamodb, firehose, iam, kinesis, s3, secretsmanager, sns, sts), with each service exercising multiple representative operations covering write/mutate and read/query patterns.

#### Scenario: Script benchmarks the 8 core services in default profile
- **WHEN** the benchmark script is invoked with `--profile default`
- **THEN** the script SHALL execute benchmark operations for all 8 core parity services: dynamodb, firehose, iam, kinesis, s3, secretsmanager, sns, sts

#### Scenario: Each service exercises at least one write and one read operation
- **WHEN** a service benchmark section executes
- **THEN** the script SHALL run at least one write/mutate operation and one read/query/list operation for that service

#### Scenario: Each service seeds required resources before benchmarking
- **WHEN** a service benchmark section begins
- **THEN** the script SHALL create any prerequisite resources (tables, buckets, streams, topics, etc.) needed for the measured operations

#### Scenario: Seed failure skips the service gracefully
- **WHEN** a service seed operation fails
- **THEN** the script SHALL skip the remaining operations for that service, log the failure, and record a skip entry in the JSON output

### Requirement: Benchmark script SHALL support profile-based execution
The benchmark system SHALL support named profiles that control the load parameters applied.

#### Scenario: Default profile benchmarks core services with moderate load
- **WHEN** the script is invoked with `--profile default` (or without `--profile`)
- **THEN** the script SHALL benchmark the 8 core parity services with 100 requests and 2 concurrent connections

#### Scenario: Stress profile benchmarks core services with heavy load
- **WHEN** the script is invoked with `--profile stress`
- **THEN** the script SHALL benchmark the 8 core parity services with 1000 requests and 6 concurrent connections

#### Scenario: Custom service filter overrides profile service list
- **WHEN** the script is invoked with `--services s3,dynamodb,iam`
- **THEN** the script SHALL benchmark only the specified services, using load parameters from the active profile

#### Scenario: Custom request and concurrency flags override profile defaults
- **WHEN** the script is invoked with `--requests 500 --concurrency 8`
- **THEN** the script SHALL use the specified values instead of the profile's defaults

### Requirement: Benchmark script SHALL support dual runtime modes
The benchmark system SHALL support running targets in Docker containers for fair comparison and optionally running openstack as a bare binary for performance showcase.

#### Scenario: Docker mode starts both targets in equivalent containers
- **WHEN** the script runs in default Docker mode
- **THEN** the script SHALL start openstack and LocalStack in Docker containers with identical configured CPU and memory limits

#### Scenario: Binary mode runs openstack as a bare process
- **WHEN** the script is invoked with `--binary`
- **THEN** the script SHALL run the openstack binary directly as a process and LocalStack in Docker

#### Scenario: Docker resource limits are configurable
- **WHEN** environment variables or flags specify CPU and memory limits
- **THEN** the script SHALL apply those limits to both Docker containers

### Requirement: Benchmark script SHALL use oha with hey as fallback
The benchmark system SHALL use the `oha` HTTP benchmarking tool for executing load tests, falling back to `hey` if `oha` is unavailable.

#### Scenario: Script selects oha when available
- **WHEN** `oha` is found in PATH
- **THEN** the script SHALL use `oha` with `--output-format json` for all benchmark operations

#### Scenario: Script falls back to hey when oha is unavailable
- **WHEN** `oha` is not found in PATH but `hey` is
- **THEN** the script SHALL use `hey` and parse its text output to extract metrics

#### Scenario: Script fails if neither tool is available
- **WHEN** neither `oha` nor `hey` is found in PATH
- **THEN** the script SHALL exit with a non-zero status and a message listing installation instructions for both tools

### Requirement: Benchmark script SHALL output structured JSON results with raw metrics
The benchmark system SHALL write a JSON report containing raw per-operation metrics for each benchmarked service and operation, without weighted averages or cross-service aggregation.

#### Scenario: JSON report contains per-operation raw metrics
- **WHEN** a benchmark run completes
- **THEN** the JSON report SHALL include for each operation: p50, p95, p99 latency in milliseconds, throughput in requests per second, error count, and total request count, for each target

#### Scenario: JSON report contains no weighted or aggregated metrics
- **WHEN** a benchmark report is generated
- **THEN** the report SHALL NOT include weighted averages, cross-service aggregate ratios, or any computed summary statistics that combine metrics across different services or operations

#### Scenario: JSON report includes run metadata
- **WHEN** a benchmark run completes
- **THEN** the JSON report SHALL include profile name, runtime mode, timestamp, request count, concurrency, and target configuration (container images, resource limits)

#### Scenario: JSON report is written to configurable output path
- **WHEN** the script is invoked with `--output /path/to/report.json`
- **THEN** the report SHALL be written to the specified path

#### Scenario: JSON report is written to stdout when no output path specified
- **WHEN** the script is invoked without `--output`
- **THEN** the report JSON SHALL be written to stdout

### Requirement: Benchmark script SHALL measure memory usage
The benchmark system SHALL capture memory usage for each target at idle (post-startup, pre-load) and post-load phases.

#### Scenario: Idle memory is captured after startup
- **WHEN** targets are started and healthy
- **THEN** the script SHALL record idle RSS memory for each target before running benchmark operations

#### Scenario: Post-load memory is captured after benchmarks complete
- **WHEN** all benchmark operations complete
- **THEN** the script SHALL record post-load RSS memory for each target

#### Scenario: Docker mode uses docker stats for memory
- **WHEN** targets run in Docker containers
- **THEN** the script SHALL use `docker stats` to collect container RSS

#### Scenario: Binary mode uses proc filesystem for openstack memory
- **WHEN** openstack runs as a bare binary
- **THEN** the script SHALL use `/proc/<pid>/status` VmRSS (Linux) or `ps` RSS (macOS) for openstack memory measurement

### Requirement: Benchmark script SHALL work identically in CI and locally
The benchmark system SHALL be invocable with the same interface in CI workflows and local development environments.

#### Scenario: Script requires no CI-specific environment
- **WHEN** the script is invoked locally with required tools installed (oha/hey, docker, jq)
- **THEN** the script SHALL execute the same benchmark operations and produce the same JSON output format as in CI

#### Scenario: Script documents prerequisites
- **WHEN** a required tool is missing
- **THEN** the script SHALL print a clear error message listing missing prerequisites and installation instructions
