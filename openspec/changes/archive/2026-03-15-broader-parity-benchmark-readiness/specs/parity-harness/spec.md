## MODIFIED Requirements

### Requirement: Protocol-aware normalization and comparison
The system SHALL normalize known nondeterministic values and compare outputs using protocol-aware rules for json, query/xml, rest-xml, and rest-json responses. Broader parity profiles SHALL suppress only differences proven non-semantic for the compared operation and SHALL continue surfacing status, error-family, and materially different body semantics.

#### Scenario: Nondeterministic fields are normalized before comparison
- **WHEN** responses include generated identifiers, timestamps, or request IDs
- **THEN** the harness SHALL apply configured normalization rules before determining parity

#### Scenario: Comparison honors protocol structure
- **WHEN** query/xml or rest-xml responses differ only in non-semantic ordering or formatting
- **THEN** the harness SHALL classify the scenario as parity-pass after canonical protocol comparison

#### Scenario: Broader profile suppresses proven non-semantic S3 wire noise
- **WHEN** a broader parity scenario such as `extended` differs only by equivalent S3 XML media type, empty-element formatting, owner-display metadata, or default object content-type representation that does not change observable behavior
- **THEN** the harness SHALL classify the scenario as parity-pass after applying scoped normalization for that proven non-semantic difference class

#### Scenario: Meaningful semantic mismatches remain visible
- **WHEN** responses differ in status code, error family, success expectation, or materially different response semantics
- **THEN** the harness SHALL continue reporting a parity failure rather than normalizing the difference away

### Requirement: Profile-based parity execution
The system SHALL support named execution profiles so CI can run a stable core parity subset independently from broader parity suites, and the all-services native parity baseline SHALL continue to cover every service listed in `README.md`. Broader profiles SHALL remain useful diagnostic suites rather than accumulating known low-signal mismatches by default.

#### Scenario: Core profile runs required baseline services
- **WHEN** the core parity profile is requested
- **THEN** the harness SHALL execute the configured baseline services and scenarios for PR gating

#### Scenario: Extended profile expands coverage without changing core set
- **WHEN** an extended profile is requested
- **THEN** the harness SHALL run additional configured scenarios while preserving core profile composition

#### Scenario: All-services profile remains the README readiness contract
- **WHEN** the all-services parity profile is requested
- **THEN** the harness SHALL treat its 24-service smoke inventory as the authoritative README readiness baseline rather than replacing it with richer lifecycle scenarios from other profiles

#### Scenario: Broader profile failures remain high-signal
- **WHEN** a broader parity profile completes
- **THEN** any remaining failures SHALL correspond to material behavior differences or explicitly unnormalized diagnostic gaps rather than known low-signal wire-format noise
