# Benchmark Harness

The benchmark harness compares runtime behavior between openstack and LocalStack using equivalent workloads.

## Profiles

- `fair-low`: low-load broad exploration lane (all services, lower validity expected during parity expansion).
- `fair-medium`: medium-load broad exploration lane (all services, lower validity expected during parity expansion).
- `fair-low-core`: low-load required-gate lane using cross-target-valid core services.
- `fair-medium-core`: medium-load required-gate lane using cross-target-valid core services.
- `fair-high`: high-load lane for scheduled deep profiling.
- `fair-extreme`: extreme S3 heavy-object lane (`1gb`, `5gb`, `10gb`) for non-blocking scheduled validation.
- Deep lanes (`fair-high`, `fair-extreme`) are marked as non-blocking profile runs in benchmark runtime metadata.

Legacy profile aliases still exist for compatibility:

- `all-services-smoke`
- `all-services-smoke-fast`
- `all-services-realistic`
- `hot-path-deep`

## Run Locally

Requirements:

- `aws` CLI available
- Docker available (unless `PARITY_LOCALSTACK_ENDPOINT` is provided)

Fair low core profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-low-core
```

Fair medium core profile:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-medium-core
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
- `PARITY_BENCHMARK_LANE_MODE=harness-influenced|low-overhead`
- `PARITY_BENCHMARK_EXECUTION_DRIVER=aws-cli|direct-http`
- `PARITY_DOCKER_CPU_LIMIT=2`
- `PARITY_DOCKER_MEMORY_LIMIT=4g`
- `PARITY_DOCKER_NETWORK_MODE=bridge`
- `BENCHMARK_HEAVY_OBJECTS=1`
- `BENCHMARK_LARGE_FILES_DIR=tests/benchmark/fixtures`
- `PARITY_OPENSTACK_PERSISTENCE_MODE=durable|non-durable`
- `PARITY_LOCALSTACK_PERSISTENCE_MODE=durable|non-durable`
- `PARITY_BENCHMARK_STARTUP_SAMPLES=3`
- `PARITY_BENCHMARK_ROLE_COVERAGE_DIAGNOSTICS_ONLY=1`
- `PARITY_BENCHMARK_ROLE_COVERAGE_STRICT=1`

Optional explicit output path:

```bash
cargo run -p openstack-integration-tests --bin benchmark_runner -- --profile fair-medium-core --output target/benchmark-reports/manual.json
```

Reports are written to `target/benchmark-reports/*.json` plus per-profile latest snapshots (`<profile>-latest.json`).

## Regression Gate

Week 3+ CI policy enforces strict regression checks on required lanes:

- Non-main PR lane (`fair-low-core`): required gate
- Main-target PR lane (`fair-medium-core`): required gate
- Thresholds:
  - p95 ratio regression limit: +8%
  - p99 ratio regression limit: +12%
  - throughput ratio regression limit: -8%

Required-lane gate behavior:

- Missing baseline: fail
- No performance scenarios: fail
- All performance scenarios skipped: fail
- Missing required service write/read role coverage: fail

Diagnostics-first rollout and rollback:

- Set `PARITY_BENCHMARK_ROLE_COVERAGE_DIAGNOSTICS_ONLY=1` and leave `PARITY_BENCHMARK_ROLE_COVERAGE_STRICT=0` to keep lane interpretability permissive while still reporting missing-role diagnostics.
- Enable strict enforcement with `PARITY_BENCHMARK_ROLE_COVERAGE_STRICT=1` once lane stability is acceptable.
- Roll back strict behavior by resetting `PARITY_BENCHMARK_ROLE_COVERAGE_STRICT=0` while keeping diagnostics mode enabled.

Manual gate run example:

```bash
python3 scripts/benchmark_regression_gate.py \
  --lane fair-low-core \
  --current-glob "target/benchmark-reports/fair-low-core-*.json" \
  --previous "target/benchmark-reports/fair-low-core-baseline.json" \
  --strict-missing-baseline \
  --p95-limit 8 \
  --p99-limit 12 \
  --throughput-limit 8
```

To seed/recover baseline data for CI gating, run the lane successfully in CI and keep the benchmark artifact available (non-expired) for baseline lookup.

### GH_TOKEN prerequisite

When baseline discovery is performed through `gh` CLI, `GH_TOKEN` must be set.

Example:

```bash
export GH_TOKEN=<github_token>
python3 scripts/benchmark_regression_gate.py \
  --lane fair-low-core \
  --current-glob "target/benchmark-reports/fair-low-core-*.json" \
  --repo "owner/repo" \
  --workflow-file "ci.yml" \
  --artifact-name "benchmark-smoke-fast-report" \
  --run-id "123456789" \
  --strict-missing-baseline
```

If `GH_TOKEN` is missing, gate diagnostics should report `missing_gh_token` explicitly.

## Local Workflow Simulation with act

Pre-requisites:

- `act` installed (`act --version`)
- Docker running
- `GH_TOKEN` exported

Suggested local simulation commands:

```bash
# Non-main PR benchmark lane (fair-low)
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-fast \
  --env GH_TOKEN="$GH_TOKEN"

# Main PR benchmark lane (fair-medium)
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-full \
  --env GH_TOKEN="$GH_TOKEN"

# Non-main PR parity lane (all-services fast)
act pull_request -W .github/workflows/ci.yml -j parity-all-services-fast \
  --env GH_TOKEN="$GH_TOKEN"
```

### Deterministic runtime-image contract

Benchmark/parity CI jobs now consume a run-scoped OpenStack runtime image produced once per workflow run.

Consumer contract fields:

- `PARITY_OPENSTACK_IMAGE`: run-scoped image tag loaded into the job runtime.
- `PARITY_OPENSTACK_IMAGE_ID`: immutable image ID expected by consumers.

Workflow behavior:

- A producer job builds OpenStack runtime image once and publishes a tar artifact for downstream jobs.
- Consumer jobs load the artifact, validate image-id provenance, and fail fast on mismatch.
- Floating `ghcr.io/...:latest` is no longer used in benchmark/parity execution lanes.

Validation evidence to capture:

- Producer output showing selected run-scoped image reference.
- Consumer preflight logs showing expected/actual image ID and inspect metadata.
- A representative benchmark lane and parity lane reusing the same image-id in one run.

For intentional failure-path validation, run the gate script against synthetic degraded reports and confirm non-zero exit plus failure diagnostics in output JSON/markdown.

See also: `docs/act-benchmark-validation.md` for the full local workflow simulation playbook.

## Interpreting Reports

- `results[*].openstack.metrics` and `results[*].localstack.metrics` include p50/p95/p99 latency, min/max/stddev, throughput (ops/s), operation count, error count, success rate, timeout count, retry count, and total duration.
- `results[*].scenario_class` identifies `coverage` vs `performance` scenarios.
- `results[*].scenario_role` identifies scenario intent (`write`, `read`, `admin`, `aux`).
- `results[*].load_tier` identifies `low`, `medium`, `high`, or `extreme` load levels.
- `results[*].skipped` and `results[*].skip_reason` indicate environment-gated scenarios (for example heavy-object runs without fixtures).
- `results[*].comparison` includes openstack-vs-localstack deltas and ratios for latency and throughput.
- `summary` provides aggregate error totals, scenario class counts, skipped count, and average ratios across performance (non-skipped) scenarios only.
- `summary.valid_performance_scenarios`, `summary.invalid_performance_scenarios`, `summary.lane_interpretable`, and `summary.invalid_reasons` provide benchmark signal-quality diagnostics.
- `summary.missing_required_role_count` provides lane-level count of missing required service role coverage.
- `summary.per_service` provides per-service execution class, durability class, scenario counts, skipped counts, average p95/p99/throughput ratios, and class-envelope breach diagnostics.
- `summary.per_service[*].required_roles`, `summary.per_service[*].covered_roles`, `summary.per_service[*].missing_roles`, and `summary.per_service[*].role_exclusions` provide realistic write/read coverage diagnostics.
- `runtime.openstack_persistence_mode`, `runtime.localstack_persistence_mode`, and `runtime.persistence_mode_equivalent` capture mode-equivalence metadata.
- `scripts/benchmark_report_consolidated.py` can generate a single consolidated markdown report across fairness lanes, including optional gate verdicts (`--include-gate`).

## Binary Size Budget

The release `openstack` binary is budgeted and checked in CI:

- Budget: `55 MB` (Linux release binary)
- Enforcement script: `scripts/check_release_binary_size.sh`

Manual check:

```bash
cargo build --release --bin openstack
./scripts/check_release_binary_size.sh target/release/openstack 55
```

## Fairness Caveats

- Use `PARITY_BENCHMARK_RUNTIME_MODE=symmetric-docker` for fair target runtime symmetry.
- Use the same profile and environment settings when comparing runs.
- Warmup iterations are excluded from measured metrics by design.
- Shared CI runners introduce noise; trend comparisons should prefer repeated runs or scheduled baselines.
- `PARITY_BENCHMARK_EXECUTION_DRIVER=aws-cli` includes client process overhead in every operation.
- `PARITY_BENCHMARK_EXECUTION_DRIVER=direct-http` removes AWS CLI process overhead and better isolates openstack-vs-localstack backend behavior for core list/call scenarios.

## Lane Modes

- `harness-influenced` (default): parity-friendly lane using existing harness behavior.
- `low-overhead`: lower benchmark-driver overhead lane for signal attribution; compare this lane against equivalent persistence/runtime settings only.
