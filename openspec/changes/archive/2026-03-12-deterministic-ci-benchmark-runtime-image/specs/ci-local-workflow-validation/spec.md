## MODIFIED Requirements

### Requirement: Benchmark workflow changes SHALL be validated locally with act
Changes to benchmark and benchmark-gate workflows MUST include local validation using `act` before merge. Validation SHALL include the deterministic runtime-image producer/consumer flow and evidence of the immutable runtime reference used for each tested benchmark/parity job.

#### Scenario: Local act validation covers benchmark lane execution
- **WHEN** benchmark workflow logic changes
- **THEN** maintainers SHALL run `act` for relevant benchmark jobs and capture pass/fail evidence

#### Scenario: Local act validation covers benchmark-gate execution
- **WHEN** benchmark-gate logic changes
- **THEN** maintainers SHALL run `act` for gate jobs and capture evidence for both pass and fail paths

#### Scenario: Local act validation captures deterministic runtime image reference
- **WHEN** benchmark/parity jobs are validated with `act`
- **THEN** validation evidence SHALL include the immutable runtime image reference consumed by those jobs and confirmation that the same reference is reused across lanes executed in that simulation

### Requirement: act validation SHALL verify GitHub auth/token prerequisites
Local and CI validation guidance MUST explicitly verify GitHub CLI token requirements used by baseline discovery flows.

#### Scenario: Missing token prerequisite is documented and testable
- **WHEN** benchmark-gate baseline discovery depends on GitHub API access
- **THEN** validation guidance SHALL include explicit `GH_TOKEN` setup requirements and expected failure behavior when missing

#### Scenario: Token-provided path is validated
- **WHEN** `GH_TOKEN` is configured during local simulation
- **THEN** baseline discovery path SHALL execute without auth-configuration errors

### Requirement: Hosted CI validation SHALL verify deterministic runtime image reuse
Hosted CI benchmark/parity validation SHALL include evidence that all relevant jobs in a run used the same immutable OpenStack runtime image reference produced for that run.

#### Scenario: GitHub CI run includes immutable runtime reference evidence
- **WHEN** benchmark/parity workflows run on GitHub-hosted runners
- **THEN** workflow evidence SHALL show the produced immutable runtime image reference and its reuse across benchmark/parity lanes in that run
