## Why

The current benchmark flow mixes coverage probes with performance measurement and runs openstack and LocalStack in asymmetric environments, which can distort comparative results. We need a fairness-first benchmark model that measures both low and high loads across all supported services, with explicit large-object S3 tests (1 GB, 5 GB, 10 GB) to validate heavy payload behavior.

## What Changes

- Introduce a fairness benchmark model that runs openstack and LocalStack in equivalent containerized environments with matched resource constraints.
- Expand benchmark profiles into load tiers (low, medium, high, extreme) so each service can be evaluated across a broad operating range.
- Separate coverage/probe scenarios from performance scenarios so failure-expected probes do not pollute performance metrics.
- Add explicit S3 large-object benchmark scenarios and validation tests for 1 GB, 5 GB, and 10 GB object handling.
- Add reporting fields that capture fairness metadata (container image, resource limits, execution order policy, and load tier).

## Capabilities

### New Capabilities
- `benchmark-fairness-runtime`: Defines equivalent container runtime and execution-order controls for dual-target benchmarks.

### Modified Capabilities
- `benchmark-harness`: Extend requirements to support tiered all-service load coverage, fairness metadata, and explicit large S3 object benchmark scenarios.

## Impact

- Affected benchmark harness code under `crates/tests/integration/src/` and benchmark scenario definitions under `tests/benchmark/scenarios/`.
- CI benchmark workflows in `.github/workflows/` will require updates to run fairness-mode benchmarks and publish richer artifacts.
- Additional container orchestration and runtime configuration will be needed for both targets.
- Benchmark report schema will expand to include fairness and load-tier metadata.
