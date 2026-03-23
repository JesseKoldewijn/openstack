## 1. Benchmark mode foundations

- [x] 1.1 Add benchmark domain models (profiles, scenarios, run config, per-target metrics, comparative report schema) in integration test harness code.
- [x] 1.2 Add benchmark runner entrypoint alongside parity runner, with CLI arguments for profile selection and output path.
- [x] 1.3 Reuse/adapt dual-target lifecycle management so benchmark mode can start/stop openstack and LocalStack consistently.

## 2. Workload execution and measurement

- [x] 2.1 Implement benchmark step execution loop with warmup iterations excluded from measurement.
- [x] 2.2 Implement measured iterations with configurable operation count and concurrency applied identically to both targets.
- [x] 2.3 Compute per-target metrics (latency p50/p95, throughput, operation count, error count) and attach run metadata.
- [x] 2.4 Compute openstack-vs-localstack comparative deltas/ratios per scenario and aggregate summary metrics.

## 3. Profiles and scenario coverage

- [x] 3.1 Define `all-services-smoke` benchmark profile with at least one representative scenario per benchmarked service.
- [x] 3.2 Define `hot-path-deep` benchmark profile with heavier workloads for high-impact services (including S3, SQS, DynamoDB).
- [x] 3.3 Add benchmark scenario configuration files and templating/placeholders for repeatable per-run resource naming.

## 4. Reporting, docs, and CI integration

- [x] 4.1 Write benchmark JSON reports to disk with profile, target metadata, per-scenario metrics, and aggregate summaries.
- [x] 4.2 Add benchmark usage documentation (local execution, profile intent, output interpretation, fairness caveats).
- [x] 4.3 Add CI workflow integration to run smoke benchmarks and publish report artifacts.
- [x] 4.4 Add optional scheduled deep benchmark workflow (non-blocking initially) to build historical baselines.

## 5. Validation and stabilization

- [x] 5.1 Add tests for benchmark metrics/statistics calculations and report serialization.
- [x] 5.2 Run benchmark profiles against both targets, validate output completeness, and capture initial baseline artifacts.
- [x] 5.3 Document follow-up optimization backlog seeded by benchmark hotspots discovered in baseline runs.
