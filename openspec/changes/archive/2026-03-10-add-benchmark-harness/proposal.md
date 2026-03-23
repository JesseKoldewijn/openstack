## Why

Runtime performance is currently a top project concern, but we do not have a consistent, repeatable way to quantify improvements or regressions across services. We need a benchmark harness that compares openstack and LocalStack under equivalent workloads so optimization work can be prioritized by data and validated before release.

## What Changes

- Add a benchmark harness mode that executes standardized performance scenarios against both openstack and LocalStack and emits machine-readable benchmark reports.
- Add all-services benchmark coverage using tiered profiles (broad smoke coverage for all enabled services, plus deep workloads for high-impact services like S3, SQS, and DynamoDB).
- Add benchmark metrics and comparison outputs including latency percentiles, throughput, error rates, warmup handling, and openstack-vs-localstack deltas.
- Add fairness and reproducibility controls (identical scenario inputs, controlled concurrency, repeat runs, and environment metadata capture) to reduce misleading benchmark conclusions.
- Add CI-friendly execution and artifact publishing for scheduled and on-demand benchmark runs.

## Capabilities

### New Capabilities
- `benchmark-harness`: Dual-target performance benchmarking across openstack and LocalStack with standardized scenarios, profiles, metrics collection, and comparative reporting.

### Modified Capabilities
- None.

## Impact

- Affected code: integration test harness and parity-adjacent orchestration under `crates/tests/integration`, benchmark scenario definitions under `tests/parity` or a dedicated benchmark directory, and reporting outputs under `target/*-reports`.
- Tooling/runtime: relies on AWS CLI, Docker (when LocalStack is managed automatically), and stable local/CI runtime environments for reproducibility.
- CI/process: introduces new benchmark workflows and report artifacts used to guide performance optimization decisions.
