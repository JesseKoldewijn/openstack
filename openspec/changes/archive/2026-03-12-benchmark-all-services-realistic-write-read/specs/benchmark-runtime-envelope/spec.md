## ADDED Requirements

### Requirement: Benchmark runs SHALL capture startup timing envelope
The benchmark system SHALL capture cold-start startup timing metrics for each target runtime and include summary statistics in benchmark output.

#### Scenario: Startup timing samples are collected
- **WHEN** a benchmark run is configured with runtime envelope collection enabled
- **THEN** the harness SHALL collect repeated startup timing samples per target and record aggregate statistics

#### Scenario: Startup timing is represented in report metadata
- **WHEN** benchmark output is emitted
- **THEN** runtime metadata SHALL include startup timing metrics per target

### Requirement: Benchmark runs SHALL capture memory envelope snapshots
The benchmark system SHALL capture memory snapshots for each target at idle and post-load phases and include them in benchmark output.

#### Scenario: Idle memory snapshot is recorded
- **WHEN** benchmark targets are started and stabilized before measured load
- **THEN** the harness SHALL record an idle memory snapshot for each target

#### Scenario: Post-load memory snapshot is recorded
- **WHEN** measured benchmark operations complete
- **THEN** the harness SHALL record a post-load memory snapshot for each target

### Requirement: Runtime envelope metrics SHALL be comparable across targets
Runtime envelope data SHALL be normalized into comparable benchmark report fields for openstack and LocalStack.

#### Scenario: Envelope comparison fields are emitted
- **WHEN** both target envelopes are available
- **THEN** the report SHALL include machine-readable comparative envelope fields for startup timing and memory

#### Scenario: Missing envelope data is explicit
- **WHEN** envelope data cannot be collected for a target
- **THEN** the report SHALL include explicit missing-data diagnostics without silently omitting the target
