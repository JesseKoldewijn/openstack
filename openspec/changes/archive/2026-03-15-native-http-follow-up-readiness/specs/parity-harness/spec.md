## MODIFIED Requirements

### Requirement: Structured parity reporting
The system SHALL emit machine-readable parity results per scenario including pass/fail status, diff classification, target evidence references, and native execution coverage status. Reports SHALL include persistence-mode metadata, deterministic persistence failure classes when relevant, and service-level follow-up indicators for README-listed services whose native HTTP parity is not yet complete.

#### Scenario: Scenario failure emits actionable diff metadata
- **WHEN** a parity mismatch is detected
- **THEN** the result SHALL include service, scenario id, comparison stage, mismatch kind, normalized diff details, and raw evidence references for both targets

#### Scenario: Aggregate report includes parity score
- **WHEN** a parity run completes
- **THEN** the harness SHALL output aggregate totals and per-service parity pass rates in a machine-readable report

#### Scenario: Persistence mismatch is classifiable
- **WHEN** a restart or recovery scenario diverges between targets
- **THEN** parity reporting SHALL include deterministic persistence failure class and scenario evidence

#### Scenario: README baseline mismatch remains follow-up-visible
- **WHEN** a README-listed baseline parity scenario executes natively but still does not match LocalStack semantics
- **THEN** the report SHALL mark the scenario as `follow_up_required` with `native_coverage_status=follow-up-required` instead of classifying it as silently complete native coverage

#### Scenario: Accepted difference exits follow-up-required state
- **WHEN** a README-listed baseline mismatch is covered by an active accepted-difference rule
- **THEN** parity reporting SHALL preserve the accepted-difference traceability while allowing the service to avoid follow-up-required classification for that governed mismatch

### Requirement: Profile-based parity execution
The system SHALL support named execution profiles so CI can run a stable core parity subset independently from broader parity suites, and the all-services native parity baseline SHALL continue to cover every service listed in `README.md`.

#### Scenario: Core profile runs required baseline services
- **WHEN** the core parity profile is requested
- **THEN** the harness SHALL execute the configured baseline services and scenarios for PR gating

#### Scenario: Extended profile expands coverage without changing core set
- **WHEN** an extended profile is requested
- **THEN** the harness SHALL run additional configured scenarios while preserving core profile composition

#### Scenario: All-services profile remains the README readiness contract
- **WHEN** the all-services parity profile is requested
- **THEN** the harness SHALL treat its 24-service smoke inventory as the authoritative README readiness baseline rather than replacing it with richer lifecycle scenarios from other profiles

#### Scenario: Deeper scenarios do not erase baseline readiness gaps
- **WHEN** a service passes a richer core or lifecycle parity scenario but still fails its all-services smoke baseline scenario
- **THEN** the README baseline report SHALL continue to show that service as follow-up-required until the smoke baseline behavior is resolved or explicitly governed
