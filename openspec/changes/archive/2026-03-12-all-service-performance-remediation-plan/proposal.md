## Why

Current benchmark results are too close to LocalStack despite Rust implementation, indicating significant non-trivial performance bottlenecks in request processing, state access, and benchmark signal quality. We need a structured, service-by-service remediation plan that improves latency, throughput, memory use, and binary size while preserving functional parity.

## What Changes

- Define a cross-service performance remediation program with explicit optimization tracks for all currently supported services.
- Add benchmark signal-quality validation so failed/invalid scenarios do not mask true performance behavior.
- Add per-service optimization plans including baseline profile, bottleneck hypotheses, optimization actions, and acceptance targets.
- Add memory and binary-size optimization requirements and CI validation strategy.
- Add local CI simulation validation using `act` to reproduce benchmark and benchmark-gate behavior, including `GH_TOKEN` handling.

## Capabilities

### New Capabilities
- `service-performance-remediation`: Defines the required per-service optimization planning and validation workflow for latency/throughput/memory/binary-size improvements.
- `benchmark-signal-quality`: Defines benchmark-data quality requirements so performance decisions and gates are based on valid scenario outcomes.
- `ci-local-workflow-validation`: Defines required local workflow validation procedures (including `act`) for benchmark and benchmark-gate jobs.

### Modified Capabilities
- `benchmark-harness`: Extend requirements for all-service performance scenario reliability, quality guards, and richer optimization-target reporting.
- `benchmark-regression-gate`: Extend requirements to include robust baseline discovery behavior, explicit auth/token prerequisites, and deterministic failure diagnostics.

## Impact

- Affected benchmark/reporting scripts under `scripts/`.
- Affected CI workflows under `.github/workflows/` and local workflow validation docs/tooling.
- Affected integration benchmark harness under `crates/tests/integration/`.
- Affected service crates under `crates/services/*` through service-specific optimization work items.
