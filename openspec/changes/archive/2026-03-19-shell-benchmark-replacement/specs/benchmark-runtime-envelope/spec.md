## MODIFIED Requirements

### Requirement: Benchmark runs SHALL capture memory envelope snapshots
The benchmark system SHALL capture memory snapshots for each target at idle and post-load phases using runtime-appropriate methods (Docker stats for containers, /proc or ps for bare processes).

#### Scenario: Idle memory snapshot is recorded
- **WHEN** benchmark targets are started and healthy before measured load
- **THEN** the script SHALL record idle RSS memory in megabytes for each target

#### Scenario: Post-load memory snapshot is recorded
- **WHEN** all measured benchmark operations complete
- **THEN** the script SHALL record post-load RSS memory in megabytes for each target

#### Scenario: Docker mode uses docker stats for memory collection
- **WHEN** targets run in Docker containers
- **THEN** the script SHALL use `docker stats` to collect container RSS

#### Scenario: Binary mode uses OS-native memory inspection for openstack
- **WHEN** openstack runs as a bare binary process
- **THEN** the script SHALL use `/proc/<pid>/status` VmRSS on Linux or `ps` RSS on macOS for openstack memory measurement

### Requirement: Runtime envelope metrics SHALL be comparable across targets
Runtime envelope data SHALL be recorded in comparable units for openstack and LocalStack.

#### Scenario: Memory values use consistent units
- **WHEN** memory data is recorded for both targets
- **THEN** the report SHALL express both targets' memory in megabytes

#### Scenario: Missing envelope data is explicit
- **WHEN** memory data cannot be collected for a target (e.g., container not running)
- **THEN** the report SHALL include null values for that target's memory fields rather than omitting the fields

## REMOVED Requirements

### Requirement: Benchmark runs SHALL capture startup timing envelope
**Reason**: Standalone startup timing benchmarking is removed from the envelope system. The shell benchmark script measures time-to-healthy as part of its container startup phase but does not run repeated startup timing samples as a dedicated benchmark. The old `bench_startup.sh` functionality is not replicated as a formal envelope metric.
**Migration**: Startup time is observable from the benchmark script's container startup log output. If dedicated startup benchmarking is needed in the future, it can be added as a separate script.
