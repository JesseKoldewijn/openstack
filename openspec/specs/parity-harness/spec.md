## ADDED Requirements

### Requirement: Dual-target scenario execution
The system SHALL execute each parity scenario against both openstack and LocalStack targets using equivalent request inputs and scenario setup.

#### Scenario: Identical scenario inputs are sent to both targets
- **WHEN** a parity scenario is selected for execution
- **THEN** the harness SHALL run the same ordered request sequence, setup steps, and teardown steps against openstack and LocalStack

#### Scenario: Target-specific connection details are isolated from scenario logic
- **WHEN** a scenario is executed against both targets
- **THEN** only endpoint/runtime connection configuration SHALL vary by target and scenario semantics SHALL remain unchanged

### Requirement: Protocol-aware normalization and comparison
The system SHALL normalize known nondeterministic values and compare outputs using protocol-aware rules for json, query/xml, rest-xml, and rest-json responses.

#### Scenario: Nondeterministic fields are normalized before comparison
- **WHEN** responses include generated identifiers, timestamps, or request IDs
- **THEN** the harness SHALL apply configured normalization rules before determining parity

#### Scenario: Comparison honors protocol structure
- **WHEN** query/xml or rest-xml responses differ only in non-semantic ordering or formatting
- **THEN** the harness SHALL classify the scenario as parity-pass after canonical protocol comparison

### Requirement: Side-effect parity verification
The system SHALL support parity assertions on observable side effects through follow-up read operations in addition to immediate response comparison.

#### Scenario: State verification step validates equivalent effects
- **WHEN** a scenario includes a post-operation readback assertion
- **THEN** the harness SHALL verify both targets expose equivalent resource state after normalization

### Requirement: Structured parity reporting
The system SHALL emit machine-readable parity results per scenario including pass/fail status, diff classification, and target evidence references.

#### Scenario: Scenario failure emits actionable diff metadata
- **WHEN** a parity mismatch is detected
- **THEN** the result SHALL include service, scenario id, comparison stage, normalized diff details, and raw evidence references for both targets

#### Scenario: Aggregate report includes parity score
- **WHEN** a parity run completes
- **THEN** the harness SHALL output aggregate totals and per-service parity pass rates in a machine-readable report

### Requirement: Known-difference governance
The system SHALL support an explicit known-differences registry that allows scoped accepted divergences with rationale and review metadata.

#### Scenario: Accepted difference suppresses hard failure with traceability
- **WHEN** a mismatch matches an active known-difference rule
- **THEN** the harness SHALL classify it as accepted-difference and include the rule identifier and rationale in the report

#### Scenario: Expired accepted difference fails parity check
- **WHEN** a known-difference entry is expired or invalid
- **THEN** the matching mismatch SHALL be treated as a parity failure

### Requirement: Profile-based parity execution
The system SHALL support named execution profiles so CI can run a stable core parity subset independently from broader parity suites.

#### Scenario: Core profile runs required baseline services
- **WHEN** the core parity profile is requested
- **THEN** the harness SHALL execute the configured baseline services and scenarios for PR gating

#### Scenario: Extended profile expands coverage without changing core set
- **WHEN** an extended profile is requested
- **THEN** the harness SHALL run additional configured scenarios while preserving core profile composition
