## MODIFIED Requirements

### Requirement: Studio API namespace
The system SHALL expose Studio-specific endpoints under `/_localstack/studio-api/*` for service catalog discovery, interaction metadata retrieval, and manual test workflow orchestration.

The namespace SHALL additionally expose guided-flow manifest and guided coverage endpoints required for manifest-driven rendering and governance visibility.

The namespace SHALL provide dashboard-ready contract guarantees so Studio can compose home, detail, and operation workspaces without out-of-band assumptions.

#### Scenario: Studio service catalog endpoint responds
- **WHEN** `GET /_localstack/studio-api/services` is called
- **THEN** the endpoint SHALL return Studio-consumable service catalog data including support tiers

#### Scenario: Studio interaction metadata endpoint responds
- **WHEN** `GET /_localstack/studio-api/interactions/schema` is called
- **THEN** the endpoint SHALL return request/response metadata schema for Studio interaction forms and validation

#### Scenario: Studio guided manifest catalog endpoint responds
- **WHEN** `GET /_localstack/studio-api/flows/catalog` is called
- **THEN** the endpoint SHALL return guided-flow manifest index metadata for all supported services

#### Scenario: Studio guided flow definition endpoint responds
- **WHEN** `GET /_localstack/studio-api/flows/{service}` is called
- **THEN** the endpoint SHALL return validated guided-flow definitions for the requested service

#### Scenario: Studio guided coverage endpoint responds
- **WHEN** `GET /_localstack/studio-api/flows/coverage` is called
- **THEN** the endpoint SHALL return guided coverage metrics by service and maturity level

#### Scenario: Dashboard contract compatibility is preserved
- **WHEN** Studio dashboard consumes service, flow, and coverage metadata endpoints together
- **THEN** response fields and semantics SHALL remain compatible with documented Studio UI requirements and integration tests
