## Context

The current benchmark harness compares openstack and LocalStack but currently runs targets with different runtime models (openstack in-process harness and LocalStack in Docker), while many all-services scenarios are probe-style commands that are useful for compatibility coverage but weak for performance truth. The benchmark path also relies on repeated AWS CLI invocation, which introduces client-side overhead that can obscure server-side differences.

This change introduces a fairness-first design: equivalent container runtime for both targets, explicit load tiers per service, and clear separation between coverage probes and performance scenarios. It also adds heavy-object S3 validation under benchmark conditions for 1 GB, 5 GB, and 10 GB objects.

## Goals / Non-Goals

**Goals:**
- Ensure benchmark comparisons run in symmetric runtime conditions (containerized openstack and containerized LocalStack with matched CPU/memory limits).
- Define low/medium/high/extreme load tiers so all benchmarked services are measured across a broad range.
- Separate performance scenarios from coverage probes in reporting and summary metrics.
- Add S3 large-object benchmark scenarios and explicit validation checks for 1 GB, 5 GB, and 10 GB handling.
- Capture fairness metadata in benchmark reports so reproducibility is auditable.

**Non-Goals:**
- Replacing the existing parity correctness workflow or known-differences model.
- Introducing benchmark pass/fail gates for CI in this change.
- Modeling every possible service-specific workload shape in the first iteration.

## Decisions

### 1) Symmetric containerized runtime for both targets
Run both openstack and LocalStack as Docker containers with identical resource constraints (CPU quota/cores, memory limit, network mode policy) and benchmark them from a separate runner process/container.

**Why:** Removes runtime asymmetry that can bias results.

**Alternatives considered:**
- Keep openstack in-process and LocalStack in Docker: simpler but not fair.
- Run both in-process: not realistic for LocalStack integration.

### 2) Load-tier matrix by service
Introduce tiered profiles (`low`, `medium`, `high`, `extreme`) with explicit per-service operation counts, concurrency bands, and payload/record sizes.

**Why:** Single-profile results hide behavior under different operating conditions.

**Alternatives considered:**
- One deep profile only: insufficient coverage and weak trend value.

### 3) Split coverage probes from performance scenarios
Tag scenarios as `coverage` or `performance`, and compute comparative performance summaries using only performance scenarios.

**Why:** Failure-expected probes should not influence throughput/latency comparisons.

**Alternatives considered:**
- Keep mixed reporting: simpler but misleading.

### 4) S3 heavy-object benchmark path with guarded execution
Add dedicated S3 scenarios for 1 GB, 5 GB, and 10 GB objects in the extreme tier, with configurable enablement for local vs CI contexts and explicit multipart strategy requirements where needed.

**Why:** Large object handling is a critical stress case and must be tested explicitly.

**Alternatives considered:**
- Test only up to sub-GB objects: misses key streaming/memory pressure behavior.

### 5) Fairness and reproducibility metadata in reports
Persist target image/tag, resource limits, runtime mode, run order policy, scenario class (`coverage`/`performance`), load tier, and large-object flags in JSON reports.

**Why:** Enables forensic analysis and reproducible comparisons.

## Risks / Trade-offs

- Increased benchmark runtime and infra cost, especially extreme tiers with 5 GB/10 GB objects -> Mitigation: split profiles by cadence (PR smoke vs scheduled deep/extreme).
- 10 GB object runs may be unstable in constrained CI runners -> Mitigation: make extreme large-object tests scheduled/non-blocking with documented machine requirements.
- Docker orchestration complexity for symmetric setup -> Mitigation: centralize runtime config and add startup validation checks before benchmark execution.
- Client overhead still present if AWS CLI remains in loop -> Mitigation: document this as a known limitation and track SDK-driver migration as follow-up.

## Migration Plan

1. Add new benchmark runtime mode that starts both targets in Docker with equal resource flags.
2. Introduce scenario schema updates for load tier and scenario class.
3. Build low/medium/high/extreme profiles with initial per-service mappings.
4. Add S3 1 GB, 5 GB, and 10 GB scenarios and environment guards.
5. Update report schema and table scripts to distinguish coverage vs performance metrics.
6. Wire CI workflows: smoke tiers for routine runs, deep/extreme for scheduled runs.
7. Validate with baseline runs and publish artifacts for trend tracking.

## Open Questions

- Should 10 GB tests run only on scheduled self-hosted runners, or also on GitHub-hosted runners with stricter timeouts?
- What is the minimum service-specific workload required to classify a scenario as performance-grade versus coverage-grade?
- Should fairness mode pin both containers to dedicated CPU sets for reduced jitter on shared runners?
