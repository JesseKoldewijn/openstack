## ADDED Requirements

### Requirement: Canonical normalized operation contract
The system SHALL define a canonical normalized operation model for guided flows and SHALL execute protocol-specific serialization/parsing via adapters.

#### Scenario: Normalized operation dispatches through adapter
- **WHEN** guided flow step defines canonical operation fields
- **THEN** execution SHALL route through the adapter matching the service protocol

### Requirement: Query protocol adapter
The query adapter SHALL serialize operations to query/form payloads and SHALL support deterministic extraction of response fields used in captures and assertions.

#### Scenario: Query adapter serializes action operation
- **WHEN** a query service step defines operation `CreateQueue`
- **THEN** adapter SHALL produce protocol-valid query/form request and parse response metadata for captures

### Requirement: JSON target protocol adapter
The json_target adapter SHALL attach target headers and JSON payload semantics required for target-style services.

#### Scenario: JSON target adapter sets target metadata
- **WHEN** a json_target operation is executed
- **THEN** adapter SHALL set protocol-required target metadata and parse JSON response fields for bindings

### Requirement: REST XML protocol adapter
The rest_xml adapter SHALL support path/query style request construction and XML extraction semantics for capture/assertion checks.

#### Scenario: REST XML adapter parses XML capture field
- **WHEN** an XML response contains captured value used by next step
- **THEN** adapter SHALL resolve the capture and expose it to downstream expression bindings

### Requirement: REST JSON protocol adapter
The rest_json adapter SHALL support REST path semantics with JSON request/response handling and standardized error mapping.

#### Scenario: REST JSON adapter returns structured error mapping
- **WHEN** operation fails with protocol error response
- **THEN** adapter SHALL return normalized error information for guided UI error presentation

### Requirement: Adapter conformance tests
Each protocol adapter SHALL have conformance tests validating serialization, parsing, captures, assertions, and error normalization.

#### Scenario: Adapter conformance suite fails on serialization regression
- **WHEN** protocol serialization output deviates from expected canonical fixture
- **THEN** conformance tests SHALL fail and block merge
