## Why

ECR and EventBridge are fully implemented and unit tested, but their parity and benchmark coverage are skeletal: each service has exactly one single-operation scenario that only verifies failure/error behavior, and neither satisfies the existing `benchmark-service-workload-matrix` requirement of at least one write and one read operation per service. This leaves two complete services invisible to both parity regressions and benchmark comparisons.

## What Changes

- Replace the ECR all-services-smoke parity probe with a full lifecycle scenario covering repository creation, image push, list, batch retrieval, and deletion, validating that each step produces LocalStack-equivalent responses.
- Replace the EventBridge all-services-smoke parity probe with a full lifecycle scenario covering event bus creation, rule creation, target attachment, rule state toggling, target removal, and cleanup.
- Expand the ECR benchmark section to add a seed step (create repository + push image) followed by write (`CreateRepository` with dynamic names, `PutImage`) and read (`DescribeRepositories`, `ListImages`, `BatchGetImage`) operations.
- Expand the EventBridge benchmark section to add a seed step (create bus + rule + targets) followed by write (`CreateEventBus` with dynamic names, `PutRule`, `PutTargets`) and read (`ListEventBuses`, `ListRules`, `DescribeRule`, `ListTargetsByRule`) operations.
- Retain existing smoke probes for the `DescribeImages` 501 stub (ECR) and `PutEvents` 501 stub (EventBridge) as assertions within the lifecycle scenarios so stub parity remains visible.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `parity-harness`: ECR and EventBridge all-services-smoke scenarios must cover happy-path lifecycle, not just expected-failure probes. The existing requirement for dual-target scenario execution with equivalent inputs applies to the full CRUD surface, not just error paths.
- `benchmark-service-workload-matrix`: ECR and EventBridge sections currently violate the existing requirement for at least one write and one read operation per service. Both need seed operations and a proper write+read benchmark matrix.

## Impact

- Affected files: `tests/parity/scenarios/all-services-smoke.json` (replace ecr-probe and events-probe scenarios), `tests/bench/bench_services.sh` (expand ECR and EventBridge sections).
- No Rust code changes required — all changes are in test data and the benchmark shell script.
- No new services, crates, or binary targets.
- Risk: if LocalStack Free responds differently to ECR lifecycle operations than openstack, new parity mismatches will surface — this is the intended outcome, not a regression.
