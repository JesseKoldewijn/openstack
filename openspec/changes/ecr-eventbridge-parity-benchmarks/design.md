## Context

ECR and EventBridge are fully implemented at the service layer with comprehensive unit test coverage. The gap is at the integration layer: each service has exactly one parity scenario (a single-step failure probe) and one benchmark operation (a single read with no seeding). This means neither service contributes meaningful signal to parity or benchmark runs — regressions in repository/image semantics or rule/target semantics would be invisible.

The benchmark script currently violates the existing `benchmark-service-workload-matrix` requirement for ECR and EventBridge (at least one write + one read per service), and the all-services-smoke parity scenarios test only stub error behavior rather than the implemented CRUD surface.

Two files require changes: `tests/parity/scenarios/all-services-smoke.json` and `tests/bench/bench_services.sh`.

## Goals / Non-Goals

**Goals:**
- Add ECR and EventBridge lifecycle parity scenarios to all-services-smoke so regressions in implemented operations are caught on every PR
- Expand ECR and EventBridge benchmark sections with seeding and a write + read operation matrix, satisfying the existing `benchmark-service-workload-matrix` requirement
- Retain existing failure-probe scenarios so stub parity (DescribeImages 501, PutEvents 501) remains explicitly verified

**Non-Goals:**
- Implementing any new service operations — both services are complete as-is
- Changing the scope or service list of any parity profile
- Adding ECR or EventBridge to the core parity profile (smoke is sufficient for the current implementation maturity)
- Benchmarking `DescribeImages` or `PutEvents` (both are intentional 501 stubs)

## Decisions

### 1. Lifecycle scenarios are added to all-services-smoke, not a separate profile

The all-services-smoke profile is the authoritative 24-service smoke baseline. Adding lifecycle scenarios as additional entries (alongside the existing probes, not replacing them) gives immediate per-PR regression coverage without restructuring any profile.

**Alternative considered**: Add lifecycle scenarios to the `core` or `extended` profiles instead.
**Rationale for rejection**: Core requires coordinated spec changes and PR-gating implications; extended is non-gating and lower signal. All-services-smoke is already running on every PR and is the right home for scenarios that exercise implemented operations.

### 2. Probe scenarios are kept alongside lifecycle scenarios

The existing `ecr-probe` and `events-probe` scenarios are not replaced — they are retained as-is. New lifecycle scenarios (`ecr-lifecycle`, `events-lifecycle`) are added as additional entries.

**Alternative considered**: Merge lifecycle steps into the existing probe scenarios.
**Rationale for rejection**: The probes and lifecycle tests serve different purposes. Probes verify stub/error parity; lifecycle tests verify happy-path CRUD parity. Merging makes failure attribution harder.

### 3. ECR BatchGetImage uses a fixed seed tag, not dynamic digest capture

`BatchGetImage` requires either an `imageTag` or `imageDigest` lookup key. The seed step pushes an image with tag `bench-img-<pid>` so the benchmark operation can reference it by fixed tag without needing to capture a dynamic digest from the PutImage response.

**Alternative considered**: Capture the image digest from the PutImage seed response and use it in BatchGetImage.
**Rationale for rejection**: The benchmark script's `seed_all_targets` helper does not capture response values for later use. Using a fixed tag matches the SecretsManager pattern (seed with `bench-secret-<pid>`, read with the same name).

### 4. EventBridge DescribeRule uses a seeded rule with fixed name

The seed step creates a rule named `bench-rule-<pid>` with a schedule expression. `DescribeRule` in the benchmark targets this fixed name, avoiding dynamic name capture.

**Alternative considered**: Only benchmark list operations (ListRules, ListEventBuses) since they need no seed reference.
**Rationale for rejection**: DescribeRule is a key single-resource read that exercises a different code path than list operations. Seeding with a fixed name is well-established in the script.

### 5. PutEvents and DescribeImages are excluded from lifecycle scenarios and benchmarks

Both operations are intentional 501 stubs that mirror LocalStack Free tier behavior. They are already covered by the existing probe scenarios. Including them in lifecycle scenarios or benchmarks would only produce expected-failure results with no comparative value.

## Risks / Trade-offs

- **LocalStack Free ECR behavior divergence** → If LocalStack Free returns different responses for CreateRepository or PutImage than openstack, new parity mismatches will surface. This is the intended outcome. Differences that are non-semantic (e.g., field ordering, timestamp precision) can be registered in `known_differences.json` with rationale.

- **EventBridge default bus behavior** → openstack always synthesizes the `default` event bus in ListEventBuses even when no state exists. If LocalStack's behavior differs (e.g., requires explicit creation), the parity scenario may fail. The lifecycle scenario should use a named bus for explicit state control, not rely on the default bus.

- **Benchmark seed isolation** → Seeds use `<pid>` to namespace resources per run. Parallel benchmark executions on the same target could collide if two processes share a PID (unlikely but possible in containerized environments). Acceptable risk given existing patterns in the script.

- **No teardown in benchmark sections** → The ECR and EventBridge benchmark sections do not clean up seed resources after the run (consistent with most other services in the script). Containers are ephemeral per benchmark run, so this has no practical impact.
