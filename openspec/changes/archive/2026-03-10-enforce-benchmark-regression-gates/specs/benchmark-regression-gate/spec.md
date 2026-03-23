## ADDED Requirements

### Requirement: Required CI lanes SHALL enforce benchmark regression thresholds
The CI system SHALL fail required benchmark lanes when measured performance regresses beyond configured week 3+ thresholds versus the previous successful baseline for the same lane.

#### Scenario: Required lane fails on p95 latency regression breach
- **WHEN** the current lane p95 ratio regresses by more than the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

#### Scenario: Required lane fails on p99 latency regression breach
- **WHEN** the current lane p99 ratio regresses by more than the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

#### Scenario: Required lane fails on throughput regression breach
- **WHEN** the current lane throughput ratio regresses below the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

### Requirement: Baseline availability SHALL be mandatory for required lanes
Required benchmark lanes MUST have a resolvable previous successful baseline; otherwise the benchmark gate SHALL fail.

#### Scenario: Missing baseline fails required lane
- **WHEN** a required lane cannot resolve a previous successful baseline report
- **THEN** the benchmark gate SHALL fail with remediation guidance describing how to seed or recover a baseline

### Requirement: Required lane result quality SHALL be validated before threshold checks
The benchmark gate SHALL validate that required lane results contain usable performance data before evaluating regression thresholds.

#### Scenario: Missing performance scenarios fails required lane
- **WHEN** a required lane report contains zero performance scenarios
- **THEN** the benchmark gate SHALL fail with a data-quality error message

#### Scenario: All performance scenarios skipped fails required lane
- **WHEN** a required lane report marks all performance scenarios as skipped
- **THEN** the benchmark gate SHALL fail with skip-reason context

### Requirement: Non-required lanes SHALL remain diagnostic
Non-required lanes SHALL publish benchmark reports and summaries but SHALL NOT block required CI checks in this change.

#### Scenario: High lane reports but does not block PR required checks
- **WHEN** `fair-high` benchmarks run in scheduled or optional CI
- **THEN** result summaries SHALL be published and required PR checks SHALL remain unaffected

#### Scenario: Extreme lane reports skip reasons without required-check failure
- **WHEN** `fair-extreme` benchmarks skip heavy scenarios due to environment constraints
- **THEN** skip reasons SHALL be included in reporting and SHALL NOT fail required PR checks

### Requirement: Consolidated benchmark summary SHALL be produced for CI readability
CI reporting SHALL produce one consolidated benchmark summary artifact that includes lane-level metrics and gate verdicts for the run.

#### Scenario: Consolidated summary includes all available fairness lanes
- **WHEN** benchmark lanes complete in a workflow run
- **THEN** CI SHALL emit a single markdown summary containing each available fairness lane (`fair-low`, `fair-medium`, `fair-high`, `fair-extreme`) with key metrics

#### Scenario: Consolidated summary includes gate verdicts
- **WHEN** required lane gate evaluation completes
- **THEN** the consolidated summary SHALL include pass/fail verdicts and threshold context for each required lane
