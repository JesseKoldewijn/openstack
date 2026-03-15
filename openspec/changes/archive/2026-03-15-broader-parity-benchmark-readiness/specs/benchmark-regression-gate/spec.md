## MODIFIED Requirements

### Requirement: Non-required lanes SHALL remain diagnostic
Non-required lanes SHALL publish benchmark reports and summaries but SHALL NOT block required CI checks in this change. Their outputs SHALL remain explicit even when scenarios are skipped, partially configured, or unavailable.

#### Scenario: High lane reports but does not block PR required checks
- **WHEN** `fair-high` benchmarks run in scheduled or optional CI
- **THEN** result summaries SHALL be published and required PR checks SHALL remain unaffected

#### Scenario: Extreme lane reports skip reasons without required-check failure
- **WHEN** `fair-extreme` benchmarks skip heavy scenarios due to environment constraints
- **THEN** skip reasons SHALL be included in reporting and SHALL NOT fail required PR checks

#### Scenario: Non-required lane with unresolved scenario configuration remains visible
- **WHEN** a non-required lane cannot resolve its configured scenarios or profile wiring
- **THEN** CI summaries and machine-readable outputs SHALL report that configuration state explicitly instead of failing as an unexplained empty benchmark lane

### Requirement: Consolidated benchmark summary SHALL be produced for CI readability
CI reporting SHALL produce one consolidated benchmark summary artifact that includes lane-level metrics and gate verdicts for the run.

#### Scenario: Consolidated summary includes all available fairness lanes
- **WHEN** benchmark lanes complete in a workflow run
- **THEN** CI SHALL emit a single markdown summary containing each available fairness lane (`fair-low`, `fair-medium`, `fair-high`, `fair-extreme`) with key metrics

#### Scenario: Consolidated summary includes gate verdicts
- **WHEN** required lane gate evaluation completes
- **THEN** the consolidated summary SHALL include pass/fail verdicts and threshold context for each required lane

#### Scenario: Consolidated summary includes explicit diagnostic-lane status
- **WHEN** non-required lanes are skipped, partially configured, or intentionally non-blocking
- **THEN** the consolidated summary SHALL include explicit status text describing whether the lane executed, skipped by policy, or failed due to scenario configuration
