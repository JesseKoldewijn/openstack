## MODIFIED Requirements

### Requirement: Benchmark metrics collection and comparison
The system SHALL capture benchmark metrics for each scenario and target, SHALL compute comparative metrics between openstack and LocalStack, and SHALL emit service-level optimization summaries suitable for remediation tracking.

#### Scenario: Per-scenario metrics are captured
- **WHEN** a benchmark scenario completes
- **THEN** the report SHALL include latency distribution metrics (including p50 and p95), throughput, operation count, and error count for each target

#### Scenario: Comparative deltas are emitted
- **WHEN** benchmark results for both targets are available for a scenario
- **THEN** the report SHALL include openstack-versus-localstack delta and ratio metrics for key latency and throughput measures

#### Scenario: Service-level optimization summary is available
- **WHEN** a benchmark run summary is emitted
- **THEN** the report SHALL include per-service comparison aggregates that can be used to track remediation progress over time

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support benchmark execution profiles that include broad all-services realistic coverage and deeper workloads for selected high-impact services, and SHALL maintain valid write and read performance scenarios for each supported service in required broad coverage lanes.

#### Scenario: All-services realistic profile covers enabled service set
- **WHEN** the all-services realistic benchmark profile is requested
- **THEN** the harness SHALL execute representative realistic benchmark scenarios for every configured benchmark service

#### Scenario: Every service includes write and read realistic scenarios
- **WHEN** required all-services realistic lanes run
- **THEN** each supported service SHALL have at least one measured write/mutate scenario and one measured read/query/list/describe scenario result, or an explicit machine-readable exclusion

#### Scenario: Deep profile targets high-impact service workloads
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute additional workload scenarios for designated high-impact services with larger payloads and/or higher operation volume

#### Scenario: Broad lane scenario validity is enforced
- **WHEN** all-services benchmark lanes run
- **THEN** each supported service SHALL have valid realistic performance scenario coverage for required write/read roles or an explicit machine-readable exclusion reason
