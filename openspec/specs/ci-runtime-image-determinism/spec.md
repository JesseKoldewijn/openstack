## ADDED Requirements

### Requirement: CI SHALL build and publish one immutable OpenStack runtime image per workflow run
The CI workflow SHALL produce exactly one OpenStack runtime image artifact for benchmark/parity consumption per workflow run, and that artifact SHALL be referenced through an immutable identifier for all downstream runtime consumers in the same run.

#### Scenario: Single producer job creates runtime image reference
- **WHEN** a CI run starts for benchmark/parity-capable workflows
- **THEN** CI SHALL execute a dedicated producer job that builds the OpenStack runtime image once and exposes a single immutable reference (digest-qualified image reference or equivalent immutable run artifact handle)

#### Scenario: Runtime reference is immutable for downstream jobs
- **WHEN** benchmark/parity jobs start in that workflow run
- **THEN** each job SHALL consume the same immutable runtime reference produced by the producer job and SHALL NOT resolve runtime image from a floating tag such as `latest`

### Requirement: Runtime-image provenance SHALL be observable in job diagnostics
Benchmark/parity jobs SHALL emit runtime-image provenance diagnostics sufficient to identify the exact image used and distinguish image integrity failures from benchmark logic failures.

#### Scenario: Preflight prints selected runtime image reference
- **WHEN** benchmark/parity preflight executes
- **THEN** the job SHALL print the selected OpenStack runtime image reference and image inspect metadata before benchmark/parity execution starts

#### Scenario: Runtime startup failures include provenance context
- **WHEN** runtime health checks fail for the OpenStack benchmark/parity target
- **THEN** failure diagnostics SHALL include runtime container/image provenance context that allows maintainers to attribute the failure to runtime image selection versus workload logic

### Requirement: Deterministic runtime-image flow SHALL be valid in act and GitHub-hosted CI
The runtime-image producer/consumer flow SHALL be executable in local `act` simulation and GitHub-hosted CI, with documented expected behavior for each environment.

#### Scenario: act simulation exercises runtime-image producer/consumer flow
- **WHEN** maintainers run benchmark/parity workflow simulation locally with `act`
- **THEN** the simulation SHALL exercise the same runtime-image producer/consumer contract and produce verifiable evidence of the selected immutable runtime reference

#### Scenario: GitHub-hosted CI exercises runtime-image producer/consumer flow
- **WHEN** benchmark/parity workflows run on GitHub-hosted runners
- **THEN** the run SHALL demonstrate that all benchmark/parity jobs use the same produced immutable runtime reference for that run
