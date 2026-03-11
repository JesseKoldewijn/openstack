## 1. Benchmark signal-quality hardening

- [x] 1.1 Add benchmark report quality fields (`valid_performance_scenarios`, `invalid_performance_scenarios`, `lane_interpretable`, `invalid_reasons`) and ensure they are emitted for each run.
- [x] 1.2 Exclude invalid scenarios from aggregate optimization metrics and clearly report exclusions in markdown outputs.
- [x] 1.3 Add guard checks that fail CI benchmark interpretation when a required lane has no valid performance scenarios.

## 2. Benchmark gate baseline and diagnostics hardening

- [x] 2.1 Update benchmark-gate baseline lookup to emit machine-readable diagnostics (lookup source, run IDs checked, artifact name, failure reason).
- [x] 2.2 Add explicit `GH_TOKEN` prerequisite handling and remediation messaging for CI and local usage.
- [x] 2.3 Add deterministic failure classes for baseline-missing vs token-missing vs data-quality failure.

## 3. All-service remediation planning matrix

- [x] 3.1 Create a service remediation matrix covering every supported service with baseline operations, hotspot hypotheses, and target gains.
- [x] 3.2 Define platform-loop optimization backlog items (gateway/protocol/state/framework) with measurable acceptance targets.
- [x] 3.3 Define service-loop execution order with wave-based prioritization and parity-risk checks.

## 4. Implementation tracks for performance dimensions

- [x] 4.1 Add latency-focused instrumentation and profiling hooks needed to identify p95/p99 bottlenecks in shared and service paths.
- [x] 4.2 Add throughput-focused load investigation tasks and identify contention/serialization bottlenecks.
- [x] 4.3 Add memory-use profiling tasks (allocation hotspots, peak RSS tracking) and optimization targets.
- [x] 4.4 Add binary-size optimization tasks (dependency/features pruning, size budget checks) and acceptance criteria.

## 5. CI and local workflow validation with act

- [x] 5.1 Add documented `act` commands to run benchmark and benchmark-gate jobs locally with required env variables.
- [x] 5.2 Validate one passing local gate path with `act` and capture evidence/output artifacts.
- [x] 5.3 Validate one intentional failing local gate path with `act` and capture evidence/output artifacts.
- [x] 5.4 Verify CI benchmark comments/workflow summaries include per-service comparison plus average metrics and gate diagnostics.

## 7. Wave 1 benchmark validity and regression safety

- [x] 7.1 Diagnose and resolve benchmark runner AWS CLI path/availability issues that caused invalid all-failed scenario outcomes in CI/local runs.
- [x] 7.2 Re-run fairness benchmark lanes after runner fix and validate gate behavior against prior baselines.
- [x] 7.3 Publish updated consolidated benchmark report capturing lane validity ratios (`valid/performance`) and gate verdicts.

## 8. Required-lane core parity hardening

- [x] 8.1 Introduce core required benchmark profiles (`fair-low-core`, `fair-medium-core`) that prioritize cross-target-valid service operations.
- [x] 8.2 Update CI required benchmark/gate jobs to use core lanes and core-lane gate artifacts.
- [x] 8.3 Validate core lanes locally with benchmark and gate checks and regenerate consolidated reporting.

## 6. Verification and documentation

- [x] 6.1 Add tests for benchmark signal-quality classification and exclusion behavior.
- [x] 6.2 Add tests for benchmark-gate baseline/token diagnostics and failure classification.
- [x] 6.3 Update benchmark docs with troubleshooting for baseline lookup, `GH_TOKEN`, and `act` simulation workflows.
- [x] 6.4 Produce a remediation progress report template (per service and overall) for weekly tracking.
