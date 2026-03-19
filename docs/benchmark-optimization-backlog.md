# Benchmark Optimization Backlog

This backlog is seeded by initial benchmark priorities and should be refined as baseline reports accumulate.

## Benchmark Tooling

Benchmarks are run via shell scripts using [oha](https://github.com/hatoo/oha) as the HTTP load generator:

- **`tests/bench/bench_services.sh`** — runs all 24 AWS service benchmarks against both OpenStack and LocalStack, outputs a JSON report with per-operation latency percentiles, throughput, errors, and memory snapshots.
- **`tests/bench/bench_gate.sh`** — evaluates a benchmark report against configurable thresholds (p95 latency ratio, memory budget, error rate) and produces a markdown summary.

Profiles: `smoke` (fast CI), `standard` (main-branch PRs), `deep` (scheduled nightly).

## Candidate Hotspots

- S3 high-concurrency `put-object` path: profile payload handling and request-body buffering.
- SQS burst `send-message`: inspect request parsing, queue URL normalization, and serialization overhead.
- DynamoDB hot-key `get-item`: evaluate table metadata lookup path and cache effectiveness.

## Follow-up Work

- Establish baseline snapshots from `standard` and `deep` profile runs in CI artifacts.
- Add trend analysis for p50/p95 latency and throughput ratio drift detection across runs.
- Tune gate thresholds (`--p95-threshold`, `--memory-budget`) as baseline data accumulates.
