## ADDED Requirements

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
