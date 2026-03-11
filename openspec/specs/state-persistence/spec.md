## MODIFIED Requirements

### Requirement: Snapshot persistence to disk
The system SHALL support persisting all service state to disk when `PERSISTENCE=1` is set. State SHALL be serialized to the data directory (default `/var/lib/localstack/state`) organized by `{service}/{account_id}/{region}/`. Persistence behavior SHALL define parity-grade durability classes per service and SHALL declare whether each service is durable, recoverable-with-known-limits, or non-durable in the active mode.

#### Scenario: State survives restart with persistence
- **WHEN** `PERSISTENCE=1` is set, an SQS queue is created, the server is stopped and restarted
- **THEN** `ListQueues` SHALL return the previously created queue

#### Scenario: State lost without persistence
- **WHEN** `PERSISTENCE` is not set, an SQS queue is created, and the server is restarted
- **THEN** `ListQueues` SHALL return an empty list

#### Scenario: Service durability class is reported
- **WHEN** persistence-capability diagnostics are requested
- **THEN** each supported service SHALL report its durability class and active persistence mode

### Requirement: Snapshot save strategies
The system SHALL support configurable save strategies via `SNAPSHOT_SAVE_STRATEGY`: `ON_SHUTDOWN` (default -- save when the server stops), `ON_REQUEST` (save after every mutating request), `SCHEDULED` (save at intervals defined by `SNAPSHOT_FLUSH_INTERVAL`), and `MANUAL` (save only on explicit API call). Save strategy behavior SHALL include deterministic failure classes and remediation details when a save operation does not complete.

#### Scenario: Save on shutdown
- **WHEN** `SNAPSHOT_SAVE_STRATEGY=ON_SHUTDOWN` and the server receives SIGTERM
- **THEN** all service state SHALL be serialized to disk before the process exits

#### Scenario: Scheduled save
- **WHEN** `SNAPSHOT_SAVE_STRATEGY=SCHEDULED` and `SNAPSHOT_FLUSH_INTERVAL=10`
- **THEN** state SHALL be saved to disk every 10 seconds

#### Scenario: Save failure is diagnosable
- **WHEN** a save attempt fails due to IO or serialization error
- **THEN** the system SHALL emit a deterministic persistence failure class with service and path context

### Requirement: Snapshot load strategies
The system SHALL support configurable load strategies via `SNAPSHOT_LOAD_STRATEGY`: `ON_STARTUP` (default -- load when the server starts), `ON_REQUEST` (load on first request to each service), and `MANUAL` (load only on explicit API call). Load behavior SHALL be parity-safe and SHALL fail fast for required durable modes when required state cannot be recovered.

#### Scenario: Load on startup
- **WHEN** `SNAPSHOT_LOAD_STRATEGY=ON_STARTUP` and persisted state exists on disk
- **THEN** all service state SHALL be deserialized from disk during server startup

#### Scenario: Required durable mode fails on unrecoverable state
- **WHEN** a required durable mode is active and persisted state is unrecoverable
- **THEN** startup SHALL fail with deterministic diagnostics rather than silently continuing with empty state
