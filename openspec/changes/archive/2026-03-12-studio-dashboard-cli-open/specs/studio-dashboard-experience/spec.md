## ADDED Requirements

### Requirement: Studio dashboard home experience
Studio SHALL provide a dashboard home view that presents all supported services, current runtime availability, support tier, and guided-flow coverage metadata in a single operational surface.

#### Scenario: Dashboard loads service inventory and readiness state
- **WHEN** a user opens `/_localstack/studio`
- **THEN** the dashboard SHALL render service cards for enabled services with status and support tier derived from Studio API metadata

#### Scenario: Dashboard highlights guided coverage quality
- **WHEN** Studio API coverage metadata is available
- **THEN** the dashboard SHALL display per-service guided-flow coverage indicators and overall readiness summary

### Requirement: Studio service detail and operation surfaces
Studio SHALL provide a service detail view that allows the user to execute guided flows and raw requests for the selected service without leaving the dashboard context.

#### Scenario: Service detail exposes guided and raw operation entry points
- **WHEN** a user selects a service in the dashboard
- **THEN** Studio SHALL show operation entry points for guided flows (if available) and raw request interaction

#### Scenario: Unsupported guided services fall back to raw interaction
- **WHEN** a service has no guided flow definition
- **THEN** Studio SHALL still provide raw interaction capability and SHALL communicate guided unavailability clearly

### Requirement: Guided flow execution and lifecycle visualization
Studio SHALL execute guided flows from manifest definitions and SHALL visualize per-step progress, assertion outcomes, cleanup outcomes, and error guidance in the dashboard.

#### Scenario: Guided flow success path renders full execution timeline
- **WHEN** a guided flow completes successfully
- **THEN** Studio SHALL display step outcomes, assertion pass results, captured values, and cleanup completion status

#### Scenario: Guided flow failure path renders actionable diagnostics
- **WHEN** a guided flow step fails
- **THEN** Studio SHALL display failure step context, normalized error details, and configured error guidance with cleanup outcome reporting

### Requirement: Raw interaction execution workspace
Studio SHALL provide a raw request workspace with editable method, path, query parameters, headers, and body, and SHALL render structured response details.

#### Scenario: Raw request execution returns structured envelope
- **WHEN** a user submits a raw request from Studio
- **THEN** Studio SHALL render status code, response headers, and body in a structured response panel

#### Scenario: Raw request failure is captured without losing draft
- **WHEN** raw request execution fails
- **THEN** Studio SHALL preserve the request editor state and show diagnosable error output

### Requirement: Interaction history and replay journey
Studio SHALL maintain a session-visible history of dashboard-triggered operations and SHALL support replay into guided or raw workspaces.

#### Scenario: History entry includes operation context and outcome
- **WHEN** an operation completes from guided or raw workspace
- **THEN** Studio SHALL append a history entry containing service, operation summary, timestamp, and result status

#### Scenario: Replay restores request context for rerun
- **WHEN** a user selects a history entry and chooses replay
- **THEN** Studio SHALL pre-populate the relevant workspace with request parameters from that entry before execution

### Requirement: Studio dashboard render and accessibility baseline
The dashboard SHALL maintain keyboard-operable controls, semantic labeling for core actions, and deterministic rendering behavior suitable for component-level render testing.

#### Scenario: Dashboard core actions are keyboard-operable
- **WHEN** a keyboard-only user navigates service selection and execution controls
- **THEN** all primary actions SHALL be reachable and executable without pointer interaction

#### Scenario: Render test fixtures remain deterministic
- **WHEN** component/render integration tests run for dashboard views
- **THEN** rendered structures and state transitions SHALL be stable under deterministic fixture inputs
