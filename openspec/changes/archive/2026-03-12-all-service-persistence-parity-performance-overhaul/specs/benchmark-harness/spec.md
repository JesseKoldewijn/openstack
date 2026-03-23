## MODIFIED Requirements

### Requirement: Dual-target benchmark execution
The system SHALL execute each benchmark scenario against both openstack and LocalStack targets using equivalent request inputs and benchmark configuration. Benchmark execution SHALL include explicit persistence-mode metadata and SHALL reject non-equivalent mode comparisons for interpretable performance claims.

#### Scenario: Equivalent scenario workload is executed on both targets
- **WHEN** a benchmark scenario is selected for execution
- **THEN** the harness SHALL run the same setup, workload, and cleanup steps against openstack and LocalStack with only endpoint/runtime connection settings varying by target

#### Scenario: Benchmark run records target metadata
- **WHEN** a benchmark run starts
- **THEN** the harness SHALL record target metadata including endpoint and LocalStack image/version (when available) in the benchmark report

#### Scenario: Non-equivalent persistence modes are marked invalid
- **WHEN** openstack and LocalStack are configured with non-equivalent persistence modes for a comparative lane
- **THEN** the lane SHALL be marked non-interpretable with `mode_mismatch` diagnostics

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
