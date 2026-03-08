# CI workflow optimization guide

This document captures baseline measurements, optimization decisions, and rollback guidance for GitHub Actions workflows.

## 1. Workflow inventory and dependency map

### Active workflows

- `CI` (`.github/workflows/ci.yml`)
- `Docker` (`.github/workflows/docker.yml`)
- `Cross-compile` (`.github/workflows/cross-compile.yml`)

### CI workflow jobs and dependencies

- `fmt`: no dependencies
- `clippy`: no dependencies
- `test` (matrix: `ubuntu-latest`, `macos-latest`): no dependencies
- `build`: no dependencies (intentionally decoupled to run in parallel)
- `verify-build-artifact`: depends on `build`
- `required-checks`: depends on `fmt`, `clippy`, `test`, `verify-build-artifact`

### Required status checks recommendation

Use only the aggregate `Required checks` job as branch protection target. This keeps branch protection stable while allowing internal job graph changes.

## 2. Baseline performance snapshot (pre-optimization)

Data source: GitHub Actions REST API (`actions/runs` and `actions/jobs`) on 2026-03-06.

### CI workflow

- Run: `https://github.com/JesseKoldewijn/openstack/actions/runs/22773818878`
- End-to-end duration: ~167s (17:10:33 -> 17:13:20)

Per-job durations from the run:

- `Rustfmt`: ~17s
- `Clippy`: ~28s
- `Test (ubuntu-latest)`: ~46s
- `Test (macos-latest)`: ~53s
- `Build (release)`: ~108s

Observed critical path (before optimization):

`max(fmt, clippy, test matrix)` -> `build`

Approximate critical path length:

- `max(parallel pre-build jobs)` = ~53s
- `build` = ~108s
- Total ~= ~161s (+ scheduling overhead)

### Cross-compile workflow

- Run: `https://github.com/JesseKoldewijn/openstack/actions/runs/22773818875`
- End-to-end duration: ~106s

Per-job durations:

- `Build x86_64-unknown-linux-gnu`: ~103s
- `Build aarch64-unknown-linux-gnu`: ~101s

Critical path is matrix max duration (~103s) with small scheduler overhead.

### Docker workflow

- Recent runs are still in progress in the sampled window, so no stable complete baseline is included yet.
- Action item: capture first completed run after optimization and append here.

## 3. Optimization decisions implemented

- Added workflow-level concurrency cancellation:
  - `ci-${{ github.ref }}`
  - `docker-${{ github.ref }}`
  - `cross-compile-${{ github.ref }}`
- Added deterministic `rust-cache` keys bound to lockfile hash and job purpose.
- Enabled explicit matrix controls (`fail-fast: false`, `max-parallel: 2`) where applicable.
- Refactored CI dependency graph so `build` runs in parallel with lint/tests.
- Added artifact verification gate:
  - `verify-build-artifact` downloads and smoke-checks release binary.
- Added aggregate required-check gate:
  - `required-checks` depends on the required upstream jobs.
- Added selective execution filters for `Docker` and `Cross-compile` workflows using `paths`.

## 4. Cache and artifact strategy

- Cache keys include `hashFiles('**/Cargo.lock')` to align cache invalidation with dependency changes.
- Distinct `shared-key` per job class (`clippy`, `test`, `build-release`) to reduce cross-job cache churn.
- Release binary is published by `build` and consumed by `verify-build-artifact` to validate artifact integrity without rebuilding.

## 5. Validation checklist

- [ ] Confirm `Required checks` appears as a successful status on PRs.
- [ ] Confirm branch protection references only `Required checks`.
- [ ] Confirm `build` starts immediately (no longer waits for lint/tests).
- [ ] Confirm `verify-build-artifact` runs after `build` and fails on invalid artifact.
- [ ] Confirm `Docker` and `Cross-compile` do not trigger for docs-only or non-runtime changes.
- [ ] Compare CI run durations before/after over at least 10 runs.

## 6. Runtime comparison template (post-optimization)

Fill after enough runs are available:

- Sample window: `<date range>`
- Baseline median CI duration: `<seconds>`
- New median CI duration: `<seconds>`
- Baseline p95 CI duration: `<seconds>`
- New p95 CI duration: `<seconds>`
- Relative improvement (median): `<percent>`
- Relative improvement (p95): `<percent>`

## 7. Rollback plan

If regressions or instability are observed:

1. Revert `.github/workflows/ci.yml`, `.github/workflows/docker.yml`, `.github/workflows/cross-compile.yml` to previous revision.
2. Keep branch protection intact by preserving a required status job (either old required job set or aggregate gate).
3. Re-run CI on a representative PR and verify all mandatory checks pass.
4. Re-introduce optimizations incrementally (concurrency first, then graph changes, then path filters).

## 8. Guardrails

- Keep a single aggregate required check (`Required checks`) as the branch protection contract.
- Avoid adding `needs` dependencies unless correctness requires it.
- Tune matrix `max-parallel` according to runner capacity and queue behavior.
- Keep cache keys deterministic and tied to dependency/toolchain inputs.
- Revisit path filters when adding new build contexts or workflow responsibilities.
