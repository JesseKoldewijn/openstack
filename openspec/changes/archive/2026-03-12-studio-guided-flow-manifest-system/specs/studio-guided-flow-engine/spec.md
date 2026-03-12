## ADDED Requirements

### Requirement: Guided flow orchestration state machine
The system SHALL execute guided flows through a deterministic state machine with ordered step execution, bounded retries, timeout handling, and terminal status reporting.

#### Scenario: Flow executes steps in deterministic order
- **WHEN** a guided flow with multiple steps is started
- **THEN** steps SHALL execute in defined order and each step outcome SHALL be recorded before continuing

### Requirement: Capture and binding propagation
The guided flow engine SHALL capture outputs from completed steps and SHALL make captured values available to subsequent steps through approved binding expressions.

#### Scenario: Captured output feeds downstream step input
- **WHEN** step A captures identifier required by step B
- **THEN** step B execution SHALL resolve the captured value without manual user re-entry

### Requirement: Assertion verification
The engine SHALL evaluate declared assertions for guided flows and SHALL classify flow success/failure based on assertion outcomes.

#### Scenario: Failed assertion marks flow failed
- **WHEN** one or more required assertions fail
- **THEN** the flow SHALL be marked failed with assertion diagnostics

### Requirement: Cleanup orchestration
The engine SHALL execute cleanup steps after flow completion or failure according to cleanup policy and SHALL report cleanup outcomes.

#### Scenario: Cleanup runs after failed flow
- **WHEN** a flow fails after creating intermediate resources
- **THEN** cleanup steps SHALL still execute according to policy and report success/failure per cleanup action

### Requirement: Replay and audit history integration
Each guided flow execution SHALL emit replayable interaction history entries including resolved request data and summarized response diagnostics.

#### Scenario: User replays prior guided step request
- **WHEN** user selects a prior guided interaction entry
- **THEN** engine SHALL prepopulate replay context with equivalent request parameters for re-execution

### Requirement: User-facing error guidance mapping
Guided steps SHALL support user-facing remediation guidance when execution errors occur.

#### Scenario: Step error includes remediation hint
- **WHEN** a step defines error guidance and execution fails
- **THEN** UI state SHALL include protocol-normalized error and configured remediation hint
