## MODIFIED Requirements

### Requirement: Required CI lanes SHALL enforce benchmark regression thresholds
The CI system SHALL fail required benchmark lanes when measured performance regresses beyond configured week 3+ thresholds versus the previous successful baseline for the same lane. Required-lane evaluation SHALL include class-aware checks so service results are validated against class-specific envelopes.

#### Scenario: Required lane fails on p95 latency regression breach
- **WHEN** the current lane p95 ratio regresses by more than the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

#### Scenario: Required lane fails on p99 latency regression breach
- **WHEN** the current lane p99 ratio regresses by more than the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

#### Scenario: Required lane fails on throughput regression breach
- **WHEN** the current lane throughput ratio regresses below the configured threshold relative to baseline
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, baseline value, threshold, and lane name

#### Scenario: Required lane fails class-envelope breach
- **WHEN** one or more required services breach their class-specific performance or resource envelope
- **THEN** the benchmark gate SHALL fail with deterministic per-service class diagnostics

### Requirement: Required lane result quality SHALL be validated before threshold checks
The benchmark gate SHALL validate that required lane results contain usable performance data before evaluating regression thresholds. Validation SHALL include lane interpretability, equivalent mode comparison, and persistence-aware validity checks.

#### Scenario: Missing performance scenarios fails required lane
- **WHEN** a required lane report contains zero performance scenarios
- **THEN** the benchmark gate SHALL fail with a data-quality error message

#### Scenario: All performance scenarios skipped fails required lane
- **WHEN** a required lane report marks all performance scenarios as skipped
- **THEN** the benchmark gate SHALL fail with skip-reason context

#### Scenario: Mode mismatch fails required lane interpretation
- **WHEN** required lane inputs are collected under non-equivalent persistence modes
- **THEN** the benchmark gate SHALL fail with a deterministic `mode_mismatch` failure category
