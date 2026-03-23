## ADDED Requirements

### Requirement: Benchmark signal quality SHALL be validated before performance interpretation
Benchmark runs MUST satisfy minimum data-quality criteria before they are used for optimization decisions or regression assertions. Native HTTP transport support gaps and other data-quality checks SHALL be treated as explicit validity signals rather than hidden fallback behavior, and the harness SHALL distinguish real product/runtime failures from harness configuration defects and unsound scenario contracts.

#### Scenario: Invalid benchmark lane is flagged as non-interpretable
- **WHEN** a benchmark lane has high error-rate scenarios, insufficient successful performance scenarios, or missing required service write/read coverage
- **THEN** the lane SHALL be marked non-interpretable with explicit failure reasons in report outputs

#### Scenario: Coverage and failed probe data are excluded from performance conclusions
- **WHEN** comparative performance summaries are produced
- **THEN** coverage/probe scenarios and invalid performance scenarios SHALL NOT contribute to optimization conclusion metrics

#### Scenario: Native fallback does not mask invalid signal
- **WHEN** a scenario lacks native HTTP execution support during migration
- **THEN** the benchmark system SHALL classify that scenario as excluded, invalid, or follow-up-required instead of silently falling back to AWS CLI for required-lane interpretation

#### Scenario: Scenario-contract defects are separated from product failures
- **WHEN** a benchmark scenario fails because its own setup, seeding, identifier capture, or warmup/measurement contract is unsound
- **THEN** the harness SHALL classify that invalidation separately from product behavior failures so remediation can target the benchmark contract first

### Requirement: Benchmark reports SHALL include quality diagnostics
Benchmark outputs SHALL include quality diagnostics needed to explain result validity, including explicit native HTTP readiness and follow-up accounting across the README service baseline.

#### Scenario: Report includes lane validity diagnostics
- **WHEN** a benchmark run completes
- **THEN** report summary SHALL include quality indicators such as valid scenario count, invalid scenario count, interpretable/non-interpretable status, and missing required role coverage counts

#### Scenario: Report includes invalid-scenario reasons
- **WHEN** scenarios are excluded from valid performance interpretation
- **THEN** exclusion reasons SHALL be recorded in machine-readable output

#### Scenario: Report includes per-service realistic coverage diagnostics
- **WHEN** all-services realistic lanes complete
- **THEN** report outputs SHALL include per-service diagnostics indicating write/read role completeness, exclusions, and invalid reasons

#### Scenario: Report includes native transport follow-up visibility
- **WHEN** a README-listed service still lacks complete native HTTP benchmark support
- **THEN** the report SHALL identify that service with explicit follow-up-required diagnostics instead of omitting it from coverage accounting

#### Scenario: Report preserves likely product-gap evidence after harness cleanup
- **WHEN** a scenario remains invalid or degraded after its benchmark contract has been made sound
- **THEN** the report SHALL preserve that result as likely product/runtime evidence rather than collapsing it back into generic harness diagnostics
