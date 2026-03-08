## ADDED Requirements

### Requirement: Account-region scoped state stores
Each service SHALL store its state in an `AccountRegionBundle` that isolates data by `(account_id, region)` tuple. Accessing state for a request SHALL automatically use the account ID and region extracted from the request context.

#### Scenario: State isolation between accounts
- **WHEN** account `111111111111` creates SQS queue `my-queue` in `us-east-1`
- **THEN** account `222222222222` calling `ListQueues` in `us-east-1` SHALL NOT see `my-queue`

#### Scenario: State isolation between regions
- **WHEN** account `000000000000` creates an S3 bucket in `us-east-1`
- **THEN** `ListBuckets` in `eu-west-1` for the same account SHALL NOT include that bucket (S3 bucket names are global but buckets have a region)

### Requirement: Cross-region attributes
Services with globally-scoped resources (e.g., IAM users, S3 bucket names) SHALL use cross-region attributes that are shared across all regions within the same account.

#### Scenario: IAM user visible across regions
- **WHEN** an IAM user is created in `us-east-1`
- **THEN** `GetUser` in `ap-southeast-1` for the same account SHALL return the same user

### Requirement: Cross-account attributes
Resources that must be globally unique across all accounts (e.g., S3 bucket names) SHALL use cross-account attributes.

#### Scenario: S3 bucket name uniqueness
- **WHEN** account `111111111111` creates bucket `unique-bucket`
- **THEN** account `222222222222` attempting to create bucket `unique-bucket` SHALL receive a `BucketAlreadyExists` error

### Requirement: Snapshot persistence to disk
The system SHALL support persisting all service state to disk when `PERSISTENCE=1` is set. State SHALL be serialized to the data directory (default `/var/lib/localstack/state`) organized by `{service}/{account_id}/{region}/`.

#### Scenario: State survives restart with persistence
- **WHEN** `PERSISTENCE=1` is set, an SQS queue is created, the server is stopped and restarted
- **THEN** `ListQueues` SHALL return the previously created queue

#### Scenario: State lost without persistence
- **WHEN** `PERSISTENCE` is not set, an SQS queue is created, and the server is restarted
- **THEN** `ListQueues` SHALL return an empty list

### Requirement: Snapshot save strategies
The system SHALL support configurable save strategies via `SNAPSHOT_SAVE_STRATEGY`: `ON_SHUTDOWN` (default -- save when the server stops), `ON_REQUEST` (save after every mutating request), `SCHEDULED` (save at intervals defined by `SNAPSHOT_FLUSH_INTERVAL`), and `MANUAL` (save only on explicit API call).

#### Scenario: Save on shutdown
- **WHEN** `SNAPSHOT_SAVE_STRATEGY=ON_SHUTDOWN` and the server receives SIGTERM
- **THEN** all service state SHALL be serialized to disk before the process exits

#### Scenario: Scheduled save
- **WHEN** `SNAPSHOT_SAVE_STRATEGY=SCHEDULED` and `SNAPSHOT_FLUSH_INTERVAL=10`
- **THEN** state SHALL be saved to disk every 10 seconds

### Requirement: Snapshot load strategies
The system SHALL support configurable load strategies via `SNAPSHOT_LOAD_STRATEGY`: `ON_STARTUP` (default -- load when the server starts), `ON_REQUEST` (load on first request to each service), and `MANUAL` (load only on explicit API call).

#### Scenario: Load on startup
- **WHEN** `SNAPSHOT_LOAD_STRATEGY=ON_STARTUP` and persisted state exists on disk
- **THEN** all service state SHALL be deserialized from disk during server startup

### Requirement: State lifecycle hooks
Service providers SHALL be able to implement lifecycle hooks that are called before and after state operations: `on_before_state_save`, `on_after_state_save`, `on_before_state_load`, `on_after_state_load`, `on_before_state_reset`, `on_after_state_reset`.

#### Scenario: Pre-save hook cleans transient data
- **WHEN** a service implements `on_before_state_save` to clear cached data
- **THEN** the hook SHALL be called before the state is serialized, and the cleared data SHALL NOT appear in the snapshot

### Requirement: State reset
The system SHALL support resetting all state to empty via an internal API call. This SHALL invoke `on_before_state_reset` and `on_after_state_reset` hooks on all providers.

#### Scenario: Reset clears all state
- **WHEN** a state reset is triggered after multiple services have accumulated state
- **THEN** all service stores SHALL be emptied and subsequent list operations SHALL return empty results
