## MODIFIED Requirements

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in a machine-readable format suitable for automation and trend analysis, and SHALL publish readable consolidated CI summaries across benchmark lanes.

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
