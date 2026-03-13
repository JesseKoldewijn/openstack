## MODIFIED Requirements

### Requirement: Health endpoint
The system SHALL expose `GET /_localstack/health` returning a JSON object with `services` (map of service name to state), `edition` (string), and `version` (string). Service states SHALL be one of: `available`, `running`, `starting`, `stopped`, `error`, `disabled`.

The health response SHALL include daemon lifecycle visibility fields when running under managed daemon mode, including daemon status classification and process metadata suitable for CLI status reporting.

#### Scenario: Health check returns service status
- **WHEN** `GET /_localstack/health` is called after S3 has been used
- **THEN** the response SHALL include service state entries and version metadata

#### Scenario: Health check with reload
- **WHEN** `GET /_localstack/health?reload` is called
- **THEN** the system SHALL actively check all services before returning status

#### Scenario: HEAD health check
- **WHEN** `HEAD /_localstack/health` is called
- **THEN** the response SHALL be `200 OK` with no body

#### Scenario: Daemon-aware status fields are present
- **WHEN** openstack runs in managed daemon mode and `GET /_localstack/health` is called
- **THEN** the response SHALL include daemon status metadata sufficient for CLI `status` command decisions

### Requirement: Health endpoint control actions
The system SHALL support `POST /_localstack/health` with JSON body for control actions: `{"action": "restart"}` to restart the server and `{"action": "kill"}` to terminate it.

The control action handling SHALL preserve graceful shutdown semantics for daemon-managed instances so state/save hooks execute before process exit where possible.

#### Scenario: Kill action
- **WHEN** `POST /_localstack/health` is called with `{"action": "kill"}`
- **THEN** the server SHALL initiate graceful shutdown

#### Scenario: Restart action
- **WHEN** `POST /_localstack/health` is called with `{"action": "restart"}`
- **THEN** the running instance SHALL initiate restart behavior as defined by lifecycle control policy

### Requirement: Info endpoint
The system SHALL expose `GET /_localstack/info` returning a JSON object with `version`, `edition`, `is_license_activated` (always false), `session_id`, `machine_id`, `system` (OS), `is_docker` (boolean), `server_time_utc`, and `uptime` (seconds).

The info response SHALL include Studio availability metadata, including Studio base path and Studio API base path when the Studio feature is enabled.

#### Scenario: Info endpoint returns metadata
- **WHEN** `GET /_localstack/info` is called
- **THEN** the response SHALL include version, platform, uptime, and session ID

#### Scenario: Info endpoint exposes Studio metadata
- **WHEN** Studio is enabled and `GET /_localstack/info` is called
- **THEN** the response SHALL include Studio route metadata used by CLI/UI tooling

### Requirement: Plugins endpoint
The system SHALL expose `GET /_localstack/plugins` returning information about all registered service providers and their states (available, loaded, error).

The endpoint SHALL support capability metadata projection for Studio catalog generation, including service support tier values for Studio interaction modes.

#### Scenario: List plugins
- **WHEN** `GET /_localstack/plugins` is called
- **THEN** the response SHALL include each service provider with its name, status, and load state

#### Scenario: Studio capability metadata is exposed
- **WHEN** Studio queries plugin/capability metadata
- **THEN** the response SHALL include per-service Studio support tier information

### Requirement: Studio API namespace
The system SHALL expose Studio-specific endpoints under `/_localstack/studio-api/*` for service catalog discovery, interaction metadata retrieval, and manual test workflow orchestration.

#### Scenario: Studio service catalog endpoint responds
- **WHEN** `GET /_localstack/studio-api/services` is called
- **THEN** the endpoint SHALL return Studio-consumable service catalog data including support tiers

#### Scenario: Studio interaction metadata endpoint responds
- **WHEN** `GET /_localstack/studio-api/interactions/schema` is called
- **THEN** the endpoint SHALL return request/response metadata schema for Studio interaction forms and validation
