# Benchmark Harness

The benchmark harness compares runtime behavior between openstack and LocalStack using equivalent workloads.

## Profiles

- `all-services-smoke`: broad, lightweight coverage across representative scenarios for each benchmarked service.
- `all-services-smoke-fast`: budget lane profile used for non-`main` PR targets.
- `hot-path-deep`: higher-volume workloads for high-impact services (`s3`, `sqs`, `dynamodb`, `lambda`, `kinesis`, `opensearch`, `cloudwatch`) to surface hotspots.

## Run Locally

Requirements:

- `aws` CLI available
- Docker available (unless `PARITY_LOCALSTACK_ENDPOINT` is provided)

Smoke profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile all-services-smoke
```

Fast smoke profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile all-services-smoke-fast
```

Deep profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile hot-path-deep
```

Optional overrides:

- `PARITY_OPENSTACK_ENDPOINT=http://127.0.0.1:4566`
- `PARITY_LOCALSTACK_ENDPOINT=http://127.0.0.1:4666`
- `PARITY_LOCALSTACK_IMAGE=localstack/localstack:3.7.2`

Optional explicit output path:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile all-services-smoke --output target/benchmark-reports/manual.json
```

Reports are written to `target/benchmark-reports/*.json`.

## Interpreting Reports

- `results[*].openstack.metrics` and `results[*].localstack.metrics` include p50/p95/p99 latency, min/max/stddev, throughput (ops/s), operation count, error count, success rate, timeout count, retry count, and total duration.
- `results[*].comparison` includes openstack-vs-localstack deltas and ratios for latency and throughput.
- `summary` provides aggregate error totals and average ratios across all executed scenarios.

## Fairness Caveats

- Use the same profile and environment settings when comparing runs.
- Warmup iterations are excluded from measured metrics by design.
- Shared CI runners introduce noise; trend comparisons should prefer repeated runs or scheduled baselines.
