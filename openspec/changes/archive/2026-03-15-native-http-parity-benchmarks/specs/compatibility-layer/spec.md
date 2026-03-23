## MODIFIED Requirements

### Requirement: Differential compatibility verification in CI
The system SHALL run a parity harness in CI that compares openstack and LocalStack behavior for a defined core compatibility profile and surfaces regressions as CI-visible failures, using native HTTP execution for supported parity scenarios rather than AWS CLI process execution.

#### Scenario: Core parity profile is executed on pull requests
- **WHEN** a pull request modifies compatibility-relevant behavior
- **THEN** CI SHALL run the core parity profile and publish pass or fail parity results

#### Scenario: Parity regression blocks required checks
- **WHEN** a non-accepted parity mismatch is detected in the required profile
- **THEN** the CI parity check SHALL fail and block merge until resolved or explicitly accepted

#### Scenario: Native transport parity remains LocalStack-referenced
- **WHEN** the parity harness runs through native HTTP transport
- **THEN** the same request semantics SHALL be executed against openstack and LocalStack and compatibility verdicts SHALL continue to be based on response equivalence between the two targets

#### Scenario: README baseline follow-up gaps remain visible in CI
- **WHEN** a README-listed service is not yet parity-equivalent or not yet fully supported by the native transport path
- **THEN** CI-visible parity reporting SHALL expose that service gap explicitly as a follow-up-required result instead of silently dropping it from coverage
