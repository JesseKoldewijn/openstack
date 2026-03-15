## Purpose
TBD

## Requirements

### Requirement: Service workload matrix SHALL define realistic write and read coverage for every supported service
The benchmark system SHALL maintain a machine-readable service workload matrix that defines required measured scenario roles for every supported service, including at least one write/mutate role and one read/query/list/describe role.

#### Scenario: Matrix includes all supported services
- **WHEN** benchmark scenarios are loaded for all-services realistic lanes
- **THEN** every service in the supported benchmark service list SHALL have a corresponding matrix entry

#### Scenario: Matrix enforces write and read role requirements
- **WHEN** a service matrix entry is evaluated for required lanes
- **THEN** the service SHALL declare at least one required write role and at least one required read role

### Requirement: Scenario role metadata SHALL be explicit and validated
Each benchmark scenario SHALL declare a machine-readable role classification used for service workload completeness checks. Required lanes SHALL NOT rely on scenario-id heuristics as the primary source of role meaning.

#### Scenario: Role metadata is present for realistic scenarios
- **WHEN** a realistic benchmark scenario is parsed
- **THEN** the scenario SHALL include a role classification compatible with the service workload matrix

#### Scenario: Unknown role fails validation
- **WHEN** a scenario declares a role not recognized by the workload matrix rules
- **THEN** benchmark validation SHALL mark the scenario invalid with a machine-readable reason

#### Scenario: Required core scenarios declare explicit roles
- **WHEN** required fair-core scenarios such as `sts-core-call` are loaded
- **THEN** the scenario SHALL carry an explicit role assignment so role completeness does not depend on inferred naming heuristics

#### Scenario: Auxiliary role does not satisfy required read/write coverage by default
- **WHEN** a scenario is classified as `aux`
- **THEN** the workload matrix SHALL NOT count that scenario toward required read or write role completeness unless an explicit lane rule maps it to one of those roles

### Requirement: Required lane completeness SHALL be evaluated per service
The benchmark harness SHALL evaluate per-service realistic completeness for required lanes and SHALL fail lane validity when any service lacks required write/read valid scenarios. Diagnostic lanes SHALL be allowed to declare partial-role intent without being judged by required-lane completeness rules.

#### Scenario: Missing write coverage invalidates required lane
- **WHEN** a required lane has a service without at least one valid write-role result
- **THEN** the lane SHALL be marked non-interpretable with a reason identifying missing write coverage for that service

#### Scenario: Missing read coverage invalidates required lane
- **WHEN** a required lane has a service without at least one valid read-role result
- **THEN** the lane SHALL be marked non-interpretable with a reason identifying missing read coverage for that service

#### Scenario: Lane policy determines whether completeness is required
- **WHEN** a benchmark lane is marked diagnostic or partial-coverage by policy
- **THEN** the workload matrix SHALL report covered roles and uncovered roles without treating the lane as missing required read/write completeness by default

### Requirement: Exclusions SHALL be explicit, machine-readable, and auditable
If realistic write/read coverage cannot be satisfied for a service, exclusions SHALL be captured explicitly with reason codes and SHALL be surfaced in benchmark outputs.

#### Scenario: Excluded service role includes reason code
- **WHEN** a service role requirement is excluded in a benchmark lane
- **THEN** the exclusion SHALL include a machine-readable reason code and human-readable rationale

#### Scenario: Exclusions are reported per service and role
- **WHEN** benchmark reports are generated
- **THEN** outputs SHALL include exclusion diagnostics keyed by service and required role

#### Scenario: Runtime-policy skips remain auditable
- **WHEN** scenarios such as heavy-object validations are skipped because runtime policy disables them
- **THEN** the workload matrix/reporting path SHALL preserve that skip as an auditable exclusion or diagnostic state rather than silently dropping role information
