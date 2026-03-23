## 1. Fix oha invocation (critical bug)

- [x] 1.1 Replace `--json` with `--output-format json` in the `bench()` function in `tests/bench/bench_services.sh`
- [x] 1.2 Fix p50 extraction path from `.responseTimeHistogram.percentiles."50"` to `.latencyPercentiles.p50` in `bench()`
- [x] 1.3 Fix p95 extraction path from `.responseTimeHistogram.percentiles."95"` to `.latencyPercentiles.p95` in `bench()`
- [x] 1.4 Fix p99 extraction path from `.responseTimeHistogram.percentiles."99"` to `.latencyPercentiles.p99` in `bench()`

## 2. Add --targets flag to bench_services.sh

- [x] 2.1 Add `TARGETS` variable with default value `os,ls,moto` and `--targets` CLI flag parsing
- [x] 2.2 Add validation that `os` is always present in `TARGETS`, exit with error if missing
- [x] 2.3 Add `--moto-image` CLI flag with default `motoserver/moto:latest`

## 3. Moto container lifecycle in bench_services.sh

- [x] 3.1 Add `MOTO_PORT=5555` and `MOTO_BASE=http://localhost:5555` variables
- [x] 3.2 Add moto container start function with CPU/memory limits matching openstack/LocalStack, conditional on `moto` in `TARGETS`
- [x] 3.3 Add moto health check polling `GET http://localhost:5555/moto-api/` with 30-second timeout
- [x] 3.4 Add moto to the cleanup/teardown function, conditional on `moto` in `TARGETS`

## 4. Rename bench_both to bench_targets

- [x] 4.1 Rename `bench_both()` to `bench_targets()` with new signature adding `moto_url` parameter
- [x] 4.2 Add conditional execution inside `bench_targets()` checking `TARGETS` for each target (os/ls/moto)
- [x] 4.3 Add `"moto"` as a valid target in the `bench()` function's target field mapping

## 5. Update all per-service benchmark sections

- [x] 5.1 Update all `bench_both` calls to `bench_targets` calls with added moto URL parameter (all 24 services)
- [x] 5.2 Define `MOTO_BASE` URL for each per-service section using path-style URLs

## 6. Moto memory collection

- [x] 6.1 Add moto idle memory collection (before benchmarks) conditional on `moto` in `TARGETS`
- [x] 6.2 Add moto loaded memory collection (after benchmarks) conditional on `moto` in `TARGETS`
- [x] 6.3 Add moto memory fields to JSON report `memory` section

## 7. JSON report schema extension

- [x] 7.1 Add `moto` object to per-operation results in the JSON report builder
- [x] 7.2 Ensure JSON report omits moto fields when moto is not in active targets

## 8. Update bench_gate.sh for three-target evaluation

- [x] 8.1 Add moto p95, moto RPS, and OS/Moto ratio columns to the markdown summary table header
- [x] 8.2 Add moto p95 extraction from JSON report for each operation row
- [x] 8.3 Compute `os_p95 / moto_p95` ratio for each operation when moto data is present
- [x] 8.4 Fail gate if either OS/LS or OS/Moto ratio exceeds `--p95-threshold`
- [x] 8.5 Add moto memory row to the markdown summary
- [x] 8.6 Handle missing moto data gracefully (skip moto columns/ratios when moto was not an active target)

## 9. CI workflow updates

- [x] 9.1 Add moto Docker image environment variable to `.github/workflows/ci.yml` benchmark jobs
- [x] 9.2 Add moto image pull to CI benchmark job preflight steps in `ci.yml`
- [x] 9.3 Add `continue-on-error: true` to benchmark artifact download step in CI PR comment job in `ci.yml`
- [x] 9.4 Add moto Docker image environment variable to `.github/workflows/benchmark-deep.yml`
- [x] 9.5 Add moto image pull to benchmark-deep.yml preflight steps
- [x] 9.6 Delete `.github/workflows/semgrep.yml`

## 10. Verification

- [x] 10.1 Run `shellcheck tests/bench/bench_services.sh` and fix any warnings
- [x] 10.2 Run `shellcheck tests/bench/bench_gate.sh` and fix any warnings
- [x] 10.3 Verify `--targets os,ls` mode works (moto excluded) by dry-run inspection
- [x] 10.4 Verify JSON report schema is valid with moto fields present and absent
