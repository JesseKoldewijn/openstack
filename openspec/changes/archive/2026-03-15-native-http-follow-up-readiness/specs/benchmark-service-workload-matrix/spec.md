## MODIFIED Requirements

### Requirement: Scenario role metadata SHALL be explicit and validated
Each benchmark scenario SHALL declare a machine-readable role classification used for service workload completeness checks.

#### Scenario: Role metadata is present for realistic scenarios
- **WHEN** a realistic benchmark scenario is parsed
- **THEN** the scenario SHALL include a role classification compatible with the service workload matrix

#### Scenario: Unknown role fails validation
- **WHEN** a scenario declares a role not recognized by the workload matrix rules
- **THEN** benchmark validation SHALL mark the scenario invalid with a machine-readable reason

#### Scenario: Auxiliary role does not satisfy required read/write coverage
- **WHEN** a scenario is classified as `aux`
- **THEN** the workload matrix SHALL NOT count that scenario toward required read or write role completeness unless an explicit matrix rule maps it to one of those roles

### Requirement: Required lane completeness SHALL be evaluated per service
The benchmark harness SHALL evaluate per-service realistic completeness for required lanes and SHALL fail lane validity when any service lacks required write/read valid scenarios.

#### Scenario: Missing write coverage invalidates required lane
- **WHEN** a required lane has a service without at least one valid write-role result
- **THEN** the lane SHALL be marked non-interpretable with a reason identifying missing write coverage for that service

#### Scenario: Missing read coverage invalidates required lane
- **WHEN** a required lane has a service without at least one valid read-role result
- **THEN** the lane SHALL be marked non-interpretable with a reason identifying missing read coverage for that service

#### Scenario: Explicit exclusion can satisfy auditability without pretending completeness
- **WHEN** a service role requirement cannot yet be satisfied for a required lane
- **THEN** the workload matrix SHALL require an explicit exclusion entry with reason code and rationale rather than allowing the role to disappear from completeness evaluation

### Requirement: Exclusions SHALL be explicit, machine-readable, and auditable
If realistic write/read coverage cannot be satisfied for a service, exclusions SHALL be captured explicitly with reason codes and SHALL be surfaced in benchmark outputs.

#### Scenario: Excluded service role includes reason code
- **WHEN** a service role requirement is excluded in a benchmark lane
- **THEN** the exclusion SHALL include a machine-readable reason code and human-readable rationale

#### Scenario: Exclusions are reported per service and role
- **WHEN** benchmark reports are generated
- **THEN** outputs SHALL include exclusion diagnostics keyed by service and required role

#### Scenario: STS role treatment is explicit
- **WHEN** `sts` remains outside required read/write workload coverage for a required lane
- **THEN** the workload matrix SHALL report that state either as missing read/write coverage or as an explicit exclusion, but SHALL NOT treat `aux` coverage as sufficient by default
