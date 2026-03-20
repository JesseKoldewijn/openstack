## Purpose
Shell-based benchmark gate that evaluates absolute openstack performance ceilings for CI pass/fail decisions. LocalStack and moto data is included as comparison context only and is never used for gating.

## Requirements

### Requirement: Benchmark gate SHALL evaluate absolute openstack-only ceilings
The benchmark gate SHALL determine pass/fail by applying absolute ceilings to openstack metrics only. The gate SHALL NOT compare openstack against LocalStack or moto for gating decisions, and SHALL NOT require or attempt to fetch prior baseline runs. LocalStack and moto data is included in the output as comparison context to demonstrate relative performance.

#### Scenario: Gate passes when openstack p95 latency is within the absolute ceiling
- **WHEN** the gate evaluates a benchmark report
- **THEN** the gate SHALL pass if every operation's openstack p95 latency does not exceed the configured absolute ceiling (default: 5ms)

#### Scenario: Gate fails when openstack p95 latency exceeds the absolute ceiling
- **WHEN** any operation's openstack p95 latency exceeds the configured ceiling
- **THEN** the gate SHALL fail with a message identifying the failing operation, the openstack p95 value, and the ceiling

#### Scenario: Gate checks absolute memory ceiling
- **WHEN** the gate evaluates memory metrics
- **THEN** the gate SHALL fail if openstack's loaded RSS exceeds the configured absolute ceiling (default: 10MB)

#### Scenario: Gate checks error rate
- **WHEN** the gate evaluates operation results
- **THEN** the gate SHALL fail if any openstack operation has a non-zero error count

#### Scenario: Gate does not fetch prior baselines
- **WHEN** the gate script is invoked
- **THEN** the gate SHALL NOT attempt to download, fetch, or reference any previous benchmark run artifacts

### Requirement: Benchmark gate SHALL produce a structured JSON evaluation report
The benchmark gate SHALL output a structured JSON report containing gate verdict, per-operation evaluation results, speedup ratios vs all active comparison targets, and memory metrics for all active targets.

#### Scenario: JSON report includes gate verdict and threshold config
- **WHEN** the gate produces its report
- **THEN** the report SHALL include a top-level `verdict` field (`PASS` or `FAIL`), a `thresholds` object with `p95_max_ms` and `memory_max_mb`, and a `failures` object with arrays for `latency`, `errors`, `memory`, and `total`

#### Scenario: JSON report includes per-operation speedup ratios vs active targets
- **WHEN** LocalStack or moto are present in the benchmark report
- **THEN** the gate JSON SHALL include per-operation `speedup_vs_localstack` and/or `speedup_vs_moto` objects with `p50`, `p95`, `p99`, and `rps` ratio fields

#### Scenario: JSON report includes per-service and overall speedup aggregates
- **WHEN** the gate report is produced
- **THEN** the `services` object SHALL include per-service `speedup_vs_localstack` and `speedup_vs_moto` aggregates (min/max/avg for each metric), and an `overall` object SHALL aggregate across all services

#### Scenario: JSON report includes memory for all active targets
- **WHEN** the gate report is produced
- **THEN** the `memory` section SHALL include `openstack`, `localstack` (if active), and `moto` (if active) objects each with `idle_mb` and `loaded_mb` fields, plus a `gate_pass` boolean

### Requirement: Benchmark gate SHALL be configurable via flags
The benchmark gate SHALL accept configuration parameters for threshold tuning and output path.

#### Scenario: Latency ceiling is configurable
- **WHEN** the gate is invoked with `--p95-max 10`
- **THEN** the gate SHALL use 10ms as the p95 latency ceiling instead of the default

#### Scenario: Memory ceiling is configurable
- **WHEN** the gate is invoked with `--memory-max 20`
- **THEN** the gate SHALL use 20MB as the openstack loaded RSS ceiling instead of the default

#### Scenario: Output path is configurable
- **WHEN** the gate is invoked with `--output benchmark-gate.json`
- **THEN** the gate SHALL write the JSON evaluation report to the specified path instead of stdout

#### Scenario: Errors can be selectively ignored
- **WHEN** the gate is invoked with `--ignore-errors iam/create_user,s3/put_object`
- **THEN** the gate SHALL skip the error check for the specified service/operation pairs and record them as `error_ignored: true` in the report

### Requirement: Benchmark gate SHALL exit with appropriate status codes
The gate script SHALL use exit codes to communicate results to CI systems.

#### Scenario: Gate exits 0 on pass
- **WHEN** all operations and memory checks pass
- **THEN** the gate SHALL exit with status code 0

#### Scenario: Gate exits 1 on failure
- **WHEN** any operation or memory check fails
- **THEN** the gate SHALL exit with status code 1

#### Scenario: Gate exits 2 on invalid input
- **WHEN** the input JSON is missing, malformed, or `--report` is not provided
- **THEN** the gate SHALL exit with status code 2 and an error message
