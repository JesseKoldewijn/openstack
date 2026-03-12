## Purpose

Define required Studio test coverage and execution guarantees across unit, integration, and end-to-end layers.

## ADDED Requirements

### Requirement: Studio frontend test pyramid
The project SHALL include a Studio frontend test pyramid that covers unit, component/integration, and browser E2E layers, and CI SHALL enforce all layers for Studio-related changes.

#### Scenario: Frontend unit tests enforce core state logic
- **WHEN** Studio state management or request formatting code changes
- **THEN** frontend unit tests SHALL validate deterministic behavior for state transitions, serialization, and parsing helpers

#### Scenario: Component tests enforce UI behavior contracts
- **WHEN** Studio UI components for navigation, request editing, and response rendering are changed
- **THEN** component/integration tests SHALL validate expected interaction behavior and rendering states

#### Scenario: Browser E2E tests are required in CI
- **WHEN** a pull request includes Studio-affecting changes
- **THEN** CI SHALL execute browser E2E suites against a real openstack runtime and block merge on failures

#### Scenario: Dashboard journey E2E coverage exists
- **WHEN** Studio E2E suite runs
- **THEN** it SHALL include dashboard load, service selection, guided execution, raw execution, and history replay journeys

### Requirement: End-to-end API interaction fidelity
Studio E2E tests SHALL execute real interactions against openstack endpoints and SHALL verify that user-triggered actions produce expected backend-visible side effects.

#### Scenario: E2E validates guided flow side effects
- **WHEN** an E2E test runs a guided workflow (for example S3 create/upload)
- **THEN** the test SHALL verify resulting state through API assertions or follow-up reads rather than UI-only checks

#### Scenario: E2E validates raw console path
- **WHEN** an E2E test sends a raw interaction request through Studio
- **THEN** the test SHALL verify the response envelope and resulting backend effect for that interaction

### Requirement: Deterministic Studio test fixtures
Studio automated tests SHALL use deterministic fixtures for runtime startup, seeded entities, and cleanup so repeated test runs are stable.

#### Scenario: Isolated test run cleanup
- **WHEN** a Studio E2E test run completes
- **THEN** all created resources and runtime artifacts for that test SHALL be cleaned up or namespaced to avoid cross-test contamination

#### Scenario: Startup readiness gating prevents flakiness
- **WHEN** Studio E2E tests begin
- **THEN** tests SHALL wait for deterministic openstack readiness checks before executing interactions

### Requirement: Coverage baseline across service classes
Studio E2E suite SHALL include representative interaction coverage across major service protocol classes and SHALL report coverage status in CI output.

#### Scenario: Protocol-class coverage exists
- **WHEN** CI runs Studio E2E suites
- **THEN** the suite SHALL include at least one validated flow for query-style, JSON-style, and REST-style service interactions

#### Scenario: Coverage report highlights gaps
- **WHEN** Studio test suites complete
- **THEN** CI output SHALL include a coverage summary identifying covered services and unimplemented interaction tiers
