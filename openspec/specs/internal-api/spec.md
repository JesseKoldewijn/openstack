## ADDED Requirements

### Requirement: Health endpoint
The system SHALL expose `GET /_localstack/health` returning a JSON object with `services` (map of service name to state), `edition` (string), and `version` (string). Service states SHALL be one of: `available`, `running`, `starting`, `stopped`, `error`, `disabled`.

#### Scenario: Health check returns service status
- **WHEN** `GET /_localstack/health` is called after S3 has been used
- **THEN** the response SHALL include `{"services": {"s3": "running", ...}, "edition": "community", "version": "<version>"}`

#### Scenario: Health check with reload
- **WHEN** `GET /_localstack/health?reload` is called
- **THEN** the system SHALL actively check all services before returning their status

#### Scenario: HEAD health check
- **WHEN** `HEAD /_localstack/health` is called
- **THEN** the response SHALL be `200 OK` with no body (used for liveness probes)

### Requirement: Health endpoint control actions
The system SHALL support `POST /_localstack/health` with JSON body for control actions: `{"action": "restart"}` to restart the server and `{"action": "kill"}` to terminate it.

#### Scenario: Kill action
- **WHEN** `POST /_localstack/health` is called with `{"action": "kill"}`
- **THEN** the server SHALL initiate a graceful shutdown

### Requirement: Info endpoint
The system SHALL expose `GET /_localstack/info` returning a JSON object with `version`, `edition`, `is_license_activated` (always false), `session_id`, `machine_id`, `system` (OS), `is_docker` (boolean), `server_time_utc`, and `uptime` (seconds).

#### Scenario: Info endpoint returns metadata
- **WHEN** `GET /_localstack/info` is called
- **THEN** the response SHALL include the current version, platform, uptime, and a unique session ID

### Requirement: Init script runner
The system SHALL execute shell scripts found in `/etc/localstack/init/{stage}.d/` at the corresponding lifecycle stages: `boot` (before services start), `start` (during startup), `ready` (after all services ready), `shutdown` (during shutdown). Scripts SHALL be executed in alphabetical order.

#### Scenario: Ready scripts execute after startup
- **WHEN** the server finishes starting and scripts exist in `/etc/localstack/init/ready.d/`
- **THEN** each `.sh` script in that directory SHALL be executed in alphabetical order

#### Scenario: Script execution is reported
- **WHEN** `GET /_localstack/init` is called after init scripts have run
- **THEN** the response SHALL list each script with its stage, name, and execution status (completed/failed)

### Requirement: Init stage endpoint
The system SHALL expose `GET /_localstack/init/<stage>` (where stage is `boot`, `start`, `ready`, or `shutdown`) returning the list of scripts for that stage and their execution status.

#### Scenario: Query ready stage scripts
- **WHEN** `GET /_localstack/init/ready` is called
- **THEN** the response SHALL list all scripts in `/etc/localstack/init/ready.d/` with their execution results

### Requirement: Plugins endpoint
The system SHALL expose `GET /_localstack/plugins` returning information about all registered service providers and their states (available, loaded, error).

#### Scenario: List plugins
- **WHEN** `GET /_localstack/plugins` is called
- **THEN** the response SHALL include each service provider with its name, status, and load state

### Requirement: Diagnostics endpoint
The system SHALL expose `GET /_localstack/diagnose` (when `DEBUG=1` is set) returning comprehensive diagnostic information including configuration, file tree of the data directory, recent logs, and service statistics.

#### Scenario: Diagnostics when debug enabled
- **WHEN** `DEBUG=1` is set and `GET /_localstack/diagnose` is called
- **THEN** the response SHALL include config dump, file listings, and service stats

#### Scenario: Diagnostics when debug disabled
- **WHEN** `DEBUG` is not set and `GET /_localstack/diagnose` is called
- **THEN** the response SHALL be `404 Not Found` or `403 Forbidden`

### Requirement: Config endpoint
The system SHALL expose `/_localstack/config` (when `ENABLE_CONFIG_UPDATES=1` is set). `GET` SHALL return current configuration. `POST` with `{"variable": "<name>", "value": "<value>"}` SHALL update a runtime configuration variable.

#### Scenario: Read current config
- **WHEN** `ENABLE_CONFIG_UPDATES=1` and `GET /_localstack/config` is called
- **THEN** the response SHALL include all current configuration values

#### Scenario: Update config at runtime
- **WHEN** `POST /_localstack/config` is called with `{"variable": "DEBUG", "value": "1"}`
- **THEN** the `DEBUG` configuration SHALL be updated for the running instance
