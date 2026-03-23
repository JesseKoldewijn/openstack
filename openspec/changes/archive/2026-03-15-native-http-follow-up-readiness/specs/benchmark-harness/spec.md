## MODIFIED Requirements

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

#### Scenario: Fair-core lanes remain non-interpretable until role completeness is resolved
- **WHEN** a required fair-core native benchmark lane completes with missing write/read service roles
- **THEN** the lane SHALL remain non-interpretable even if all executed native requests succeeded

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

#### Scenario: Missing runtime evidence remains explicit
- **WHEN** a benchmark lane cannot collect runtime evidence such as memory RSS for one target
- **THEN** the report SHALL record the missing target explicitly rather than implying full observability coverage
