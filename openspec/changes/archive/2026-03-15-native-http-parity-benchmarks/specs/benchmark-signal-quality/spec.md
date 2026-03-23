## MODIFIED Requirements

### Requirement: Benchmark signal quality SHALL be validated before performance interpretation
Benchmark runs MUST satisfy minimum data-quality criteria before they are used for optimization decisions or regression assertions, and native HTTP transport support gaps SHALL be treated as explicit validity signals rather than hidden fallback behavior.

#### Scenario: Invalid benchmark lane is flagged as non-interpretable
- **WHEN** a benchmark lane has high error-rate scenarios, insufficient successful performance scenarios, missing required service write and read coverage, or unresolved native transport support gaps
- **THEN** the lane SHALL be marked non-interpretable with explicit failure reasons in report outputs

#### Scenario: Coverage and failed probe data are excluded from performance conclusions
- **WHEN** comparative performance summaries are produced
- **THEN** coverage or probe scenarios and invalid performance scenarios SHALL NOT contribute to optimization conclusion metrics

#### Scenario: Native fallback does not mask invalid signal
- **WHEN** a scenario lacks native HTTP execution support during this migration
- **THEN** the benchmark system SHALL classify that scenario as excluded, invalid, or follow-up-required instead of silently falling back to AWS CLI for required-lane interpretation

### Requirement: Benchmark reports SHALL include quality diagnostics
Benchmark outputs SHALL include quality diagnostics needed to explain result validity, including explicit native HTTP readiness and follow-up accounting across the README service baseline.

#### Scenario: Report includes lane validity diagnostics
- **WHEN** a benchmark run completes
- **THEN** report summary SHALL include quality indicators such as valid scenario count, invalid scenario count, interpretable or non-interpretable status, and missing required role coverage counts

#### Scenario: Report includes invalid-scenario reasons
- **WHEN** scenarios are excluded from valid performance interpretation
- **THEN** exclusion reasons SHALL be recorded in machine-readable output

#### Scenario: Report includes per-service realistic coverage diagnostics
- **WHEN** all-services realistic lanes complete
- **THEN** report outputs SHALL include per-service diagnostics indicating write and read role completeness, exclusions, and invalid reasons

#### Scenario: Report includes native transport follow-up visibility
- **WHEN** a README-listed service still lacks complete native HTTP benchmark support
- **THEN** the report SHALL identify that service with explicit follow-up-required diagnostics instead of omitting it from coverage accounting
