## Why

Benchmark and parity lanes currently depend on a floating OpenStack runtime image (`PARITY_OPENSTACK_IMAGE=...:latest`), which can drift independently from code under test and cause cross-lane failures that are not tied to the PR. We need deterministic per-run runtime image selection so local `act` validation and GitHub CI runs execute against the same known-good artifact.

## What Changes

- Add a workflow-level mechanism to build the OpenStack runtime image once per CI run and expose a single immutable reference (digest-pinned tag/output) for all benchmark/parity jobs in that run.
- Replace direct `latest` consumption in benchmark/parity preflight and execution paths with the run-scoped immutable image reference.
- Add runtime image preflight diagnostics (selected reference, inspect metadata, startup verification) so failures clearly distinguish image integrity issues from benchmark logic regressions.
- Extend CI validation guidance to cover both local `act` simulation and hosted GitHub Actions verification for the same runtime-image flow.
- Preserve existing benchmark lanes and gates, but make their runtime dependency deterministic.

## Capabilities

### New Capabilities
- `ci-runtime-image-determinism`: Build once and reuse one immutable OpenStack runtime image reference across all benchmark/parity jobs in a CI run, with explicit diagnostics for provenance.

### Modified Capabilities
- `benchmark-harness`: Benchmark execution requirements change to consume deterministic run-scoped OpenStack runtime image references instead of floating defaults in CI paths.
- `ci-local-workflow-validation`: CI validation requirements expand to require coverage of the deterministic runtime-image path in both local `act` and GitHub-hosted runs.

## Impact

- Affected workflows: `.github/workflows/ci.yml`, `.github/workflows/benchmark-deep.yml`.
- Affected benchmark/parity runtime configuration and diagnostics in integration harness paths.
- Affected docs/playbooks for local CI simulation and hosted CI validation.
- No external API breaking changes; impact is on CI/runtime determinism, observability, and reliability.
