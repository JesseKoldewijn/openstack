## Purpose
Shell-based benchmark gate that evaluates within-run comparisons for CI pass/fail decisions.

## Requirements

### Requirement: Benchmark gate SHALL evaluate within-run comparisons only
The benchmark gate SHALL determine pass/fail by comparing openstack metrics against LocalStack metrics from the same run. The gate SHALL NOT require or attempt to fetch prior baseline runs.

#### Scenario: Gate passes when openstack meets latency threshold
- **WHEN** the gate evaluates a benchmark report
- **THEN** the gate SHALL pass if every operation's openstack p95 latency does not exceed LocalStack p95 latency by more than the configured ratio threshold (default: 1.5x)

#### Scenario: Gate fails when openstack exceeds latency threshold
- **WHEN** any operation's openstack p95 latency exceeds LocalStack p95 by more than the configured ratio threshold
- **THEN** the gate SHALL fail with a message identifying the failing operation, the openstack value, the LocalStack value, and the threshold

#### Scenario: Gate checks memory budget
- **WHEN** the gate evaluates memory metrics
- **THEN** the gate SHALL fail if the openstack-to-LocalStack RSS ratio exceeds the configured memory budget (default: 0.20)

#### Scenario: Gate checks error rate
- **WHEN** the gate evaluates operation results
- **THEN** the gate SHALL fail if any openstack operation has a non-zero error count

#### Scenario: Gate does not fetch prior baselines
- **WHEN** the gate script is invoked
- **THEN** the gate SHALL NOT attempt to download, fetch, or reference any previous benchmark run artifacts

### Requirement: Benchmark gate SHALL produce raw per-operation markdown summary
The benchmark gate SHALL output a markdown summary showing raw per-operation metrics suitable for CI comments and workflow summaries.

#### Scenario: Markdown summary shows per-operation metrics table
- **WHEN** the gate produces its summary
- **THEN** the summary SHALL include a table with columns for service, operation, openstack p50/p95/p99, LocalStack p50/p95/p99, throughput ratio, and pass/fail status per operation

#### Scenario: Markdown summary shows memory comparison
- **WHEN** memory data is present in the report
- **THEN** the summary SHALL include idle and post-load RSS for both targets

#### Scenario: Markdown summary shows no weighted averages
- **WHEN** the gate produces its summary
- **THEN** the summary SHALL NOT include weighted averages, aggregate ratios, or any cross-service combined metrics

#### Scenario: Markdown summary shows overall gate verdict
- **WHEN** the gate evaluation completes
- **THEN** the summary SHALL include an overall PASS or FAIL verdict with a count of failing operations if any

### Requirement: Benchmark gate SHALL be configurable via flags
The benchmark gate SHALL accept configuration parameters for threshold tuning.

#### Scenario: Latency threshold is configurable
- **WHEN** the gate is invoked with `--p95-threshold 2.0`
- **THEN** the gate SHALL use 2.0x as the p95 latency ratio threshold instead of the default

#### Scenario: Memory budget is configurable
- **WHEN** the gate is invoked with `--memory-budget 0.10`
- **THEN** the gate SHALL use 0.10 as the openstack/LocalStack RSS ratio budget instead of the default

#### Scenario: Output path is configurable
- **WHEN** the gate is invoked with `--output-markdown summary.md`
- **THEN** the gate SHALL write the markdown summary to the specified path

### Requirement: Benchmark gate SHALL exit with appropriate status codes
The gate script SHALL use exit codes to communicate results to CI systems.

#### Scenario: Gate exits 0 on pass
- **WHEN** all operations and memory checks pass
- **THEN** the gate SHALL exit with status code 0

#### Scenario: Gate exits 1 on failure
- **WHEN** any operation or memory check fails
- **THEN** the gate SHALL exit with status code 1

#### Scenario: Gate exits 2 on invalid input
- **WHEN** the input JSON is missing or malformed
- **THEN** the gate SHALL exit with status code 2 and an error message
