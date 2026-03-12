## Why

The benchmark harness currently provides broad service coverage, but many all-services scenarios are lightweight single-operation probes that do not represent realistic service usage patterns. We need a benchmark model that measures end-to-end write and read behavior for every LocalStack-covered service so performance comparisons between openstack and LocalStack better reflect production-like usage and optimization priorities.

## What Changes

- Introduce an explicit all-services realistic benchmark contract requiring at least one measured write/mutate scenario and one measured read/query/list/describe scenario per supported service.
- Add service-specific benchmark scenario packs for all currently supported services with deterministic setup, wait, and cleanup behavior.
- Define a benchmark coverage matrix and validation rules so profiles fail fast when any service is missing required write/read coverage.
- Expand benchmark reporting to include contract compliance diagnostics (coverage completeness, exclusions, invalid reasons) per service and per profile.
- Add optional benchmark runtime envelope measurements (startup timing and idle/post-load memory) aligned with compare-style benchmarking, while keeping PR lanes stable and predictable.
- Add profile strategy updates that separate required broad CI lanes from heavier realism/deep lanes to preserve signal quality and runtime budgets.
- Update regression-gate policy to ensure required lanes evaluate only valid realistic scenarios and surface missing coverage as a first-class failure mode.

## Capabilities

### New Capabilities
- `benchmark-service-workload-matrix`: Defines the required write/read benchmark contract for every supported service, including scenario taxonomy, setup/cleanup expectations, and exclusion semantics.
- `benchmark-runtime-envelope`: Defines startup and memory envelope measurements (cold start, idle RSS, post-load RSS/container memory) and reporting requirements for benchmark runs.

### Modified Capabilities
- `benchmark-harness`: Extend profile and scenario requirements from broad probe coverage to realistic write+read service workloads across all supported services.
- `benchmark-signal-quality`: Tighten validity criteria so required lanes fail when service-level realistic scenario coverage is missing or invalid.
- `benchmark-regression-gate`: Ensure gate logic incorporates realistic scenario completeness and does not treat missing write/read coverage as a passing condition.

## Impact

- Affected code:
  - `crates/tests/integration/src/benchmark.rs` (scenario generation, profile selection, validation, reporting)
  - `tests/benchmark/scenarios/*.json` (all-services and deep profile scenario definitions)
  - `tests/benchmark/README.md` and benchmark documentation
  - benchmark gate/report scripts where required for new diagnostics
- Affected systems:
  - CI benchmark lanes and artifact interpretation
  - local developer benchmark workflows
- Dependencies/runtime:
  - No new product runtime dependencies expected
  - Potential benchmark execution time increase mitigated through profile/lane partitioning and tuned iteration defaults
