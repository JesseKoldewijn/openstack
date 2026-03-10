# Benchmark Harness

The benchmark harness compares runtime behavior between openstack and LocalStack using equivalent workloads.

## Profiles

- `fair-low`: low-load fairness lane with broad service coverage.
- `fair-medium`: medium-load fairness lane for routine PR runs.
- `fair-high`: high-load lane for scheduled deep profiling.
- `fair-extreme`: extreme S3 heavy-object lane (`1gb`, `5gb`, `10gb`) for non-blocking scheduled validation.

Legacy profile aliases still exist for compatibility:

- `all-services-smoke`
- `all-services-smoke-fast`
- `hot-path-deep`

## Run Locally

Requirements:

- `aws` CLI available
- Docker available (unless `PARITY_LOCALSTACK_ENDPOINT` is provided)

Fair low profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-low
```

Fair medium profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-medium
```

Fair high profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-high
```

Fair extreme profile (heavy S3 objects):

```bash
BENCHMARK_HEAVY_OBJECTS=1 cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-extreme
```

Optional overrides:

- `PARITY_OPENSTACK_ENDPOINT=http://127.0.0.1:4566`
- `PARITY_LOCALSTACK_ENDPOINT=http://127.0.0.1:4666`
- `PARITY_LOCALSTACK_IMAGE=localstack/localstack:3.7.2`
- `PARITY_OPENSTACK_IMAGE=ghcr.io/jessekoldewijn/openstack:latest`
- `PARITY_BENCHMARK_RUNTIME_MODE=symmetric-docker`
- `PARITY_BENCHMARK_EXECUTION_ORDER=alternating`
- `PARITY_DOCKER_CPU_LIMIT=2`
- `PARITY_DOCKER_MEMORY_LIMIT=4g`
- `PARITY_DOCKER_NETWORK_MODE=bridge`
- `BENCHMARK_HEAVY_OBJECTS=1`
- `BENCHMARK_LARGE_FILES_DIR=tests/benchmark/fixtures`

Optional explicit output path:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-medium --output target/benchmark-reports/manual.json
```

Reports are written to `target/benchmark-reports/*.json`.

## Regression Gate

Week 3+ CI policy enforces strict regression checks on required lanes:

- Non-main PR lane (`fair-low`): required gate
- Main-target PR lane (`fair-medium`): required gate
- Thresholds:
  - p95 ratio regression limit: +8%
  - p99 ratio regression limit: +12%
  - throughput ratio regression limit: -8%

Required-lane gate behavior:

- Missing baseline: fail
- No performance scenarios: fail
- All performance scenarios skipped: fail

Manual gate run example:

```bash
python3 scripts/benchmark_regression_gate.py \
  --lane fair-low \
  --current-glob "target/benchmark-reports/fair-low-*.json" \
  --previous "target/benchmark-reports/fair-low-baseline.json" \
  --strict-missing-baseline \
  --p95-limit 8 \
  --p99-limit 12 \
  --throughput-limit 8
```

To seed/recover baseline data for CI gating, run the lane successfully in CI and keep the benchmark artifact available (non-expired) for baseline lookup.

## Interpreting Reports

- `results[*].openstack.metrics` and `results[*].localstack.metrics` include p50/p95/p99 latency, min/max/stddev, throughput (ops/s), operation count, error count, success rate, timeout count, retry count, and total duration.
- `results[*].scenario_class` identifies `coverage` vs `performance` scenarios.
- `results[*].load_tier` identifies `low`, `medium`, `high`, or `extreme` load levels.
- `results[*].skipped` and `results[*].skip_reason` indicate environment-gated scenarios (for example heavy-object runs without fixtures).
- `results[*].comparison` includes openstack-vs-localstack deltas and ratios for latency and throughput.
- `summary` provides aggregate error totals, scenario class counts, skipped count, and average ratios across performance (non-skipped) scenarios only.
- `summary.per_service` provides per-service scenario counts, skipped counts, and average p95/p99/throughput ratios for openstack-vs-localstack comparison.
- `scripts/benchmark_report_consolidated.py` can generate a single consolidated markdown report across fairness lanes, including optional gate verdicts (`--include-gate`).

## Fairness Caveats

- Use `PARITY_BENCHMARK_RUNTIME_MODE=symmetric-docker` for fair target runtime symmetry.
- Use the same profile and environment settings when comparing runs.
- Warmup iterations are excluded from measured metrics by design.
- Shared CI runners introduce noise; trend comparisons should prefer repeated runs or scheduled baselines.
