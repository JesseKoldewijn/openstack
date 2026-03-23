## 1. Phase 1 — Foundational Helpers (no behavior change)

- [x] 1.1 Add `SpooledBody::peek_bytes(n: usize)` method: return first `n` bytes without consuming the body; for inline variant slice the buffer, for file variant read and seek back to 0
- [x] 1.2 Add `SpooledBody::materialize()` async method as an alias for `into_bytes()` with a clear name that signals "lazy on-demand access"
- [x] 1.3 Add `HashingReader<D: Digest>` struct in `crates/service-framework/src/hashing.rs`: wraps an `AsyncRead + Unpin`, feeds each poll chunk to the digest, exposes `finalize() -> Output<D>`
- [x] 1.4 Write unit tests for `HashingReader`: verify MD5 over known byte sequences matches expected hex digest; verify digest is byte-for-byte correct vs `md5::compute()`
- [x] 1.5 Write unit tests for `SpooledBody::peek_bytes`: inline and file-backed cases; verify body remains fully readable after peek
- [x] 1.6 Run `cargo test -p service-framework` — all tests green

## 2. Phase 2 — Fix the Filesystem Race Condition

- [x] 2.1 Add `uuid` crate to `crates/services/s3/Cargo.toml` if not already present; use `uuid::Uuid::new_v4()`
- [x] 2.2 In `crates/services/s3/src/object_store.rs`, change the temp file path from `format!("{version_id}.tmp")` to `format!("{version_id}-{}.tmp", Uuid::new_v4())` in `write_object()` and `write_object_from_reader()`
- [x] 2.3 In `crates/services/s3/src/object_store.rs`, apply the same UUID-suffixed temp naming to part files in the UploadPart write path
- [x] 2.4 Write a unit test in `s3_tests.rs` for concurrent PutObject to the same key: spawn 10 concurrent tasks each calling the write path, assert all succeed with no errors and the final object exists
- [x] 2.5 Run `cargo test -p s3` — all tests green
- [x] 2.6 Run a quick local benchmark (or smoke test) — confirm error rate drops to zero for same-key concurrent writes

## 3. Phase 3 — Lazy Body Materialization in the Gateway

- [x] 3.1 Change `raw_body: Bytes` to `raw_body: Option<Bytes>` in `crates/service-framework/src/traits.rs` `RequestContext` struct
- [x] 3.2 Add `RequestContext::raw_body() -> &Option<Bytes>` accessor and `RequestContext::materialize_body(spooled: &mut SpooledBody) -> io::Result<Bytes>` helper that fills and caches `raw_body`
- [x] 3.3 In `crates/gateway/src/server.rs`, remove the `spooled.to_bytes()` call; set `raw_body: None` in the `RequestContext` construction; pass `spooled_body: Some(spooled)` on the context
- [x] 3.4 Adapt `crates/aws-protocol/src/json.js`: replace `serde_json::from_slice(&ctx.raw_body)` with `serde_json::from_reader(SyncIoBridge::new(spooled_reader))` via `spawn_blocking`, or materialize body and use `from_slice` — evaluate latency in implementation
- [x] 3.5 Adapt `crates/aws-protocol/src/rest_json.rs` the same way as `json.rs`
- [x] 3.6 Adapt `crates/aws-protocol/src/rest_xml.rs`: call `ctx.materialize_body()` and pass `&[u8]` to the XML parser
- [x] 3.7 Adapt `crates/aws-protocol/src/query.rs`: call `ctx.materialize_body()` and parse form-encoded bytes
- [x] 3.8 Adapt `crates/aws-protocol/src/ec2.rs` the same way as `query.rs`
- [x] 3.9 Fix all compile errors from the `Option<Bytes>` type change across all service providers (SQS, SNS, Route53, Lambda, CloudWatch, and any others): replace direct `ctx.raw_body` access with `ctx.materialize_body()` or `ctx.raw_body.as_deref()`
- [x] 3.10 Fix CloudWatch `raw_body.starts_with(b"Action=")` check: use `ctx.spooled_body.peek_bytes(7)?` instead
- [x] 3.11 Update all test files that construct `RequestContext` with `raw_body: Bytes` to use `raw_body: None` (or `Some(bytes)` where the test explicitly needs the raw bytes populated)
- [x] 3.12 Run `cargo build --all` — zero compile errors
- [x] 3.13 Run `cargo test --all` — all tests green
- [x] 3.14 Run `cargo clippy --all -- -D warnings` — zero warnings

## 4. Phase 4 — Wire S3 PutObject/UploadPart to Streaming Write

- [x] 4.1 In `crates/services/s3/src/provider.rs` `handle_put_object()`: replace `ctx.raw_body.to_vec()` with `object_store.write_object_from_reader(HashingReader::new(spooled.into_reader()))` and derive ETag from `hashing_reader.finalize()`
- [x] 4.2 In `crates/services/s3/src/provider.rs` `handle_upload_part()`: apply the same streaming write + incremental MD5 pattern
- [x] 4.3 Update `crates/services/s3/src/store.rs` `put_object_version()` call sites to accept the ETag from the hashing reader rather than computing it from a `Vec<u8>`
- [x] 4.4 Ensure `ObjectFileStore::write_object_from_reader()` returns the byte count so `ObjectVersion.size` is set correctly without a stat call
- [x] 4.5 Write a unit test in `s3_tests.rs` for PutObject with `spooled_body = Some(SpooledBody::from_bytes(data))` and `raw_body = None`: verify the object is written, ETag is correct, and `raw_body` is never materialized
- [x] 4.6 Write a parameterized unit test for bodies crossing the spool threshold (e.g., 100 B inline, 2 MiB spilled to disk): both should produce correct ETags and stored content
- [x] 4.7 Run `cargo test -p s3` — all tests green

## 5. Phase 5 — Integration Tests and Benchmark Validation

- [x] 5.1 Add an integration test in `smoke_tests.rs`: upload 1 KB, 256 KB, 1 MB, 10 MB, 50 MB objects via HTTP; verify content round-trips correctly for each
- [x] 5.2 Add a concurrent upload integration test: 10 parallel PutObject requests to the same key, all succeed, final GetObject returns one correct body
- [x] 5.3 Verify `smoke_s3_large_object_streaming` (existing 2 MiB test) still passes
- [x] 5.4 Run parity tests: `cargo test -p integration-tests parity` — all scenarios pass
- [x] 5.5 Run `cargo fmt --all --check` — no formatting issues
- [x] 5.6 Run the local benchmark: `bash tests/bench/bench_services.sh` targeting openstack; save results
- [x] 5.7 Run the gate: `bash tests/bench/bench_gate.sh --report <output>` — gate SHALL pass (zero errors, p95 ≤ 5 ms, loaded RSS ≤ 10 MB)
- [x] 5.8 If any gate check fails, investigate and fix before pushing
