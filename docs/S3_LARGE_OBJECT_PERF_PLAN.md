# S3 Large Object Performance Optimization Plan

## Current State & Bottleneck Analysis
Initial benchmarking of `openstack` S3 implementation (Rust) on large objects (50MB - 100MB) shows degradations specifically in **PUT** operations under concurrency, and high memory pressure during **GET** if many large objects are requested simultaneously.

### Identified Bottlenecks
1.  **Gateway Spooling:** The current `Gateway` logic uses `SpooledBody` which defaults to full memory buffering for smaller objects but can still introduce overhead when transitioning to disk for larger ones.
2.  **S3 Storage I/O:** `ObjectFileStore` uses `tokio::io::copy_buf` with an adaptive `BufWriter`. While better than a naive copy, the buffer sizes (max 16MiB) and the interaction with the underlying filesystem (ext4/xfs) during high concurrency might lead to lock contention or I/O wait.
3.  **Sync vs Async Handlers:** The S3 provider currently uses `spawn_blocking` for some filesystem operations, but the overhead of context switching for large streams may be impacting throughput.
4.  **Protocol Overhead:** XML serialization/deserialization and checksum calculations (if any) are performed synchronously on the main thread for parts of the request metadata.

## Optimization Strategy (Tiered Approach)

### Phase 1: Storage Layer & I/O (The "Engine")
*   **Direct I/O & Pre-allocation:** Implement `fallocate` (via `nix` or `libc`) to pre-allocate the entire file size before writing. This prevents filesystem fragmentation which is a major killer for large file performance.
*   **Zero-Copy Streaming:** Evaluate `sendfile(2)` or `splice(2)` for GET operations to avoid copying data into user-space memory entirely.
*   **I/O Ring (Optional/Long-term):** Explore `tokio-uring` for high-throughput Linux-native I/O, though this may require significant architectural changes.

### Phase 2: Gateway & Body Handling (The "Pipeline")
*   **True Streaming PUT:** Refactor `RequestContext` and S3 provider to support `Stream<Item = Result<Bytes>>` directly from the network socket to the storage layer, bypassing `SpooledBody` entirely for S3-specific paths when `Content-Length > Threshold`.
*   **Backpressure Management:** Implement explicit flow control in the body stream to prevent the gateway from overwhelming the storage writer.

### Phase 3: Resource Management
*   **Jemalloc Configuration:** Tune memory allocator (jemalloc) for large allocation reuse to reduce fragmentation and allocation latency for 50MB+ buffers.
*   **Worker Pool Isolation:** Move S3 heavy I/O to a dedicated thread pool to prevent large file transfers from starving metadata-only requests (e.g., IAM, STS).

## Implementation Plan (Milestones)

| Milestone | Task | Success Metric |
| :--- | :--- | :--- |
| **M1: Baseline** | Create repeatable large-object benchmark suite in `tests/bench` | Consistent p95 data for 50MB/100MB |
| **M2: Storage** | Implement `fallocate` and optimized `BufWriter` scaling | PUT latency -15% |
| **M3: Streaming** | Implement zero-copy GET and bypass-spooling PUT | Memory RSS < 100MB during 100MB GET |
| **M4: Validation** | Verify parity with LocalStack/AWS for large multi-part uploads | 100% Pass in `s3_perf_tests.rs` |

## Next Steps (Immediate)
1.  Update `s3_perf_tests.rs` to include a 100MB concurrency test case.
2.  Implement the `fallocate` pre-allocation logic in `crates/services/s3/src/object_store.rs`.
3.  Benchmark the impact of `fallocate` alone.
