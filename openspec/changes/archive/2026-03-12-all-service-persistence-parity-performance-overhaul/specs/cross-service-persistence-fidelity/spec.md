## ADDED Requirements

### Requirement: Persistence parity modes SHALL be explicitly comparable
The system SHALL define persistence parity modes for benchmarking and parity validation, and SHALL only compare OpenStack and LocalStack results under equivalent modes.

#### Scenario: Non-equivalent persistence comparison is invalid
- **WHEN** a benchmark or parity run compares targets with non-equivalent persistence modes
- **THEN** the run SHALL be marked non-interpretable with a deterministic `mode_mismatch` reason

### Requirement: Restart survival semantics SHALL be validated per supported service
The system SHALL validate persistence lifecycle parity for each supported service by verifying state creation, process restart, state recovery, and post-restart behavior consistency.

#### Scenario: Service state survives restart in durable mode
- **WHEN** a service creates durable state and the runtime is restarted in durable mode
- **THEN** the same state SHALL be observable after restart with parity-consistent behavior

#### Scenario: Non-durable mode does not claim restart durability
- **WHEN** the runtime is executed in non-durable mode
- **THEN** parity reports SHALL not claim restart-survival guarantees for that mode

### Requirement: Persistence fidelity reports SHALL include failure classes
The system SHALL report persistence parity outcomes with deterministic failure classes including durability mismatch, mode mismatch, and recovery inconsistency.

#### Scenario: Recovery inconsistency is reported with context
- **WHEN** post-restart behavior differs between targets
- **THEN** the report SHALL include service, operation, expected behavior, observed behavior, and failure class
