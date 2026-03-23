## Context

The current benchmark harness provides broad service coverage and strong openstack-vs-LocalStack comparability, but its all-services lanes are primarily composed of lightweight single-operation scenarios (often list/probe style). This creates a signal gap: benchmark outputs are stable and broad, but they do not sufficiently represent realistic read/write usage patterns for each service. The result is weaker prioritization guidance for performance remediation and potential mismatch with externally observed benchmark behavior.

This change spans benchmark scenario modeling, profile semantics, signal-quality validation, and reporting. It must preserve CI practicality while raising realism for every LocalStack-covered service currently represented by `all_service_names()`.

Constraints:
- Required CI lanes must remain deterministic and budget-aware.
- Scenarios must be valid against both openstack and LocalStack, or explicitly excluded with machine-readable reasons.
- Coverage requirements must apply to every supported service, not only hot-path subsets.
- Existing benchmark artifacts and regression gate outputs should remain backward-compatible where feasible.

Stakeholders:
- Platform engineers maintaining benchmark and parity lanes
- Service owners using benchmark reports for optimization work
- CI/release maintainers depending on stable gate behavior

## Goals / Non-Goals

**Goals:**
- Define and enforce a benchmark contract where every supported service has measured write and measured read scenarios.
- Introduce a service workload matrix abstraction that maps each service to required scenario categories, setup/cleanup behavior, and exclusion policy.
- Expand profile strategy to separate fast required lanes from deeper realism lanes while preserving all-service write/read contract coverage.
- Add runtime envelope measurements (startup timing and memory snapshots) as benchmark report metadata for compare-style analysis.
- Strengthen signal-quality and gate checks so missing realistic coverage causes explicit failure in required lanes.
- Keep benchmark execution reproducible with deterministic resource naming, setup, and cleanup.

**Non-Goals:**
- Replacing the benchmark harness with a different load generator framework.
- Matching every compare.sh implementation detail one-to-one (for example, exact CLI utilities or output formatting).
- Expanding product service surface beyond services already supported by openstack and benchmark harness.
- Introducing benchmark scenarios that rely on non-deterministic external systems.

## Decisions

### Decision 1: Introduce a service workload matrix as the source of truth
Create a structured service workload matrix that defines, for each supported service:
- required measured write category
- required measured read category
- deterministic setup and cleanup expectations
- allowed exclusion reasons (if any)

Rationale:
- Prevents drift between desired coverage and actual scenario files.
- Enables compile-time/test-time validation and report diagnostics.
- Makes service coverage requirements explicit and reviewable.

Alternatives considered:
- Implicit convention via scenario naming only: rejected due to weak validation and high drift risk.
- Manual docs-only matrix: rejected because gate enforcement requires machine-readable metadata.

### Decision 2: Represent write/read realism as explicit scenario categories
Extend benchmark scenario metadata with scenario role/category labels (for example `write`, `read`, optional `admin`/`aux`) and enforce at least one valid measured write and one valid measured read per service in required lanes.

Rationale:
- Current `scenario_class` (`coverage`/`performance`) is insufficient to express write/read completeness.
- Category-aware validation allows strict enforcement without overfitting to service-specific operation names.

Alternatives considered:
- Infer category from command verbs (`put/create/list/get`): rejected due to ambiguity across services and protocols.

### Decision 3: Keep profile lanes but tighten required-lane completeness semantics
Retain multi-lane strategy, but require realistic write/read completeness for all services in required broad lanes. Heavier per-service workloads remain in deeper/non-blocking lanes.

Rationale:
- Preserves CI runtime budgets while enforcing realism contract in gating lanes.
- Avoids forcing maximum load in every required lane.

Alternatives considered:
- One monolithic lane with all deep workloads: rejected due to runtime/noise risks.

### Decision 4: Add runtime envelope collection to benchmark reports
Collect startup timing and memory snapshots as first-class benchmark metadata:
- startup timing (cold start, repeated samples)
- idle memory snapshot
- post-load memory snapshot

Rationale:
- Aligns benchmark outputs with compare-style operational insights.
- Provides complementary signal to per-operation latency/throughput metrics.

Alternatives considered:
- Keep runtime envelope in ad hoc scripts only: rejected because results would not be normalized in benchmark artifacts.

### Decision 5: Service-specific scenario packs with deterministic lifecycle handling
For each service, define scenario packs that include:
- setup steps that establish valid resource state
- measured write/read operation steps
- cleanup steps that remove or reset resources
- optional readiness/wait polling where eventual consistency applies

Rationale:
- Realistic scenarios require lifecycle context and cannot rely on isolated probes.
- Deterministic lifecycle handling improves validity and reduces flakes.

Alternatives considered:
- Reuse parity probes as measured benchmark operations: rejected due to low realism and false signal.

### Decision 6: Explicit exclusion model, treated as debt in required lanes
If a service cannot immediately satisfy realistic write/read contract, it must declare a machine-readable exclusion reason tied to service + scenario role; required lanes treat unresolved exclusions as invalid for gate purposes.

Rationale:
- Makes gaps visible and actionable.
- Prevents silent regression to incomplete coverage.

Alternatives considered:
- Skip unsupported scenarios silently: rejected because it weakens gate trust.

### Decision 7: Backward-compatible report evolution
Extend report JSON with additional fields rather than breaking existing keys:
- per-service realistic coverage diagnostics
- scenario role/category details
- runtime envelope summary

Rationale:
- Preserves downstream consumers and historical tooling.

Alternatives considered:
- New report format version only: deferred unless compatibility burden grows.

## Risks / Trade-offs

- **[Risk] Increased benchmark runtime and CI cost** -> Mitigation: tune required lane iteration counts; keep deep workloads separate; enforce profile budgets.
- **[Risk] Service-specific scenario flakes due to async state transitions** -> Mitigation: deterministic polling/wait windows, idempotent setup/cleanup, bounded retries.
- **[Risk] Cross-target behavior mismatches reduce valid scenario count initially** -> Mitigation: explicit exclusion ledger with reason codes and remediation ownership.
- **[Risk] More complex benchmark configuration/reporting** -> Mitigation: centralized matrix model, strict metadata schema, and docs updates.
- **[Risk] Overly strict gate causes temporary delivery friction** -> Mitigation: phased rollout with baseline seeding and visibility-first period before hard enforcement.

## Migration Plan

1. Define and land service workload matrix model and scenario category metadata.
2. Add realistic write/read scenario packs for all services and wire to all-services profiles.
3. Implement validation enforcing per-service write/read completeness and invalid reason reporting.
4. Add runtime envelope collection and emit in reports.
5. Update benchmark docs and profile guidance.
6. Roll out gate enforcement in phases:
   - Phase A: diagnostics-only visibility
   - Phase B: strict required-lane enforcement

Rollback strategy:
- Gate strictness can be reduced to diagnostics-only mode if instability emerges.
- New metadata fields are additive; existing report consumers remain functional.
- Scenario pack changes can be reverted per service without dismantling matrix infrastructure.

## Open Questions

- Should required lanes enforce exactly one write + one read per service, or allow multiple with minimum cardinality constraints?
- For inherently non-resource-oriented services (notably STS), what operation classification should count as realistic write-equivalent?
- Should runtime envelope metrics participate in regression gates initially, or remain informational until baseline variance is characterized?
- What is the acceptable maximum runtime budget for required lanes after all-service realistic scenario expansion?
- Do we need profile-level toggles to disable select heavy setup operations in local developer runs while preserving CI strictness?
