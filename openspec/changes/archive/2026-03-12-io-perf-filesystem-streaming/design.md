## Context

All 24 services in openstack currently buffer entire request bodies into `Bytes` (`raw_body` in `RequestContext`) before dispatch, and all response bodies are complete `Bytes` buffers in `DispatchResponse`. The S3 service stores object data as `Vec<u8>` directly in `S3Store` structs (with base64 JSON serialization for persistence snapshots). There is no streaming, no chunked transfer encoding, and no filesystem-backed object storage. This means:

- Object size is bounded by available process memory
- Large PutObject/GetObject requests cause memory spikes proportional to object size
- Multipart upload parts are fully buffered in memory, then concatenated into another full buffer on complete
- Persistence snapshots base64-encode every object into a single JSON file, making snapshot size and I/O explosive

The gateway (`crates/gateway/src/server.rs`) reads the full body at line 152 via `axum::body::to_bytes(req.into_body(), usize::MAX)`. The `DispatchResponse.body` is `Bytes` (line 104 in `traits.rs`), and the gateway converts it to `Body::from(body)` on line 297.

## Goals / Non-Goals

**Goals:**
- Eliminate memory as the bottleneck for request/response body sizes across all services
- Store S3 object data on the filesystem rather than in process memory
- Stream S3 PutObject/UploadPart request bodies directly to disk without full buffering
- Stream S3 GetObject response bodies from disk without loading into memory
- Keep small payloads fast by using a configurable spool threshold (in-memory below threshold, filesystem above)
- Maintain 100% feature parity as validated by existing parity test suites (core, extended, all-services-smoke)
- Make the framework-level body types support both buffered and streaming variants so other services can adopt streaming later

**Non-Goals:**
- Streaming for non-S3 services (they benefit from filesystem spooling but not streaming responses in this change)
- Replacing the persistence snapshot format entirely (we adapt it to reference files on disk, but don't redesign the persistence architecture)
- Memory-mapped I/O or zero-copy transfers (future optimization)
- Compression of stored objects on disk
- S3 range-read (byte-range GET) streaming optimization (separate change)
- Changing the DashMap-based multi-tenancy model

## Decisions

### 1. Spooled body abstraction via `SpooledBody` type

**Decision**: Introduce a `SpooledBody` type in the `service-framework` crate that wraps either an in-memory `Bytes` buffer (for small payloads) or a `tempfile::SpooledTempFile` (for payloads exceeding a configurable threshold). All services receive `SpooledBody` instead of `Bytes` for request bodies.

**Rationale**: `tempfile::SpooledTempFile` already implements this pattern (memory below threshold, auto-spills to disk above), is well-maintained, and integrates with `std::io::Read`/`Write`. Wrapping it gives us an async-friendly API using `tokio::io::AsyncRead`/`AsyncWrite`. The threshold is configurable via `BODY_SPOOL_THRESHOLD_BYTES` (default 1 MiB).

**Alternatives considered**:
- *Always write to disk*: Penalizes the many small requests (SQS messages, DynamoDB items) that are well-served by memory
- *Custom ring-buffer/mmap approach*: More complex, diminishing returns for the common case
- *Bytes with a manual spill*: Re-inventing what `SpooledTempFile` already does

### 2. `ResponseBody` enum for `DispatchResponse`

**Decision**: Replace `body: Bytes` in `DispatchResponse` with `body: ResponseBody` where:
```rust
pub enum ResponseBody {
    Buffered(Bytes),
    Streaming {
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_length: Option<u64>,
    },
}
```

**Rationale**: Most services (SQS, DynamoDB, IAM, etc.) return small JSON/XML responses that are perfectly fine as buffered `Bytes`. Only S3 GetObject needs streaming. An enum keeps the common path allocation-free (no boxing for buffered) while enabling streaming where needed. The gateway converts `Streaming` to an axum `Body::from_stream()`.

**Alternatives considered**:
- *Always Box<dyn Stream>*: Adds heap allocation overhead to every response, including tiny ones
- *Generic type parameter on DispatchResponse*: Infects the entire ServiceProvider trait with generics, painful for the plugin manager
- *Separate `StreamingResponse` type returned via a different trait method*: Splits the dispatch path, complicates error handling

### 3. Filesystem-backed S3 object store

**Decision**: Store S3 object content in individual files under `{DATA_DIR}/objects/s3/{account_id}/{region}/{bucket}/{key_hash}/{version_id}`. The `ObjectVersion.data` field changes from `Vec<u8>` to an `ObjectDataRef` enum:
```rust
pub enum ObjectDataRef {
    Inline(Vec<u8>),         // small objects, delete markers
    FileRef(PathBuf),        // filesystem path to object content
}
```

**Rationale**: Using the filesystem means object size is bounded by disk space, not RAM. Content-addressed paths (using a hash of the key, since S3 keys can contain characters invalid in file paths) keep the directory structure flat and avoid path-injection issues. The `Inline` variant handles empty bodies (delete markers) and very small objects without filesystem overhead.

**Alternatives considered**:
- *Single large file with offset index (append-only log)*: Better sequential write performance but complicates deletion, versioning, and concurrent access
- *SQLite/RocksDB for blob storage*: Adds a significant dependency; filesystem is simpler and sufficient
- *Always inline (current approach)*: The problem we're solving

### 4. Streaming ingestion for S3 PutObject/UploadPart

**Decision**: Instead of buffering the full request body, S3 PutObject and UploadPart stream the request body directly to a temporary file using `tokio::io::copy()` from an `AsyncRead` adapter over the spooled body, computing the MD5 ETag incrementally via a hashing writer wrapper. Once complete, the temp file is atomically renamed to its final path.

**Rationale**: Atomic rename ensures no partial writes are visible. Incremental hashing avoids reading the data twice. The temp file lives in the same filesystem as the target to guarantee rename is atomic (no cross-device move).

### 5. Streaming delivery for S3 GetObject

**Decision**: S3 GetObject opens the object file and returns a `ResponseBody::Streaming` with a `ReaderStream` wrapping a `tokio::fs::File`. Chunk size defaults to 64 KiB. The `content-length` header is set from the stored `ObjectVersion.size` metadata.

**Rationale**: `tokio::fs::File` + `tokio_util::io::ReaderStream` is the idiomatic Tokio approach. Known content length enables HTTP clients to show progress and validate completeness.

### 6. Persistence adaptation for file-backed objects

**Decision**: The S3 persistence snapshot (`store.json`) stores `ObjectDataRef::FileRef` paths relative to the data directory, not the object bytes. On save, object files are already on disk -- the snapshot only records metadata and the path. On load, paths are resolved relative to the data directory. Non-S3 services are unaffected (they continue with JSON snapshots as-is).

**Rationale**: Avoids the current problem of base64-encoding potentially gigabytes of object data into a JSON file. Snapshot save/load becomes fast (metadata only) and object data is durable independently.

**Alternatives considered**:
- *Keep base64 in JSON, just add streaming*: Defeats the purpose -- save/load would still load everything into memory
- *Separate metadata and data stores entirely*: More invasive refactor than needed for this change

### 7. Request body access in the gateway

**Decision**: The gateway writes incoming request bodies to a `SpooledBody` (streaming from the hyper body) before dispatch. For S3 operations specifically, the raw hyper body stream is passed through to the provider so it can stream directly to disk. For all other services, the spooled body is materialized to `Bytes` for backward compatibility (via `SpooledBody::into_bytes()`).

**Rationale**: This is the minimal-disruption approach. Only S3 needs true end-to-end streaming right now. Other services continue working with `Bytes` but benefit from not holding the entire body in the gateway's memory during spooling.

## Risks / Trade-offs

- **[Disk I/O latency for small objects]** -> Mitigated by the spool threshold: objects under 1 MiB stay in memory. The threshold is configurable for tuning.

- **[Filesystem as dependency for S3 correctness]** -> Mitigated by atomic writes (temp file + rename). Disk full errors produce clear error responses (500 with InternalError). Integration tests cover disk-failure scenarios.

- **[Breaking change to DispatchResponse]** -> Mitigated by providing `ResponseBody::Buffered` as the default path. Existing `ok_json()` and `ok_xml()` helpers continue to return buffered responses. Only S3 GetObject explicitly uses `Streaming`. All 23 other services compile unchanged.

- **[Persistence snapshot format change for S3]** -> Mitigated by making `ObjectDataRef` deserialize from both the old base64 format (`Inline`) and the new path format (`FileRef`). Existing snapshots load without migration.

- **[Temp file cleanup on crash]** -> Mitigated by using the system temp directory or a configurable spool directory within the data dir. Orphaned temp files are cleaned on startup by scanning for `.tmp` suffixes in the object store directories.

- **[Concurrent access to object files during streaming]** -> Files are immutable after creation (S3 object versions are immutable). Deletion happens via the store's delete path which removes the file after removing the metadata reference. A brief race window exists where a GetObject stream holds an open file handle to a deleted object -- this is safe on Unix (unlinked files remain readable until the last fd closes).

## Open Questions

- Should the spool threshold be per-service configurable, or is a single global default sufficient for the initial implementation?
- Should we add `Content-MD5` validation on PutObject request bodies (stream through a hashing reader and compare) in this change, or defer to a separate correctness change?
- What is the cleanup policy for object files when versioning is enabled and versions are truncated beyond the 100-version cap?
