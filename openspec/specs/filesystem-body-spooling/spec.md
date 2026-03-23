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

### Requirement: SpooledBody SHALL support non-consuming body prefix inspection
The `SpooledBody` type SHALL provide a `peek_bytes(n: usize)` method that returns up to `n` bytes from the start of the body without consuming the body or advancing any read position. After `peek_bytes()` returns, the body SHALL still be fully readable from the beginning via `into_reader()` or `materialize()`.

#### Scenario: Peek bytes on an inline body
- **WHEN** a `SpooledBody` holds 1 KiB of inline memory data and `peek_bytes(16)` is called
- **THEN** the first 16 bytes are returned, and subsequently calling `into_reader()` yields the full 1 KiB from the beginning

#### Scenario: Peek bytes on a file-backed body
- **WHEN** a `SpooledBody` has spilled to a temp file and `peek_bytes(16)` is called
- **THEN** the first 16 bytes of the file are returned, and the file read cursor is reset so `into_reader()` still yields the full body from the beginning

#### Scenario: Peek with limit larger than body size
- **WHEN** `peek_bytes(1024)` is called on a body containing only 10 bytes
- **THEN** all 10 bytes are returned without error

### Requirement: SpooledBody SHALL support lazy materialization to Bytes
The `SpooledBody` type SHALL provide an async `materialize()` method that reads the full body into a `Bytes` value. This method is semantically equivalent to the existing `into_bytes()` but is designed for use via `RequestContext`'s lazy accessor where the result should be cached externally.

#### Scenario: Materialize inline body
- **WHEN** `materialize()` is called on an inline `SpooledBody`
- **THEN** the body bytes are returned as `Bytes` with no disk I/O

#### Scenario: Materialize file-backed body
- **WHEN** `materialize()` is called on a file-backed `SpooledBody`
- **THEN** the file is read completely and its contents returned as `Bytes`
