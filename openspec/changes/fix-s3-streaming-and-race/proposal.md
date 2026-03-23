## Why

The latest CI benchmark (PR #6, run 23325947644) fails 13 gate checks: S3 PutObject has a ~50% error rate caused by a filesystem write race condition, loaded RSS is 664 MB (66x over the 10 MB ceiling) due to eager body materialization in the gateway, and S3 data-plane latencies are 25-680x over the 5 ms p95 threshold. All non-S3 services pass with excellent performance, so these are S3-specific regressions that must be fixed to unblock CI.

## What Changes

- **Fix filesystem write race condition**: S3 PutObject/UploadPart with versioning disabled always uses `version_id = "null"`, causing all concurrent writes to the same key to collide on the same temp file (`null.tmp`). Temp files will use a unique suffix (UUID) to prevent concurrent writers from truncating each other.
- **Wire up lazy body materialization in the gateway**: Remove the eager `spooled.to_bytes()` call in `server.rs` that defeats spooling by pulling the entire body into heap memory. Instead, pass the `SpooledBody` through to services and let them materialize on demand.
- **Connect S3 PutObject/UploadPart to existing streaming write path**: The `ObjectFileStore::write_object_from_reader()` method already exists but isn't called. PutObject and UploadPart will use it via `SpooledBody::into_reader()` with incremental MD5 hashing, avoiding the current `raw_body.to_vec()` copy.
- **Adapt protocol parsers for lazy body access**: JSON and RestJson parsers switch from `serde_json::from_slice()` to `serde_json::from_reader()` on `SpooledBody`. Query/EC2 parsers materialize the (always-small) body on demand. RestXml reads from `SpooledBody` for XML parsing.
- **Add incremental MD5 hashing adapter**: Wrap the `AsyncRead` stream in a `HashingReader` that computes the ETag during streaming write, eliminating the need to buffer the body for hashing.
- **Expand test coverage**: Add unit tests for SpooledBody round-trips, concurrent PutObject race-condition regression tests, parameterized size tests crossing the spool threshold, and memory-budget assertions.

## Capabilities

### New Capabilities

- `s3-concurrent-write-safety`: Ensures S3 filesystem writes use unique temp file paths per write operation, preventing data corruption and I/O errors when multiple concurrent requests target the same object key.

### Modified Capabilities

- `gateway-core`: Gateway body handling changes from eager `to_bytes()` materialization to lazy `SpooledBody` passthrough, affecting how all services receive request bodies.
- `s3-streaming-io`: PutObject and UploadPart are wired to use the streaming write path (`write_object_from_reader`) with incremental MD5, fulfilling the existing spec requirements that are currently unimplemented.
- `filesystem-body-spooling`: `SpooledBody` gains a `peek_bytes(limit)` method for service detection and a lazy `materialize()` accessor, changing how the spooled body is consumed downstream.
- `s3-filesystem-object-storage`: Atomic write temp file naming changes from `{version_id}.tmp` to `{version_id}-{uuid}.tmp` to support concurrent writers.

## Impact

- **Gateway (`crates/gateway/src/server.rs`, `context.rs`)**: Body pipeline restructured; `raw_body` population deferred.
- **Service framework (`crates/service-framework/src/traits.rs`, `spooled.rs`)**: `RequestContext.raw_body` becomes lazily populated; `SpooledBody` gets `peek_bytes()` and `materialize()` methods.
- **S3 service (`crates/services/s3/src/provider.rs`, `object_store.rs`, `store.rs`)**: PutObject/UploadPart switch from buffered to streaming writes; temp file naming gains UUID suffix.
- **Protocol parsers (`crates/aws-protocol/src/json.rs`, `rest_json.rs`, `rest_xml.rs`, `query.rs`, `ec2.rs`)**: All five parsers adapted to read from `SpooledBody` instead of `&[u8]`.
- **Other services using `raw_body`**: SQS, SNS, Route53, Lambda, CloudWatch — minimal changes to call lazy materializer instead of reading `raw_body` directly.
- **Tests**: New unit tests, concurrent write regression tests, large object integration tests; existing parity and benchmark tests must continue to pass.
- **Benchmark gate**: All 13 current failures should be resolved — zero errors, memory ≤ 10 MB, p95 ≤ 5 ms.
