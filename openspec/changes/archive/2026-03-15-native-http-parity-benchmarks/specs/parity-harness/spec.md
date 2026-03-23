## ADDED Requirements

### Requirement: README service baseline coverage tracking
The parity harness SHALL treat the services listed in `README.md` as the authoritative all-services coverage baseline for native HTTP parity migration, and SHALL report native parity maturity for each listed service.

#### Scenario: All README-listed services appear in coverage accounting
- **WHEN** an all-services parity profile completes
- **THEN** the report SHALL include one entry for each service listed in `README.md`, even if the service currently has unsupported native operations or follow-up-required mismatches

#### Scenario: Follow-up-required service gaps remain visible
- **WHEN** a README-listed service cannot yet be executed natively or does not yet respond equivalently to LocalStack for its baseline parity scenario
- **THEN** the harness SHALL record a machine-readable follow-up-required outcome rather than silently omitting the service or falling back to AWS CLI

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

#### Scenario: Native coverage gaps are reported explicitly
- **WHEN** a scenario cannot be executed natively or yields a known follow-up-required outcome during the migration baseline
- **THEN** the report SHALL include explicit machine-readable status for that gap without reclassifying it as a successful parity pass

### Requirement: Dual-target scenario execution
The system SHALL execute each parity scenario against both openstack and LocalStack targets using equivalent native HTTP request inputs and scenario setup, with only target endpoint/runtime configuration varying by target.

#### Scenario: Identical scenario inputs are sent to both targets
- **WHEN** a parity scenario is selected for execution
- **THEN** the harness SHALL run the same ordered request sequence, setup steps, and teardown steps against openstack and LocalStack

#### Scenario: Target-specific connection details are isolated from scenario logic
- **WHEN** a scenario is executed against both targets
- **THEN** only endpoint/runtime connection configuration SHALL vary by target and scenario semantics SHALL remain unchanged

#### Scenario: AWS CLI is not required for parity execution
- **WHEN** a parity profile is executed with supported native translators available
- **THEN** the harness SHALL perform scenario execution without spawning AWS CLI processes

### Requirement: Protocol-aware normalization and comparison
The system SHALL normalize known nondeterministic values and compare outputs using protocol-aware native HTTP rules for json, query/xml, rest-xml, and rest-json responses, including status codes, selected protocol-relevant headers, response bodies, and error structures.

#### Scenario: Nondeterministic fields are normalized before comparison
- **WHEN** responses include generated identifiers, timestamps, or request IDs
- **THEN** the harness SHALL apply configured normalization rules before determining parity

#### Scenario: Comparison honors protocol structure
- **WHEN** query/xml or rest-xml responses differ only in non-semantic ordering or formatting
- **THEN** the harness SHALL classify the scenario as parity-pass after canonical protocol comparison

#### Scenario: HTTP status and error shape differences are detectable
- **WHEN** openstack and LocalStack respond with different status codes or semantically different error payloads for the same request
- **THEN** the harness SHALL classify the scenario as a parity mismatch even if both targets returned non-success responses

### Requirement: Profile-based parity execution
The system SHALL support named execution profiles so CI can run a stable core parity subset independently from broader parity suites, and the all-services native parity baseline SHALL continue to cover every service listed in `README.md`.

#### Scenario: Core profile runs required baseline services
- **WHEN** the core parity profile is requested
- **THEN** the harness SHALL execute the configured baseline services and scenarios for PR gating

#### Scenario: Extended profile expands coverage without changing core set
- **WHEN** an extended profile is requested
- **THEN** the harness SHALL run additional configured scenarios while preserving core profile composition

#### Scenario: All-services profile retains README service inventory
- **WHEN** the all-services parity profile is requested
- **THEN** the harness SHALL attempt native parity coverage for each service listed in `README.md` and SHALL surface any non-equivalent or unsupported results as explicit outcomes
