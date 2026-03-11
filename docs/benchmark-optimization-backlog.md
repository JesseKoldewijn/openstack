# Benchmark Optimization Backlog

This backlog is seeded by initial benchmark harness priorities and should be refined as baseline reports accumulate.

## Candidate Hotspots

- S3 high-concurrency `put-object` path: profile payload handling and request-body buffering.
- SQS burst `send-message`: inspect request parsing, queue URL normalization, and serialization overhead.
- DynamoDB hot-key `get-item`: evaluate table metadata lookup path and cache effectiveness.

## Follow-up Work

- Establish baseline snapshots from `all-services-smoke` and `hot-path-deep` runs in CI artifacts.
- Add trend scripts for p50/p95 latency and throughput ratio drift detection.
- Define advisory thresholds before introducing any benchmark gating.
