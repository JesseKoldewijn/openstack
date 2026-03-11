## Context

OpenStack already has fairness-mode dual-target benchmarking and regression gates, but measured gains versus LocalStack remain too small for a Rust implementation and benchmark runs have shown signal-quality issues (for example high error counts and baseline lookup failures). This suggests we need a cross-service remediation plan that treats performance as a first-class product surface and validates measurement quality before optimization decisions.

The scope covers all services in the workspace, benchmark harness quality, CI regression gating reliability, memory behavior, and binary-size pressure. The objective is not one-off tuning but a repeatable optimization program with per-service plans and enforceable validation.

## Goals / Non-Goals

**Goals:**
- Establish a service-by-service performance remediation plan for every supported service (latency, throughput, memory, binary size).
- Introduce benchmark signal-quality checks so failed/invalid data does not drive optimization or gate outcomes.
- Strengthen benchmark-gate baseline resolution and diagnostics, including explicit GitHub CLI token requirements in CI.
- Add local workflow validation using `act` for benchmark and gate workflows before relying on GitHub runs.
- Define acceptance criteria and verification evidence for each service optimization track.

**Non-Goals:**
- Guaranteeing absolute performance parity or superiority for every operation in one iteration.
- Replacing existing parity harness semantics.
- Introducing new external benchmark platforms beyond current CI + local tooling in this change.

## Decisions

### 1) Use a two-loop optimization model: platform loop + service loop
We will optimize in two coordinated loops:
- **Platform loop:** gateway/protocol/state/framework/reporting/runtime overhead reductions that affect all services.
- **Service loop:** service-specific hotspot remediation plans with explicit targets and validation.

**Why:** Cross-cutting overhead can hide service improvements; service-only tuning misses shared bottlenecks.

**Alternatives considered:**
- Service-only optimization: too fragmented and likely to plateau.
- Platform-only optimization: misses protocol/business-path hotspots unique to each service.

### 2) Treat benchmark quality as a hard prerequisite to optimization conclusions
Benchmark runs with invalid signal (high error-rate scenarios, skipped-only lanes, missing baselines) must be called out and excluded from optimization decision-making.

**Why:** Invalid measurement causes false optimization priorities.

**Alternatives considered:**
- Continue using mixed-quality runs with manual interpretation: too error-prone.

### 3) Require explicit baseline-discovery diagnostics in benchmark gate behavior
Benchmark gate output must explicitly report baseline discovery path and auth prerequisites (`GH_TOKEN`) and provide deterministic remediation steps.

**Why:** Current baseline-missing failures are ambiguous and can appear as if current artifacts are lost.

**Alternatives considered:**
- Keep strict fail without diagnostics: maintains correctness but hurts operability.

### 4) Add mandatory local workflow validation using `act`
For benchmark and gate workflow changes, local validation via `act` is required before merge, including explicit token propagation checks.

**Why:** Shortens feedback loops and catches workflow integration faults earlier.

**Alternatives considered:**
- CI-only validation: slower and harder to debug iteration-to-iteration.

### 5) Define per-service remediation artifacts and acceptance contracts
Each service gets a plan template with:
- operation baseline set,
- bottleneck hypotheses,
- optimization actions,
- expected gain band,
- verification evidence,
- parity-risk checks.

**Why:** Prevents uneven optimization coverage and creates accountable progress across all services.

## Risks / Trade-offs

- [Risk] Program complexity across many services may slow delivery -> Mitigation: prioritize shared-platform bottlenecks first and phase service tracks by impact.
- [Risk] Stricter quality gates may increase short-term CI failures -> Mitigation: improve diagnostics and baseline seeding workflow.
- [Risk] `act` divergence from GitHub-hosted runners -> Mitigation: treat `act` as preflight, keep final CI truth in GitHub runs.
- [Risk] Binary-size optimization can conflict with ergonomics/feature coverage -> Mitigation: use explicit size budgets and justified exceptions.

## Migration Plan

1. Define and land benchmark signal-quality rules and reporting fields.
2. Update benchmark-gate diagnostics and baseline lookup behavior.
3. Add `act` validation playbook and CI simulation commands.
4. Create per-service remediation task matrix and prioritize execution order.
5. Run iterative optimization cycles (platform loop then service loop), validating parity + perf + memory + size.
6. Report weekly progress with per-service scoreboard and regression trend summaries.

## Open Questions

- Should binary-size budgets be global-only or also per-feature/service binary targets?
- Which services should be first-wave optimization targets after platform-loop fixes (S3/SQS/DDB likely)?
- Should benchmark gate policy support an explicit bootstrap mode for new branches with no historical baseline?
