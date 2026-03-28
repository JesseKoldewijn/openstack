## 1. ECR Parity Lifecycle Scenario

- [x] 1.1 Add `ecr-lifecycle` scenario to `tests/parity/scenarios/all-services-smoke.json` with a setup step that calls `ecr create-repository` using `{{run_id}}` as a name suffix
- [x] 1.2 Add lifecycle steps to the `ecr-lifecycle` scenario: PutImage (with a minimal JSON manifest and fixed tag `bench-latest`), ListImages (assert `imageIds` non-empty), BatchGetImage (by tag `bench-latest`, assert `images` non-empty), DescribeRepositories (assert `repositories` non-empty)
- [x] 1.3 Add a cleanup step to `ecr-lifecycle` that calls `ecr delete-repository` for the run-scoped repository
- [x] 1.4 Verify `ecr-probe` (describe-images expecting failure) is still present alongside the new lifecycle scenario

## 2. EventBridge Parity Lifecycle Scenario

- [x] 2.1 Add `events-lifecycle` scenario to `tests/parity/scenarios/all-services-smoke.json` with a setup step that calls `events create-event-bus` using `{{run_id}}` as a name suffix
- [x] 2.2 Add lifecycle steps: PutRule (with a schedule expression on the seed bus, assert `RuleArn` present), DescribeRule (assert `State` is `ENABLED`), PutTargets (attach a dummy SQS ARN target, assert `FailedEntryCount` is 0), ListTargetsByRule (assert `Targets` non-empty), DisableRule, DescribeRule again (assert `State` is `DISABLED`), EnableRule, DescribeRule again (assert `State` is `ENABLED`), RemoveTargets, DeleteRule
- [x] 2.3 Add a cleanup step to `events-lifecycle` that calls `events delete-event-bus` for the run-scoped bus
- [x] 2.4 Verify `events-probe` (put-events expecting failure) is still present alongside the new lifecycle scenario

## 3. ECR Benchmark Expansion

- [x] 3.1 Add a seed block to the ECR section in `tests/bench/bench_services.sh` that calls `CreateRepository` (naming the repo `bench-ecr-<pid>`) and `PutImage` (pushing a minimal manifest with tag `bench-img-<pid>`) against all active targets using `seed_all_targets`
- [x] 3.2 Add a write benchmark using `bench_dynamic_targets`: `CreateRepository` with per-iteration unique names (`bench-ecr-create-<pid>-{i}`)
- [x] 3.3 Add read benchmarks using `bench_targets`: `DescribeRepositories` (empty body), `ListImages` (body `{"repositoryName":"bench-ecr-<pid>"}`), `BatchGetImage` (body with `repositoryName` and `imageIds:[{"imageTag":"bench-img-<pid>"}]`)
- [x] 3.4 Wrap the write and read benchmarks in a seed guard: call `skip_service "ecr"` with a reason message if the seed step fails for the required openstack target

## 4. EventBridge Benchmark Expansion

- [x] 4.1 Add a seed block to the EventBridge section in `tests/bench/bench_services.sh` that calls `CreateEventBus` (naming the bus `bench-bus-<pid>`), `PutRule` (naming the rule `bench-rule-<pid>` with a schedule expression targeting the seed bus), and `PutTargets` (attaching a dummy SQS ARN) against all active targets using `seed_all_targets`
- [x] 4.2 Add a write benchmark using `bench_dynamic_targets`: `PutRule` with per-iteration unique names (`bench-rule-create-<pid>-{i}`) on the seed bus
- [x] 4.3 Add read benchmarks using `bench_targets`: `ListEventBuses` (empty body), `ListRules` (body `{"EventBusName":"bench-bus-<pid>"}`), `DescribeRule` (body `{"Name":"bench-rule-<pid>"}`), `ListTargetsByRule` (body `{"Rule":"bench-rule-<pid>"}`)
- [x] 4.4 Wrap the write and read benchmarks in a seed guard: call `skip_service "eventbridge"` with a reason message if the seed step fails for the required openstack target

## 5. Validation

- [x] 5.1 Run `cargo run -p openstack-integration-tests --bin parity_runner -- --profile all-services-smoke` and confirm `ecr-lifecycle` passes for both targets (or surface any real parity mismatches)
- [x] 5.2 Run the same parity suite and confirm `events-lifecycle` passes for both targets (or surface any real parity mismatches)
- [x] 5.3 Run `./tests/bench/bench_services.sh --services ecr` and confirm the report includes `create_repository`, `describe_repositories`, `list_images`, and `batch_get_image` operations with non-zero metrics for all active targets
- [x] 5.4 Run `./tests/bench/bench_services.sh --services eventbridge` and confirm the report includes `put_rule`, `list_event_buses`, `list_rules`, `describe_rule`, and `list_targets_by_rule` operations with non-zero metrics for all active targets
- [x] 5.5 Confirm the existing ECR and EventBridge probe scenarios (`ecr-probe`, `events-probe`) still pass in the all-services-smoke run
