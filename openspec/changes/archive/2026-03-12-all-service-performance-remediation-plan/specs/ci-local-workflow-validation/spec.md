## ADDED Requirements

### Requirement: Benchmark workflow changes SHALL be validated locally with act
Changes to benchmark and benchmark-gate workflows MUST include local validation using `act` before merge.

#### Scenario: Local act validation covers benchmark lane execution
- **WHEN** benchmark workflow logic changes
- **THEN** maintainers SHALL run `act` for relevant benchmark jobs and capture pass/fail evidence

#### Scenario: Local act validation covers benchmark-gate execution
- **WHEN** benchmark-gate logic changes
- **THEN** maintainers SHALL run `act` for gate jobs and capture evidence for both pass and fail paths

### Requirement: act validation SHALL verify GitHub auth/token prerequisites
Local and CI validation guidance MUST explicitly verify GitHub CLI token requirements used by baseline discovery flows.

#### Scenario: Missing token prerequisite is documented and testable
- **WHEN** benchmark-gate baseline discovery depends on GitHub API access
- **THEN** validation guidance SHALL include explicit `GH_TOKEN` setup requirements and expected failure behavior when missing

#### Scenario: Token-provided path is validated
- **WHEN** `GH_TOKEN` is configured during local simulation
- **THEN** baseline discovery path SHALL execute without auth-configuration errors
