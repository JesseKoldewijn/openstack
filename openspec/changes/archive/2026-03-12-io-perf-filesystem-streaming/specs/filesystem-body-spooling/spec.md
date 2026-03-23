## ADDED Requirements

### Requirement: Request bodies SHALL be spooled to the filesystem above a configurable threshold
The framework SHALL provide a `SpooledBody` type that buffers request body data in memory when the payload size is at or below `BODY_SPOOL_THRESHOLD_BYTES` (default 1 MiB), and transparently spills to a temporary file on disk when the payload exceeds that threshold. All services SHALL receive request bodies through this type rather than a fully-buffered `Bytes` value.

#### Scenario: Small request body stays in memory
- **WHEN** a request arrives with a body of 512 KiB (below the default 1 MiB threshold)
- **THEN** the `SpooledBody` holds the data entirely in memory with no filesystem I/O

#### Scenario: Large request body spills to disk
- **WHEN** a request arrives with a body of 10 MiB (above the default 1 MiB threshold)
- **THEN** the `SpooledBody` writes the data to a temporary file on disk, and the in-memory buffer is released

#### Scenario: Spool threshold is configurable
- **WHEN** the environment variable `BODY_SPOOL_THRESHOLD_BYTES` is set to `524288`
- **THEN** request bodies exceeding 512 KiB are spooled to disk, and bodies at or below 512 KiB remain in memory

#### Scenario: Spooled body can be materialized to bytes
- **WHEN** a service that requires the full body as `Bytes` calls `SpooledBody::into_bytes()`
- **THEN** the full body content is returned as a contiguous `Bytes` value, reading from disk if the body was spooled

#### Scenario: Spooled body supports async reading
- **WHEN** a service needs to stream the body content
- **THEN** the `SpooledBody` implements `tokio::io::AsyncRead`, allowing incremental consumption without loading the full body into memory

### Requirement: Temporary spool files SHALL be cleaned up deterministically
The framework SHALL ensure that spool temporary files are deleted when the `SpooledBody` is dropped, and SHALL clean up orphaned `.tmp` spool files in the spool directory on startup.

#### Scenario: Spool file is cleaned up after request completes
- **WHEN** a request with a spooled body finishes processing and the `SpooledBody` is dropped
- **THEN** the temporary file on disk is deleted

#### Scenario: Orphaned spool files are cleaned on startup
- **WHEN** the server starts and the spool directory contains `.tmp` files from a previous crash
- **THEN** the orphaned temporary files are deleted during startup
