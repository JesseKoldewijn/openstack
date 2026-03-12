## 1. Dependencies and Crate Setup

- [x] 1.1 Add `tempfile`, `tokio-util` (for `ReaderStream`), and `futures-core` (for `Stream` trait) dependencies to `crates/service-framework/Cargo.toml`
- [x] 1.2 Add `tokio-util` and `tempfile` dependencies to `crates/gateway/Cargo.toml`
- [x] 1.3 Add `tokio-util` and `tempfile` dependencies to `crates/services/s3/Cargo.toml`
- [x] 1.4 Add the `BODY_SPOOL_THRESHOLD_BYTES` configuration field to `crates/config/src/lib.rs` (default 1 MiB) and add the S3 object storage directory config (`S3_OBJECT_STORE_DIR`)

## 2. SpooledBody Type (service-framework)

- [x] 2.1 Create `crates/service-framework/src/spooled.rs` implementing the `SpooledBody` type wrapping `tempfile::SpooledTempFile` with an async-compatible interface
- [x] 2.2 Implement `SpooledBody::new(threshold: usize)` constructor and `SpooledBody::from_bytes(bytes: Bytes)` for pre-buffered data
- [x] 2.3 Implement `SpooledBody::write_from_stream()` async method that consumes a `futures::Stream<Item = Result<Bytes, _>>` and writes to the spooled file
- [x] 2.4 Implement `tokio::io::AsyncRead` for `SpooledBody` so consumers can stream-read the body
- [x] 2.5 Implement `SpooledBody::into_bytes()` method that materializes the full body to `Bytes` (reading from disk if spooled)
- [x] 2.6 Implement `Drop` for `SpooledBody` to ensure temp file cleanup
- [x] 2.7 Export `SpooledBody` from `crates/service-framework/src/lib.rs`
- [x] 2.8 Write unit tests for `SpooledBody`: in-memory path, spill-to-disk path, async read, into_bytes, drop cleanup

## 3. ResponseBody Enum (service-framework)

- [x] 3.1 Define `ResponseBody` enum in `crates/service-framework/src/traits.rs` with `Buffered(Bytes)` and `Streaming { stream, content_length }` variants
- [x] 3.2 Change `DispatchResponse.body` from `Bytes` to `ResponseBody`
- [x] 3.3 Update `DispatchResponse::ok_json()` to return `ResponseBody::Buffered`
- [x] 3.4 Update `DispatchResponse::ok_xml()` to return `ResponseBody::Buffered`
- [x] 3.5 Add `DispatchResponse::streaming()` constructor for the streaming variant
- [x] 3.6 Add a `ResponseBody::into_bytes()` convenience method that consumes a buffered variant or collects a stream (for tests/backward compat)

## 4. RequestContext Changes (service-framework)

- [x] 4.1 Add `spooled_body: Option<SpooledBody>` field to `RequestContext` in `crates/service-framework/src/traits.rs`
- [x] 4.2 Update `RequestContext::new()` to initialize `spooled_body: None`
- [x] 4.3 Update the gateway's `RequestContext` (in `crates/gateway/src/context.rs`) `to_service_request_context()` method to pass through the spooled body

## 5. Gateway Streaming Support

- [x] 5.1 Replace `axum::body::to_bytes(req.into_body(), usize::MAX)` in `handle_request()` with streaming the body into a `SpooledBody` using the configured threshold
- [x] 5.2 Attach the `SpooledBody` to the gateway `RequestContext` so it flows to `to_service_request_context()`
- [x] 5.3 Update the response building in `handle_request()` to match on `ResponseBody::Buffered` vs `ResponseBody::Streaming`, using `Body::from_stream()` for streaming responses
- [x] 5.4 Set `Content-Length` header from `content_length` when present on streaming responses
- [x] 5.5 Ensure error responses from `DispatchError` still use the buffered path (no changes needed)

## 6. Fix All Non-S3 Service Compilation

- [x] 6.1 Update all service providers that access `response.body` as `Bytes` to use `ResponseBody::Buffered` (grep for `.body` usage across all `crates/services/*/src/provider.rs` files)
- [x] 6.2 Verify all 24 services compile cleanly with `cargo check --workspace`

## 7. S3 Filesystem Object Storage

- [x] 7.1 Create `crates/services/s3/src/object_store.rs` module with `ObjectFileStore` struct managing the filesystem layout `{base_dir}/{account_id}/{region}/{bucket}/{key_hash}/{version_id}`
- [x] 7.2 Implement `ObjectFileStore::write_object()` — writes data to a temp file, atomically renames to final path, returns `PathBuf`
- [x] 7.3 Implement `ObjectFileStore::read_object()` — returns `tokio::fs::File` for a given path
- [x] 7.4 Implement `ObjectFileStore::delete_object()` — removes file at path, cleans up empty parent directories
- [x] 7.5 Implement `ObjectFileStore::delete_bucket_dir()` — removes entire bucket directory tree
- [x] 7.6 Implement `ObjectFileStore::copy_object()` — filesystem copy from source to destination path
- [x] 7.7 Implement `ObjectFileStore::key_hash()` — hash S3 key to filesystem-safe directory name
- [x] 7.8 Implement `ObjectFileStore::cleanup_orphaned_temps()` — scan for and remove `.tmp` files on startup
- [x] 7.9 Write unit tests for `ObjectFileStore`: write/read/delete/copy, atomic writes, key hashing, cleanup

## 8. S3 Store Data Model Changes

- [x] 8.1 Define `ObjectDataRef` enum (`Inline(Vec<u8>)`, `FileRef(PathBuf)`) in `crates/services/s3/src/store.rs`
- [x] 8.2 Implement serde for `ObjectDataRef` with backward-compatible deserialization (base64 string → Inline, path object → FileRef)
- [x] 8.3 Change `ObjectVersion.data` from `Vec<u8>` to `ObjectDataRef`
- [x] 8.4 Change `UploadPart.data` from `Vec<u8>` to `ObjectDataRef`
- [x] 8.5 Update `ObjectVersion::new()` to accept `ObjectDataRef` and compute ETag based on variant
- [x] 8.6 Update `S3Store::put_object()` to accept `ObjectDataRef` instead of `Vec<u8>`
- [x] 8.7 Update `S3Store::upload_part()` to accept `ObjectDataRef` instead of `Vec<u8>`
- [x] 8.8 Update `S3Store::complete_multipart_upload()` to work with filesystem-backed parts (delegate concatenation to ObjectFileStore)
- [x] 8.9 Update `S3Store::delete_object()` and `S3Store::delete_object_version()` to trigger file deletion via ObjectFileStore
- [x] 8.10 Update `S3Store::delete_bucket()` to trigger bucket directory deletion

## 9. S3 Provider Streaming Integration

- [x] 9.1 Update `handle_put_object()` in `crates/services/s3/src/provider.rs` to stream request body from `SpooledBody` through `ObjectFileStore::write_object()` with incremental MD5 hashing
- [x] 9.2 Update `handle_get_object()` to return `ResponseBody::Streaming` using `ReaderStream` over the object file, with `content_length` set from `ObjectVersion.size`
- [x] 9.3 Update `handle_upload_part()` to stream request body to a part file via `ObjectFileStore`
- [x] 9.4 Update `handle_complete_multipart_upload()` to concatenate parts from disk to the final object file
- [x] 9.5 Update `handle_copy_object()` to use `ObjectFileStore::copy_object()` for filesystem-level copy
- [x] 9.6 Update `handle_head_object()` to return metadata without reading the object file
- [x] 9.7 Update `handle_delete_objects()` (batch delete) to delete backing files for each removed object
- [x] 9.8 Inject `ObjectFileStore` into `S3Provider` (add as field, initialize with configured data dir on `start()`)

## 10. S3 Persistence Adaptation

- [x] 10.1 Update S3's `PersistableStore` implementation to serialize `ObjectDataRef::FileRef` as relative paths in the snapshot JSON
- [x] 10.2 Update S3's `PersistableStore::load()` to resolve `FileRef` paths relative to the data directory and verify file existence
- [x] 10.3 Add warning logging for missing object files during load (mark version as unavailable rather than crash)
- [x] 10.4 Ensure S3 save does NOT re-write object body files (metadata snapshot only)

## 11. Startup Cleanup

- [x] 11.1 Add orphaned temp file cleanup to gateway/server startup (call `ObjectFileStore::cleanup_orphaned_temps()`)
- [x] 11.2 Add orphaned spool directory cleanup on startup (scan spool dir for `.tmp` files from previous crashes)

## 12. Testing and Validation

- [x] 12.1 Run `cargo test --workspace` and fix any compilation errors or test failures from the type changes
- [x] 12.2 Update existing S3 unit tests in `crates/services/s3/tests/s3_tests.rs` to work with the new `ObjectDataRef` and filesystem-backed storage
- [x] 12.3 Add integration tests for streaming PutObject/GetObject with large bodies (verify constant memory usage pattern)
- [x] 12.4 Add integration test for multipart upload with filesystem-backed parts
- [x] 12.5 Add integration test for CopyObject between buckets with filesystem storage
- [x] 12.6 Run the core parity test suite (`tests/parity/scenarios/core.json`) and verify all S3 scenarios pass
- [x] 12.7 Run the extended parity test suite (`tests/parity/scenarios/extended.json`) and verify all scenarios pass
- [x] 12.8 Run the all-services-smoke parity suite to verify no regressions in non-S3 services
- [x] 12.9 Run the compatibility tests (`tests/compat/compatibility_tests.sh`) and verify pass
- [x] 12.10 Test persistence round-trip: create objects with `PERSISTENCE=1`, restart, verify objects are accessible and streamed correctly
