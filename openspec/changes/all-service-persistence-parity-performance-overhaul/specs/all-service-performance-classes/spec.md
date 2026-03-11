## ADDED Requirements

### Requirement: Service class taxonomy SHALL be explicit and enforced
The system SHALL classify every supported service into a declared execution class (`in-proc-stateful`, `mixed-orchestration`, or `external-engine-adjacent`) and SHALL publish class membership in benchmark and gate artifacts.

#### Scenario: Every supported service is classified
- **WHEN** the benchmark pipeline generates lane metadata
- **THEN** each supported service SHALL have exactly one declared execution class

#### Scenario: Unknown class membership is rejected
- **WHEN** a supported service appears without a declared class
- **THEN** required-lane interpretation SHALL fail with a deterministic `missing_service_class` diagnostic

### Requirement: Class-specific performance and resource targets SHALL exist
The system SHALL define class-specific acceptance envelopes for latency, throughput, memory, and startup behavior, and SHALL evaluate services against the envelope of their declared class.

#### Scenario: Class target evaluation is lane-aware
- **WHEN** a required lane is evaluated
- **THEN** each service SHALL be checked against thresholds derived from its class and lane mode

#### Scenario: Class budget regressions are diagnosable
- **WHEN** a service breaches a class envelope
- **THEN** the gate output SHALL include service name, class name, metric breached, and threshold details
