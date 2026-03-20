## MODIFIED Requirements

### Requirement: Object files SHALL be written atomically
The S3 service SHALL write object data to a temporary file first and then atomically rename it to the final path. No partially-written object file SHALL be visible at the canonical path. The temporary file path SHALL include a UUID v4 component in addition to the version ID, ensuring uniqueness across concurrent write operations targeting the same object key.

#### Scenario: Atomic write via rename
- **WHEN** a PutObject writes object data to disk
- **THEN** the data is first written to a temp file with a path of the form `{version_id}-{uuid_v4}.tmp` in the same directory, then renamed to the final canonical path upon completion

#### Scenario: Incomplete write on crash leaves no corrupt file
- **WHEN** the process crashes during a PutObject write
- **THEN** no file exists at the canonical object path (only a `.tmp` file may remain, cleaned up on next startup)

#### Scenario: Concurrent writes to the same key use distinct temp files
- **WHEN** two concurrent PutObject requests target the same key with the same version ID
- **THEN** each write uses a distinct temp file path (different UUID suffixes), and neither write truncates or corrupts the other's in-progress data
