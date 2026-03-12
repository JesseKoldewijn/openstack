## Why

All service input data and S3 object bodies are currently held entirely in memory as `Vec<u8>` / `Bytes` buffers, which caps the practical object size at available RAM and prevents handling large payloads efficiently. Moving input storage to the filesystem and adding streaming I/O for S3 operations removes memory as the bottleneck, enables arbitrarily large objects, and brings performance characteristics closer to real AWS behavior -- all while retaining feature parity validated by the existing parity test suite.

## What Changes

- Introduce a filesystem-backed temporary storage layer that all services use for incoming request bodies instead of buffering in memory. Input bodies are spooled to disk above a configurable threshold, keeping small payloads in memory for speed.
- Replace the in-memory `Vec<u8>` object data in `S3Store` (`ObjectVersion.data`, `UploadPart.data`) with filesystem-backed handles, so S3 object content lives on disk rather than in process memory.
- Add streaming request ingestion for S3 `PutObject` and `UploadPart` -- stream the request body directly to disk without full buffering.
- Add streaming response delivery for S3 `GetObject` -- stream object content from disk through the HTTP response using chunked transfer encoding.
- Update `DispatchResponse` to support a streaming body variant alongside the existing `Bytes` body, so the gateway can serve streamed responses without loading full content into memory.
- Update `RequestContext` to provide access to a spooled body handle (file-backed above threshold) instead of requiring the full `raw_body: Bytes` upfront.
- Adapt the persistence layer so filesystem-stored objects are correctly handled during snapshot save/load cycles (reference files on disk rather than base64-encoding large blobs into JSON).
- **BREAKING**: `DispatchResponse.body` changes from `Bytes` to an enum supporting both buffered and streaming variants. Service providers returning large bodies should migrate to the streaming variant.

## Capabilities

### New Capabilities
- `filesystem-body-spooling`: Configurable spooling of request bodies to disk above a size threshold, replacing full in-memory buffering across all services.
- `s3-filesystem-object-storage`: Filesystem-backed S3 object and multipart-part storage, replacing in-memory `Vec<u8>` data fields.
- `s3-streaming-io`: Streaming request ingestion (PutObject, UploadPart) and streaming response delivery (GetObject) for S3, using async readers/writers over filesystem-backed storage.
- `streaming-response-support`: Extension of `DispatchResponse` and the gateway to support streamed response bodies alongside buffered ones.

### Modified Capabilities
- `state-persistence`: Snapshot persistence must handle filesystem-backed object references instead of base64-encoding large blobs into JSON snapshots. Persistence save/load must coordinate with the filesystem object store.
- `service-framework`: `RequestContext` and `DispatchResponse` types change to support spooled/streaming bodies. The `ServiceProvider` trait contract is affected by the new body types.
- `gateway-core`: The gateway response path must support streaming bodies (chunked transfer encoding) in addition to buffered responses.

## Impact

- **Core types**: `RequestContext`, `DispatchResponse`, `ServiceProvider` trait in `crates/service-framework/`
- **Gateway**: `crates/gateway/src/server.rs` response serialization path
- **S3 service**: `crates/services/s3/src/store.rs` (data model), `provider.rs` (all object operations)
- **State/persistence**: `crates/state/src/persistence.rs` (snapshot save/load for file-backed data)
- **All other services**: Minor -- they gain filesystem body spooling for free via the framework, but their in-memory stores are unchanged in this change
- **Dependencies**: New deps on `tempfile` (or similar) for spooled files, `tokio-util` for `ReaderStream`/`StreamReader`
- **Parity tests**: All existing core/extended/all-services-smoke parity scenarios must continue to pass
- **Config**: New env vars for spool threshold (e.g., `BODY_SPOOL_THRESHOLD_BYTES`, default 1 MiB) and S3 object storage directory
