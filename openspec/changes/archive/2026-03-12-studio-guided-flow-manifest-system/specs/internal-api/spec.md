## MODIFIED Requirements

### Requirement: Info endpoint
The system SHALL expose `GET /_localstack/info` returning a JSON object with `version`, `edition`, `is_license_activated` (always false), `session_id`, `machine_id`, `system` (OS), `is_docker` (boolean), `server_time_utc`, and `uptime` (seconds).

The info response SHALL include Studio availability metadata, including Studio base path and Studio API base path when the Studio feature is enabled.

The info response SHALL additionally expose guided-flow manifest system metadata, including active manifest schema version and manifest coverage summary references.

#### Scenario: Info endpoint returns metadata
- **WHEN** `GET /_localstack/info` is called
- **THEN** the response SHALL include version, platform, uptime, and session ID

#### Scenario: Info endpoint exposes Studio metadata
- **WHEN** Studio is enabled and `GET /_localstack/info` is called
- **THEN** the response SHALL include Studio route metadata used by CLI/UI tooling

#### Scenario: Info endpoint exposes guided-flow manifest metadata
- **WHEN** guided-flow manifest system is enabled and `GET /_localstack/info` is called
- **THEN** the response SHALL include manifest schema/version metadata and coverage summary references

### Requirement: Studio API namespace
The system SHALL expose Studio-specific endpoints under `/_localstack/studio-api/*` for service catalog discovery, interaction metadata retrieval, and manual test workflow orchestration.

The namespace SHALL additionally expose guided-flow manifest and guided coverage endpoints required for manifest-driven rendering and governance visibility.

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
