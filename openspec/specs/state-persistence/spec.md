## MODIFIED Requirements

### Requirement: Snapshot persistence to disk
The system SHALL support persisting all service state to disk when `PERSISTENCE=1` is set. State SHALL be serialized to the data directory (default `/var/lib/localstack/state`) organized by `{service}/{account_id}/{region}/`. Persistence behavior SHALL define parity-grade durability classes per service and SHALL declare whether each service is durable, recoverable-with-known-limits, or non-durable in the active mode.

For services that use filesystem-backed data storage (S3), the snapshot SHALL store metadata and file path references rather than serializing object data inline. The snapshot format SHALL support backward-compatible deserialization from both the legacy base64-inline format and the new file-reference format.

- **Scenario: State survives restart with persistence** - When `PERSISTENCE=1`, create SQS queue, restart, `ListQueues` returns it.
- **Scenario: State lost without persistence** - When `PERSISTENCE` not set, queue is lost on restart.
- **Scenario: Service durability class is reported** - Each service reports its durability class and active persistence mode.
- **Scenario: S3 snapshot stores file references not inline data** - When `PERSISTENCE=1` and an S3 object exists with filesystem-backed storage, the snapshot JSON contains a file path reference for the object data, not base64-encoded bytes.
- **Scenario: Legacy base64 snapshot loads correctly** - When loading a snapshot that contains base64-encoded inline object data (pre-migration format), the system deserializes it as `ObjectDataRef::Inline` and operates correctly.
- **Scenario: S3 object files persist independently of snapshot** - When `PERSISTENCE=1`, S3 object files written to `{DATA_DIR}/objects/s3/` survive restart independently of the metadata snapshot, and the snapshot references resolve correctly after reload.

### Requirement: Snapshot save strategies
Configurable via `SNAPSHOT_SAVE_STRATEGY`: `ON_SHUTDOWN` (default), `ON_REQUEST`, `SCHEDULED`, `MANUAL`. Includes deterministic failure classes and remediation on save failures.

For S3, the save strategy SHALL only persist metadata (bucket configs, object version metadata, multipart state) to the snapshot file. Object body files are already durable on disk and SHALL NOT be re-serialized during save.

- **Scenario: Save on shutdown** - SIGTERM triggers full state serialization before exit.
- **Scenario: Scheduled save** - `SNAPSHOT_FLUSH_INTERVAL=10` saves every 10 seconds.
- **Scenario: Save failure is diagnosable** - IO/serialization errors emit deterministic failure class with context.
- **Scenario: S3 save does not re-write object files** - When saving S3 state, only the metadata snapshot is written; object body files on disk are not touched.

### Requirement: Snapshot load strategies
Configurable via `SNAPSHOT_LOAD_STRATEGY`: `ON_STARTUP` (default), `ON_REQUEST`, `MANUAL`. Parity-safe; fails fast for required durable modes.

On load, the S3 store SHALL verify that referenced object files exist on disk. Missing file references SHALL be reported as a recoverable warning (the object metadata entry is retained but the object is marked as unavailable) rather than a fatal error.

- **Scenario: Load on startup** - Persisted state is deserialized during startup.
- **Scenario: Required durable mode fails on unrecoverable state** - Startup fails with diagnostics rather than silently using empty state.
- **Scenario: Missing object file on load produces warning** - When an S3 snapshot references an object file that no longer exists on disk, the system logs a warning with the bucket, key, and expected path, and marks the object version as unavailable rather than crashing.

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
