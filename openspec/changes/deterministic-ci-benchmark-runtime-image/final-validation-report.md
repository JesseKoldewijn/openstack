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

- Status: **pending** (requires pushing workflow changes and observing GitHub-hosted run evidence).
- Required evidence to capture:
  - run URL(s)
  - producer output image ref/id
  - matching consumer preflight image ref/id across benchmark and parity jobs

## Compatibility and rollback

- Compatibility target: benchmark gates and required-check aggregators continue to behave as before when upstream benchmark/parity results pass/fail.
- Rollback path: revert producer-consumer wiring and restore prior runtime image source while retaining benchmark timeout diagnostics where possible.

## Pass/Fail Matrix (this session)

- `prepare-openstack-runtime-image` (`ci.yml` via act): **pass**
- `benchmark-smoke-fast` (`ci.yml` via act): **fail** (health timeout; provenance checks pass)
- `parity-all-services-fast` (`ci.yml` via act): **fail** (health timeout; provenance checks pass)
- `prepare-openstack-runtime-image` (`benchmark-deep.yml` via act): **pass**
- `benchmark-deep` (`benchmark-deep.yml` via act): **fail** (health timeout; provenance checks pass)
