## Why

The current benchmark system is a ~3000-line Rust integration test module (`crates/tests/integration/src/benchmark.rs`) with 10 profiles, classification systems, role coverage tracking, and envelope validation. While thorough, it is heavy to compile, tightly coupled to the Rust test harness, opaque to casual contributors, and impossible to run without a full Rust toolchain. The supporting Python gating/reporting pipeline adds another ~2000 lines of complexity across 4 scripts.

A shell-based benchmark script would provide transparency (anyone can read the HTTP calls), portability (runs anywhere with bash + an HTTP bench tool), fast iteration (edit a line, re-run), and dual-mode support (Docker for fair comparison, bare binary for showcasing Rust performance). This was identified as a priority in PR #6 review feedback.

## What Changes

- **BREAKING**: Remove the Rust benchmark engine (`crates/tests/integration/src/benchmark.rs`, `benchmark_runner.rs` binary, `tests/benchmark/scenarios/*.json`)
- **BREAKING**: Remove the existing shell bench scripts (`tests/bench/bench_startup.sh`, `tests/bench/bench_memory.sh`) — their functionality is absorbed into the new script
- **BREAKING**: Replace the Python reporting/gating pipeline (`scripts/benchmark_report_tables.py`, `scripts/benchmark_regression_gate.py`, `scripts/benchmark_report_consolidated.py`, `scripts/benchmark_progress_dashboard.py`) with a simplified gate/reporting approach
- Add a comprehensive shell benchmark script (`tests/bench/bench_services.sh`) covering all 24 services with ~3-4 curated operations each (~75 total operations)
- Add profile support: `smoke` (8 core services, light load for PRs), `standard` (all 24, medium load for PRs to main), `deep` (all 24, heavy load for nightly/scheduled runs)
- Add dual runtime mode: Docker containers (default, fair comparison against LocalStack) and bare binary (`--binary` flag, openstack-only, showcases native performance)
- Add a simplified benchmark gate script for CI pass/fail decisions
- Rewrite CI workflow benchmark jobs to invoke the new shell script with appropriate profiles
- Remove CI requirement for prior benchmark baseline runs — the current gate fails when no previous run exists, which blocks new branches and fresh repos. The new gate should operate standalone (compare openstack vs LocalStack within the same run) without needing historical baselines.
- Report raw per-operation metrics (p50, p95, p99, throughput, error count) instead of weighted averages — the current system computes weighted cross-service aggregates that obscure individual service performance. The new output shows raw numbers per operation for clarity and actionability.
- Uses `oha` as primary HTTP benchmarking tool with `hey` as fallback

## Capabilities

### New Capabilities
- `shell-benchmark-engine`: The core shell-based benchmark script with profile support, service coverage for all 24 services, dual runtime modes (Docker/binary), memory measurement, and structured JSON output
- `shell-benchmark-gate`: Simplified regression gate and reporting that reads the new JSON output format and produces CI-friendly pass/fail verdicts with markdown summaries. Reports raw per-operation metrics only — no weighted averages or cross-service aggregation.

### Modified Capabilities
- `benchmark-harness`: Requirement changes from Rust-based harness to shell-based harness. Profile system changes from 10 Rust-defined profiles to 3 shell profiles (smoke/standard/deep). Execution model changes from in-process native HTTP to external HTTP benchmarking tool (oha/hey).
- `benchmark-regression-gate`: Gate input format changes from Rust engine JSON schema to new simpler JSON schema. Gate implementation changes from Python to shell. Threshold concept is preserved but simplified. Baseline-required behavior removed — gate evaluates within-run comparisons only, no dependency on prior run artifacts.
- `benchmark-runtime-envelope`: Memory and startup measurement approach changes from Rust harness instrumentation to shell-native Docker stats / process inspection. Envelope metrics preserved but collection method changes.
- `benchmark-service-workload-matrix`: Workload definitions move from Rust code (`default_read_write_commands_for_service`) to explicit shell script sections with raw HTTP calls per service. Role coverage concept (write/read) preserved but enforced differently.

## Impact

- **Removed code**: ~3000 lines Rust benchmark module, ~200 lines benchmark runner binary, ~2000 lines Python scripts, ~300 lines existing shell scripts, 6 JSON scenario files
- **Added code**: ~800-1000 line shell benchmark script, ~200 line shell gate script
- **CI workflows**: `.github/workflows/ci.yml` benchmark jobs rewritten, `.github/workflows/benchmark-deep.yml` rewritten
- **Dependencies**: New runtime dependency on `oha` (preferred) or `hey` (fallback) HTTP benchmarking tool — must be available in CI runner and documented for local use
- **Report format**: JSON output schema changes — any downstream consumers of benchmark reports need updating
- **Documentation**: `docs/act-benchmark-validation.md`, `docs/benchmark-optimization-backlog.md` need updating to reflect new tooling
