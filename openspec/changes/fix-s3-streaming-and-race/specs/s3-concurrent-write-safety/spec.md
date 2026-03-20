## ADDED Requirements

### Requirement: S3 PutObject and UploadPart SHALL use unique temp file paths per write
When writing object or part data to disk, the S3 service SHALL generate a unique temporary file path for each individual write operation. The temp file path SHALL incorporate a UUID v4 component so that concurrent writes to the same object key never collide on the same filesystem path. No two concurrent write operations SHALL share a temp file path, regardless of the target key, version ID, or bucket.

#### Scenario: Concurrent PutObject to the same key succeeds
- **WHEN** 10 concurrent PutObject requests target the same bucket and key with versioning disabled
- **THEN** all 10 requests succeed with status 200, each using a distinct temp file path, and the final object reflects exactly one of the writes with no I/O errors or corrupt data

#### Scenario: Temp file path includes a UUID component
- **WHEN** a PutObject write begins
- **THEN** the temp file is created at a path of the form `{version_id}-{uuid_v4}.tmp`, where `{uuid_v4}` is unique per write operation

#### Scenario: Versioning-disabled writes do not collide
- **WHEN** versioning is disabled and two concurrent PutObject requests target the same key (both assigned `version_id = "null"`)
- **THEN** each write uses a distinct temp file (`null-{uuid1}.tmp` and `null-{uuid2}.tmp`), and both writes complete without truncating each other

#### Scenario: Failed write leaves no corrupt object at canonical path
- **WHEN** a write operation fails after creating the temp file but before the rename
- **THEN** no file exists at the canonical object path, and the unique temp file (if present) is cleaned up on next startup
