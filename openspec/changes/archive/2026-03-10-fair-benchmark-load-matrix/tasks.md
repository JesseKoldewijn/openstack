## 1. Fairness runtime foundation

- [x] 1.1 Add benchmark runtime mode that starts both openstack and LocalStack as Docker containers with identical CPU/memory/network constraints.
- [x] 1.2 Add runtime preflight validation that verifies both targets are started with matching constraints before benchmarks begin.
- [x] 1.3 Add execution-order policy support (openstack-first, localstack-first, alternating) and persist selected policy in run metadata.

## 2. Scenario model and load-tier expansion

- [x] 2.1 Extend benchmark scenario schema with `scenario_class` (`coverage` or `performance`) and `load_tier` (`low`/`medium`/`high`/`extreme`).
- [x] 2.2 Split existing all-services scenarios into coverage probes and performance scenarios with explicit classification.
- [x] 2.3 Define tiered workload parameters per service (iteration counts, operation counts, concurrency, payload/record sizes).

## 3. S3 heavy-object benchmark coverage

- [x] 3.1 Add S3 performance scenarios for 1 GB object put/get validation and cleanup in heavy-object tiers.
- [x] 3.2 Add S3 performance scenarios for 5 GB object put/get validation and cleanup in heavy-object tiers.
- [x] 3.3 Add S3 performance scenarios for 10 GB object put/get validation and cleanup in heavy-object tiers.
- [x] 3.4 Implement environment guards that skip heavy-object scenarios with explicit skip reasons when runtime requirements are not met.

## 4. Reporting and analysis updates

- [x] 4.1 Extend benchmark report schema to include runtime fairness metadata, scenario class, load tier, and skip reasons.
- [x] 4.2 Update summary calculations so comparative latency/throughput rollups include only performance scenarios.
- [x] 4.3 Update report table scripts and docs to display tiered results and clearly separate coverage versus performance outputs.
- [x] 4.4 Add per-service benchmark summary metrics (p95/p99/throughput ratios, error counts, skipped counts) to benchmark JSON output.
- [x] 4.5 Include per-service comparison tables in benchmark markdown reports and consolidated workflow summaries.

## 5. CI integration and validation

- [x] 5.1 Update CI workflows to run low/medium fairness tiers on routine runs and high/extreme tiers on scheduled runs.
- [x] 5.2 Add benchmark tests/assertions that verify heavy-object scenario definitions exist for 1 GB, 5 GB, and 10 GB.
- [x] 5.3 Execute baseline fairness runs against both targets and publish artifacts for trend tracking and follow-up optimization analysis.
- [x] 5.4 Expand fair low/medium benchmark scenarios to include performance-grade operations for all currently benchmarked services.
