## Why

The shell-based benchmark system (`bench_services.sh` + `bench_gate.sh`) deployed in the `shell-benchmark-replacement` change produces zero metrics in CI. The root cause is that `oha` is invoked with `--json` (which does not exist) instead of `--output-format json`, and the JSON field paths used to extract latency percentiles are wrong. Every metric reads as `0ms` / `0 RPS`, making the gate trivially pass with no real data. Additionally, the benchmark only compares openstack against LocalStack — adding moto as a third independent target provides a broader competitive picture and catches regressions that only appear relative to a pure-Python mock backend.

## What Changes

- **Fix oha invocation**: Replace `--json` with `--output-format json` and correct the JSON extraction paths from `responseTimeHistogram.percentiles."N"` to `latencyPercentiles.pN` in `bench_services.sh`.
- **Add moto as a third benchmark target**: Start a `motoserver/moto` Docker container alongside openstack and LocalStack, benchmark all services against all three targets, and include moto metrics in the JSON report.
- **Add `--targets` filtering**: Allow selecting which targets to benchmark (`os`, `ls`, `moto`) via a `--targets` CLI flag, defaulting to all three. Targets not selected are skipped entirely (no container startup, no bench calls).
- **Update bench_gate.sh**: Extend the gate to evaluate openstack p95 latency ratios against both LocalStack and moto. Extend the markdown summary table to show all three targets side-by-side with two ratio columns.
- **Harden CI artifact handling**: Add `continue-on-error: true` to the benchmark artifact download step in the PR comment job so missing artifacts don't kill the comment.
- **Remove Semgrep workflow**: Delete `.github/workflows/semgrep.yml` — CodeRabbit now handles Semgrep analysis.
- **Update CI workflows**: Add moto image pull to benchmark job preflight steps.

## Capabilities

### New Capabilities
- `benchmark-moto-target`: Moto as a third benchmark comparison target — container lifecycle, per-operation benchmarking, and gate evaluation against moto alongside LocalStack.

### Modified Capabilities
- `benchmark-harness`: The harness now supports three targets (openstack, LocalStack, moto) instead of two, with configurable target selection via `--targets`. The oha invocation and metric extraction are corrected.
- `benchmark-regression-gate`: The gate evaluates openstack performance against both LocalStack and moto, with per-target ratio columns in the markdown summary.

## Impact

- **`tests/bench/bench_services.sh`**: Major changes — oha flag fix, moto container lifecycle, `bench_all()` function, `--targets` flag, moto memory collection, JSON schema extension.
- **`tests/bench/bench_gate.sh`**: Moderate changes — moto columns in markdown, dual ratio evaluation, moto memory row.
- **`.github/workflows/ci.yml`**: Minor — moto image env var, preflight pull, `continue-on-error` on artifact download.
- **`.github/workflows/benchmark-deep.yml`**: Minor — moto image env var, preflight pull.
- **`.github/workflows/semgrep.yml`**: Deleted.
- **New Docker dependency**: `motoserver/moto:latest` pulled in CI alongside LocalStack.
