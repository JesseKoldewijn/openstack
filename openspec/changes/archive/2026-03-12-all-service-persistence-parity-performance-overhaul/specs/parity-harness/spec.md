## MODIFIED Requirements

### Requirement: Side-effect parity verification
The system SHALL support parity assertions on observable side effects through follow-up read operations in addition to immediate response comparison. Side-effect parity SHALL include persistence lifecycle checks for supported services in declared durable modes.

#### Scenario: State verification step validates equivalent effects
- **WHEN** a scenario includes a post-operation readback assertion
- **THEN** the harness SHALL verify both targets expose equivalent resource state after normalization

#### Scenario: Restart lifecycle parity is verified
- **WHEN** a persistence lifecycle scenario creates state, restarts both targets in equivalent durable mode, and re-reads state
- **THEN** the harness SHALL classify parity based on post-restart observable equivalence

### Requirement: Structured parity reporting
The system SHALL emit machine-readable parity results per scenario including pass/fail status, diff classification, and target evidence references. Reports SHALL include persistence-mode metadata and deterministic persistence failure classes when relevant.

#### Scenario: Scenario failure emits actionable diff metadata
- **WHEN** a parity mismatch is detected
- **THEN** the result SHALL include service, scenario id, comparison stage, normalized diff details, and raw evidence references for both targets

#### Scenario: Aggregate report includes parity score
- **WHEN** a parity run completes
- **THEN** the harness SHALL output aggregate totals and per-service parity pass rates in a machine-readable report

#### Scenario: Persistence mismatch is classifiable
- **WHEN** a restart or recovery scenario diverges between targets
- **THEN** parity reporting SHALL include deterministic persistence failure class and scenario evidence
