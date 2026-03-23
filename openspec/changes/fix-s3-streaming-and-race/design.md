## Context

The gateway currently eagerly materializes every request body via `spooled.to_bytes()` (server.rs:490), allocating the full body as `Bytes` on the heap immediately after spooling it to disk. This defeats the purpose of `SpooledBody`: for a 100 MB upload the peak memory is ~300 MB (spool buffer + `Bytes` + `to_vec()` copy in the provider), and at 6 concurrent requests to the same object the process RSS hits 664 MB.

Independently, S3's atomic-write pattern uses a temp file path of `{version_id}.tmp`. When versioning is disabled, `version_id` is always the string `"null"`, so every concurrent PutObject to the same key races on the same `null.tmp` file: `File::create()` truncates in-progress writes and `rename()` calls collide, producing ~50% I/O errors.

The streaming infrastructure to fix both problems already exists: `SpooledBody::into_reader()` returns an `AsyncRead`, and `ObjectFileStore::write_object_from_reader()` accepts one—but PutObject calls the buffered `write_object(&[u8])` variant instead.

## Goals / Non-Goals

**Goals:**
- Eliminate the filesystem write race by using unique temp file names per write operation
- Remove the eager `to_bytes()` materialization from the gateway so large bodies never live entirely in heap memory
- Connect S3 PutObject and UploadPart to the existing `write_object_from_reader()` streaming path with incremental MD5
- Adapt all five protocol parsers to consume `SpooledBody` directly rather than a pre-materialized `&[u8]`
- Keep all other 23 services functionally identical with no performance regression
- Pass all five CI gates: unit tests, integration tests, parity tests, clippy, rustfmt, and benchmark gate (p95 ≤ 5 ms, RSS ≤ 10 MB, zero errors)

**Non-Goals:**
- Streaming response bodies for non-S3 services (GetObject streaming already exists; no new response streaming work)
- Signature validation (not currently validated; no change)
- Changing the on-disk object storage layout or metadata format
- Streaming request bodies for services other than S3 PutObject/UploadPart (all other services have small bodies; lazy materialization is sufficient)
- Multipart CompleteMultipartUpload body streaming (its body is tiny XML; only the parts themselves are large)

## Decisions

### D1: Unique temp file names via UUID suffix

**Decision**: Rename temp files from `{version_id}.tmp` to `{version_id}-{uuid_v4}.tmp`.

**Rationale**: The race happens because concurrent writers share the same path. A per-write UUID makes each temp path unique, so `File::create()`, `write_all()`, and `rename()` never collide. UUID v4 requires no coordination between threads and is already available via the `uuid` crate (already a dependency via the store layer).

**Alternative considered**: Using a thread-local counter or timestamp. Rejected because neither guarantees uniqueness under async concurrency without a lock, and UUIDs are already used elsewhere in the codebase.

### D2: Lazy body materialization — SpooledBody flows through to service dispatch

**Decision**: Remove `spooled.to_bytes()` from the gateway. `RequestContext.raw_body` becomes `Option<Bytes>`, defaulting to `None`. Services that need raw bytes call a new `ctx.raw_body()` async method that materializes on first access (and caches the result). Services that don't need the raw body (14 of 24) pay zero cost.

**Rationale**: This is the minimal-diff path. The gateway already constructs `RequestContext` and passes it to every service; changing `raw_body: Bytes` to `raw_body: Option<Bytes>` with lazy fill requires only the gateway change and a small accessor method. Protocol parsers that currently receive `&ctx.raw_body` instead call `ctx.materialize_body().await?` — a one-liner change per parser.

**Alternative considered**: Making `SpooledBody` part of the public `RequestContext` API with explicit ownership transfer to each service. Rejected because it requires every service to be aware of the `SpooledBody` type and adds complexity to service authors. The `Option<Bytes>` lazy approach hides the mechanism behind a single async accessor.

**Alternative considered**: Moving to a true single-pass streaming pipeline where the body is never re-readable. Rejected for this change: several operations (e.g., `Content-MD5` validation before write, DeleteObjects XML parsing) need the body twice or in multiple contexts. Full single-pass streaming is a larger architectural refactor; lazy materialization recovers most of the memory benefit with minimal risk.

### D3: S3 PutObject/UploadPart use write_object_from_reader with HashingReader

**Decision**: PutObject and UploadPart call `object_store.write_object_from_reader(spooled_body.into_reader())` wrapped in a `HashingReader<md5::Md5>` adapter that computes the ETag incrementally during the write. After the write completes, `hashing_reader.finalize()` returns the hex digest for the `ETag` response header.

**Rationale**: `write_object_from_reader()` already exists on `ObjectFileStore` and accepts any `AsyncRead + Unpin`. `SpooledBody::into_reader()` already returns one. The only missing piece is the MD5 adapter. A thin `HashingReader<D: Digest>` wrapper that implements `AsyncRead` by delegating to an inner reader while feeding chunks to a digest is ~30 lines and composes cleanly.

**Alternative considered**: Computing MD5 as a separate pass after the write (read the file back from disk). Rejected: doubles disk I/O and requires the object to be fully on disk before confirming the ETag.

**Alternative considered**: Using the existing `Content-MD5` header from the client request as the ETag. Rejected: the header is optional and its format (base64) differs from S3's hex ETag. We must compute our own.

### D4: Protocol parsers use SpooledBody via read-to-bytes for non-S3, from_reader for JSON

**Decision**: 
- **JSON / RestJson parsers**: Use `serde_json::from_reader(spooled_body.into_reader().compat())` — zero copy, works for any body size.
- **Query / EC2 parsers**: Call `ctx.raw_body().await?` (lazy materialize). Query bodies are always small (form-encoded parameters) so full materialization is fine.
- **RestXml parser**: Call `ctx.raw_body().await?` and parse from slice. XML bodies for RestXml ops are also small (DeleteObjects lists are bounded by the 1000-key API limit).

**Rationale**: JSON bodies can be arbitrarily large for some services in principle, so using `from_reader` is future-safe. Query and XML bodies are structurally bounded in size and benefit more from simplicity than from avoiding materialization.

### D5: SpooledBody gains peek_bytes(n) for service detection

**Decision**: Add `SpooledBody::peek_bytes(n: usize) -> io::Result<&[u8]>` that returns up to `n` bytes from the start of the body without consuming it. Used by the CloudWatch handler (which currently calls `raw_body.starts_with(b"Action=")`) and by any future detect-service logic that inspects the body prefix.

**Rationale**: Without this, removing eager `to_bytes()` would break the CloudWatch body-prefix check. A bounded peek is efficient: for inline bodies it's a slice; for file-backed bodies it reads only `n` bytes.

## Risks / Trade-offs

- **[Risk] Regression in a service that silently depends on `raw_body` being pre-populated** → Mitigation: Audit all 24 service providers for `ctx.raw_body` access at compile time (the type change to `Option<Bytes>` turns all existing access into compile errors, forcing explicit migration of every call site).

- **[Risk] `serde_json::from_reader` requires `Read` not `AsyncRead`; bridging requires `.compat()` from `tokio_util::io::SyncIoBridge` or `futures::io::AllowStdIo`** → Mitigation: Use `tokio_util::io::ReaderStream` + collect, or `SyncIoBridge` in a `spawn_blocking` for the actual JSON parse. Both are already available; evaluate in implementation. If blocking is required, the latency impact is negligible for small service request bodies.

- **[Risk] UUID generation overhead per PutObject** → Mitigation: UUID v4 is a rand call — nanoseconds. Not measurable against disk I/O.

- **[Risk] `into_reader()` consumes the `SpooledBody`; if the write fails mid-stream the body is gone** → Mitigation: On write failure, return the error immediately; the HTTP response is already determined (500). The client will retry. No body re-reading is needed after a failed write.

- **[Risk] Tests that set `raw_body: Bytes::new()` on `RequestContext` will break when the field type changes** → Mitigation: The compile error is intentional — each test site must be updated to either use `raw_body: Some(bytes)` or rely on the lazy path. This is a forcing function for test correctness.

## Migration Plan

1. **Phase 1 (no behavior change)**: Add `SpooledBody::peek_bytes()` and `materialize()` methods; add `HashingReader` adapter in service-framework. All tests pass.
2. **Phase 2 (race fix only)**: Change temp file naming to `{version_id}-{uuid}.tmp` in `object_store.rs`. Run benchmark — error rate drops to zero, memory still high.
3. **Phase 3 (lazy gateway + parser adaptation)**: Change `RequestContext.raw_body` to `Option<Bytes>`; remove `to_bytes()` from gateway; update all five parsers; update all services that reference `raw_body`. Run full test suite — all must pass.
4. **Phase 4 (S3 streaming write)**: Wire PutObject/UploadPart to `write_object_from_reader` + `HashingReader`. Run benchmark — memory and latency gates should pass.
5. **Verify**: Run `cargo test --all`, `cargo clippy --all`, `cargo fmt --check`, parity tests, and benchmark gate locally before pushing.

Rollback: Each phase is independently revertable. Phase 2 (UUID rename) is the safest standalone fix and can be merged independently if time pressure demands.

## Open Questions

- **SyncIoBridge vs spawn_blocking for JSON parsing**: Does `serde_json::from_reader` on a `SyncIoBridge`-wrapped `SpooledBody` block the Tokio thread long enough to matter? Benchmark both approaches during implementation; if blocking latency exceeds 1 ms for typical service bodies, use `spawn_blocking`.
- **peek_bytes backing for file-backed SpooledBody**: Should `peek_bytes` cache the peeked bytes in the `SpooledBody` struct, or seek back to 0 before the subsequent `into_reader()` call? Evaluate which is simpler given `SpooledBody`'s current internal representation.
