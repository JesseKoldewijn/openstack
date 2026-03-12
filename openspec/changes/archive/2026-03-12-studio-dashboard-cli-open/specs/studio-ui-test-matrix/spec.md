## ADDED Requirements

### Requirement: Studio test matrix enforcement
Studio changes SHALL satisfy a layered automated test matrix covering unit, render/component integration, service contract integration, and end-to-end user journeys.

#### Scenario: Unit and render tests gate Studio state and rendering logic
- **WHEN** Studio UI domain or rendering logic changes
- **THEN** CI SHALL execute and require passing unit and render/component integration tests for affected Studio modules

#### Scenario: Contract integration tests gate API and gateway compatibility
- **WHEN** Studio API or gateway Studio route behavior changes
- **THEN** CI SHALL execute and require passing integration tests that validate endpoint contracts and Studio routing semantics

#### Scenario: End-to-end Studio journeys gate user-facing behavior
- **WHEN** pull requests modify Studio capabilities
- **THEN** CI SHALL execute and require passing Studio E2E journeys covering dashboard load, guided flow execution, raw operation execution, and history replay

### Requirement: Representative protocol and service operation coverage
Studio automated suites SHALL include representative operation coverage for query, json_target, rest_xml, and rest_json classes as part of guided and/or raw pathways.

#### Scenario: Protocol-class operation coverage is validated
- **WHEN** Studio test suites run
- **THEN** they SHALL verify at least one representative operation journey per protocol class

#### Scenario: Coverage gaps are surfaced in CI output
- **WHEN** Studio coverage validation completes
- **THEN** CI SHALL report covered and uncovered Studio operation classes and services

### Requirement: Deterministic fixtures and cleanup
Studio tests SHALL use deterministic fixtures, explicit startup readiness gates, and resource cleanup to avoid flaky or state-leaking behavior.

#### Scenario: Runtime readiness gate precedes Studio test execution
- **WHEN** integration or E2E Studio tests start
- **THEN** tests SHALL wait for explicit runtime readiness before issuing operations

#### Scenario: Test-created resources are cleaned up
- **WHEN** a Studio test completes
- **THEN** created resources and test artifacts SHALL be cleaned up or namespaced to prevent cross-test contamination

### Requirement: Security regression coverage for Studio operations
Studio operation endpoints and flows SHALL include automated security regressions for method restrictions, payload bounds, and replay safety expectations.

#### Scenario: Guided execution method restrictions are tested
- **WHEN** a Studio guided execution endpoint receives a disallowed method in tests
- **THEN** the system SHALL reject the request with method-not-allowed semantics

#### Scenario: Guided execution payload bounds are tested
- **WHEN** a Studio guided execution endpoint receives an oversized payload in tests
- **THEN** the system SHALL reject the request with payload-too-large semantics
