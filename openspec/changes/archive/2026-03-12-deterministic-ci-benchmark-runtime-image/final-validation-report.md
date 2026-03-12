# Deterministic CI Runtime Image Validation Report

## Scope

Validated deterministic OpenStack runtime image producer/consumer flow for benchmark and parity lanes, including `ci.yml` and `benchmark-deep.yml` updates.

## Deterministic runtime image contract

- Producer job: `prepare-openstack-runtime-image`
- Consumer fields:
  - `PARITY_OPENSTACK_IMAGE`
  - `PARITY_OPENSTACK_IMAGE_ID`
- Artifact handoff (GitHub Actions): `openstack-runtime-image-<run_id>` tar archive

## Local act validation

### Run 1: `benchmark-smoke-fast`
- Command: `act pull_request -W .github/workflows/ci.yml -e .act.pull_request.non_main.json -j benchmark-smoke-fast`
- Result: **failed** (expected benchmark runtime health failure persists)
- Evidence:
  - Producer succeeded and emitted image ref/id.
  - Consumer used `openstack-runtime-ci:1-fac5278bf137c976cea0b137b0fdf0aabcdcd3b5`.
  - Expected and actual image IDs matched: `sha256:1ce1f490439769021c904c5506ed08baa97a1695efe91e644555549614ef4afb`.
  - Failure remained benchmark target health timeout, not image provenance mismatch.

### Run 2: `parity-all-services-fast`
- Command: `act pull_request -W .github/workflows/ci.yml -e .act.pull_request.non_main.json -j parity-all-services-fast`
- Result: **failed** (parity runtime health timeout)
- Evidence:
  - Producer succeeded and emitted same image ref/id contract.
  - Consumer used same image ref and matching expected/actual image ID.
  - Failure was localstack health timeout, not provenance mismatch.

### Run 3: `benchmark-deep`
- Command: `act workflow_dispatch -W .github/workflows/benchmark-deep.yml -j benchmark-deep`
- Result: **failed** (benchmark high profile health timeout)
- Evidence:
  - Producer succeeded and emitted deterministic deep-run image ref/id.
  - Consumer used the same image ref and matching expected/actual image ID.
  - Failure remained benchmark runtime health timeout, not producer/consumer drift.

## Hosted GitHub CI validation

- Status: **completed** (hosted evidence captured).
- Run:
  - Run ID: `22975794792`
  - URL: `https://github.com/JesseKoldewijn/openstack/actions/runs/22975794792`
  - Branch/PR context: `feat/perf-improvements-v2` / PR #2 (non-main lane)

### Producer evidence (`Prepare OpenStack runtime image`)

- `image_ref="openstack-runtime-ci:22975794792-312e951a01a8c2463dd33866e9f1aea38656d460"`
- `image_artifact=openstack-runtime-image-22975794792`
- `image_id="sha256:0a5d0de5d91d92fea110aa122377a78f77eab5bc2231d767f33f3cb15d097db6"`
- Artifact upload name confirmed: `openstack-runtime-image-22975794792`

### Consumer provenance evidence

- `Benchmark (all-services-smoke fast)`
  - `Using OpenStack runtime image: openstack-runtime-ci:22975794792-312e951a01a8c2463dd33866e9f1aea38656d460`
  - `Expected image id: sha256:0a5d0de5d91d92fea110aa122377a78f77eab5bc2231d767f33f3cb15d097db6`
  - `Actual image id:   sha256:0a5d0de5d91d92fea110aa122377a78f77eab5bc2231d767f33f3cb15d097db6`
- `Parity (core)`
  - `Using OpenStack runtime image: openstack-runtime-ci:22975794792-312e951a01a8c2463dd33866e9f1aea38656d460`
  - `Expected image id: sha256:0a5d0de5d91d92fea110aa122377a78f77eab5bc2231d767f33f3cb15d097db6`
  - `Actual image id:   sha256:0a5d0de5d91d92fea110aa122377a78f77eab5bc2231d767f33f3cb15d097db6`
- `Parity (all-services-smoke fast)`
  - Runtime image provenance step executed with same ref/id contract; no mismatch emitted before profile execution.

### Hosted failure signatures (non-provenance)

- `Benchmark (all-services-smoke fast)` failed in benchmark execution, not image integrity:
  - `Error: timed out waiting for benchmark target health at http://127.0.0.1:39041/_localstack/health (target=openstack, attempts=405, last_error=error sending request for url (http://127.0.0.1:39041/_localstack/health))`
  - Container diagnostic in same failure path:
    - `status=exited health=unhealthy ... ExitCode=0 ... recent_logs=[<empty>]`
- `Parity (core)` failed in profile execution, not image integrity:
  - `Error: timed out waiting for localstack health at http://127.0.0.1:37097/_localstack/health`
- `Parity (all-services-smoke fast)` failed in profile execution, not image integrity:
  - `Error: timed out waiting for localstack health at http://127.0.0.1:34665/_localstack/health`

### Hosted job outcome snapshot

- Success: `Prepare OpenStack runtime image`, `Rustfmt`, `Clippy`, `Test (ubuntu/macos)`, `Build (release)`, `Verify release artifact`, `Validate Harness Coverage`, `PR Results Comment`.
- Failure: `Parity (core)`, `Parity (all-services-smoke fast)`, `Benchmark (all-services-smoke fast)`.
- Skipped (expected for this non-main failing run): `Benchmark Gate (main lane)`, `Benchmark Gate (non-main lane)`, `Required checks (main target)`, `Required checks (non-main target)`, plus non-applicable full/push parity+benchmark lanes.

## Compatibility and rollback

- Compatibility target: benchmark gates and required-check aggregators continue to behave as before when upstream benchmark/parity results pass/fail.
- Hosted non-main run `22975794792` compatibility observation:
  - `Benchmark Gate (non-main lane)` was skipped because `benchmark-smoke-fast` failed upstream, preserving existing dependency semantics via `needs`.
  - `Required checks (non-main target)` was skipped due failed/blocked upstream dependencies, consistent with prior aggregator behavior.
  - `PR Results Comment` still executed (`if: always()`), confirming result-reporting lane remained resilient while required-check lanes respected hard dependencies.
- Rollback path: revert producer-consumer wiring and restore prior runtime image source while retaining benchmark timeout diagnostics where possible.

## Pass/Fail Matrix (this session)

- `prepare-openstack-runtime-image` (`ci.yml` via act): **pass**
- `benchmark-smoke-fast` (`ci.yml` via act): **fail** (health timeout; provenance checks pass)
- `parity-all-services-fast` (`ci.yml` via act): **fail** (health timeout; provenance checks pass)
- `prepare-openstack-runtime-image` (`benchmark-deep.yml` via act): **pass**
- `benchmark-deep` (`benchmark-deep.yml` via act): **fail** (health timeout; provenance checks pass)
- `Prepare OpenStack runtime image` (hosted CI run `22975794792`): **pass**
- `Parity (core)` (hosted CI run `22975794792`): **fail** (localstack health timeout; provenance checks pass)
- `Parity (all-services-smoke fast)` (hosted CI run `22975794792`): **fail** (localstack health timeout; provenance checks pass)
- `Benchmark (all-services-smoke fast)` (hosted CI run `22975794792`): **fail** (benchmark target/localstack health timeout; provenance checks pass)
