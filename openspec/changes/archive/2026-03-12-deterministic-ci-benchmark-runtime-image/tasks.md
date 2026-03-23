## 1. Runtime image producer and immutable reference contract

- [x] 1.1 Add a dedicated runtime-image producer job to `.github/workflows/ci.yml` that builds OpenStack runtime image once per run and emits an immutable reference output.
- [x] 1.2 Add equivalent runtime-image producer flow to `.github/workflows/benchmark-deep.yml` and ensure it emits the same immutable-reference contract shape.
- [x] 1.3 Define and document the consumer contract (output/env variable names and expected immutable format) used by benchmark/parity jobs.

## 2. Rewire benchmark/parity consumers to deterministic runtime image

- [x] 2.1 Update benchmark jobs in `.github/workflows/ci.yml` (`benchmark-smoke-full`, `benchmark-smoke-fast`, `benchmark-smoke-push`) to consume the producer output instead of floating `PARITY_OPENSTACK_IMAGE` defaults.
- [x] 2.2 Update parity jobs that run OpenStack runtime comparisons to consume the same producer output for run-consistent image selection.
- [x] 2.3 Update `.github/workflows/benchmark-deep.yml` benchmark execution jobs to consume the producer output and remove floating-tag runtime dependency.

## 3. Provenance diagnostics and fail-fast runtime checks

- [x] 3.1 Add preflight diagnostics in benchmark/parity jobs to print selected immutable OpenStack image reference and inspect metadata before execution.
- [x] 3.2 Add fail-fast workflow checks that surface clear runtime-image integrity/provenance errors when startup/health cannot be established.
- [x] 3.3 Ensure benchmark failure output and summary artifacts retain enough provenance context to distinguish runtime image issues from benchmark logic regressions.

## 4. act and hosted CI validation coverage

- [x] 4.1 Update local validation docs (`tests/benchmark/README.md` and related CI validation docs) with deterministic runtime-image producer/consumer steps for `act`.
- [x] 4.2 Run local `act` validation for representative benchmark/parity jobs and capture evidence that one immutable runtime image reference is reused across tested lanes.
- [x] 4.3 Run hosted GitHub CI validation and capture run evidence that benchmark/parity jobs in a workflow run consume the same immutable runtime image reference.

## 5. Completion, compatibility, and rollback readiness

- [x] 5.1 Verify benchmark gates and required-check aggregators still behave correctly after producer-consumer wiring changes.
- [x] 5.2 Add/update rollback notes documenting how to temporarily revert to prior runtime image selection if producer flow is disrupted.
- [x] 5.3 Produce a final validation summary artifact documenting pass/fail matrix for `act` and hosted CI under deterministic runtime-image flow.
