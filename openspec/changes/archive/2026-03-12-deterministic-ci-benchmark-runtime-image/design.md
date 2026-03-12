## Context

Benchmark and parity jobs currently depend on a floating OpenStack runtime image reference (`ghcr.io/...:latest`) configured at workflow scope. When that floating tag drifts or points to an invalid image, all benchmark lanes fail uniformly even when benchmark logic and PR code are correct. Recent failures show deterministic health-check timeouts where the OpenStack container exits immediately with `ExitCode=0`, indicating runtime image integrity/provenance concerns rather than workload regressions.

This change introduces deterministic runtime image selection per workflow run so `act` and GitHub-hosted CI evaluate benchmark/parity against the same run-scoped artifact reference.

## Goals / Non-Goals

**Goals:**
- Build OpenStack runtime image once per workflow run for benchmark/parity execution.
- Propagate one immutable runtime image reference to all benchmark/parity jobs in that run.
- Remove benchmark/parity dependency on floating `latest` tags in CI execution paths.
- Surface runtime image provenance diagnostics in preflight and failure output.
- Validate the flow in both local `act` and hosted GitHub Actions runs.

**Non-Goals:**
- Redesign benchmark scenario logic, fairness profiles, or gate thresholds.
- Replace existing docker publication workflows outside benchmark/parity runtime selection.
- Introduce new benchmark result formats unrelated to runtime-image provenance.

## Decisions

### Decision 1: Add a dedicated runtime-image producer job in CI workflows
Create a workflow job that builds the OpenStack runtime image once and emits an immutable reference for downstream jobs.

- Rationale: Establishes a single source of truth for runtime image selection per run.
- Alternatives considered:
  - Continue using floating `latest`: rejected due to drift/non-determinism.
  - Build independently in each consumer job: rejected due to inconsistency risk and duplicated cost.

### Decision 2: Use immutable references (digest-qualified) as consumer contract
Consumer jobs receive a resolved immutable image reference (digest-qualified image ref, or equivalent immutable artifact reference if environment constraints require it).

- Rationale: Guarantees all lanes consume identical image contents.
- Alternatives considered:
  - Mutable run tag only: rejected because tags can be overwritten.
  - Workflow env `PARITY_OPENSTACK_IMAGE=latest`: rejected due to non-determinism.

### Decision 3: Update benchmark/parity workflows to consume producer output
Benchmark and parity jobs in `ci.yml` and `benchmark-deep.yml` are wired to use the producer output instead of hardcoded floating defaults in workflow execution paths.

- Rationale: This is where failures manifest and where deterministic behavior matters.
- Alternatives considered:
  - Keep workflows unchanged and only modify harness defaults: rejected because CI env still overrides defaults.

### Decision 4: Add explicit provenance diagnostics and fail-fast checks
Before benchmark/parity execution, jobs print selected runtime reference and inspect metadata, and fail fast with actionable diagnostics if image startup/provenance checks fail.

- Rationale: Distinguishes image integrity incidents from harness/workload regressions quickly.
- Alternatives considered:
  - Rely on existing health timeout output only: rejected because it obscures source of failure.

### Decision 5: Require dual-path validation (act + hosted CI)
Validation artifacts and docs require evidence for both local `act` simulation and hosted CI runs using the deterministic image contract.

- Rationale: Prevents local-only confidence gaps and hosted-only regressions.
- Alternatives considered:
  - Validate hosted CI only: rejected due to slow debugging iteration and lower developer feedback loop quality.

## Risks / Trade-offs

- [Registry/auth constraints for immutable reference publication in some contexts] -> Mitigation: support fallback immutable artifact handoff where needed and document expected mode by environment.
- [Increased workflow complexity and dependency fan-in] -> Mitigation: keep producer contract minimal (single output reference) and isolate diagnostic logic into concise preflight steps.
- [Potential runtime-image build cost increase in CI] -> Mitigation: build once per run and reuse across all consumers; evaluate cache strategy to offset cost.
- [act environment differences from hosted runners] -> Mitigation: explicitly document expected differences and enforce common producer/consumer contract validation evidence.

## Migration Plan

1. Add runtime-image producer job to benchmark/parity workflows.
2. Expose immutable runtime image reference as job output.
3. Rewire benchmark/parity jobs to consume output reference and stop pulling floating `latest` in execution paths.
4. Add preflight provenance diagnostics and fail-fast checks in consumer jobs.
5. Update local/hosted validation documentation and evidence expectations.
6. Run `act` validation matrix for affected jobs and capture artifacts/log references.
7. Run hosted CI validation and confirm cross-lane runtime reference consistency.

Rollback strategy:
- Revert workflow wiring to previous runtime image configuration if producer-consumer wiring causes widespread blockage, while retaining diagnostics improvements where safe.

## Open Questions

- Should immutable reference transport default to digest-from-registry in all hosted runs, with artifact handoff only as fallback, or should artifact handoff be primary for PR workflows?
- Should parity-only lanes also be hard-required consumers of the same producer output in the initial rollout, or staged after benchmark lanes?
- What minimum diagnostic fields are mandatory in workflow summaries versus raw logs for auditability?
