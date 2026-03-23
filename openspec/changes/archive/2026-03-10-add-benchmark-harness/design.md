## Context

The project currently has a parity harness that validates behavior equivalence between openstack and LocalStack, but it does not provide benchmark-grade runtime measurement. Performance improvements are being discussed (including large payload handling in S3), and we need a repeatable way to quantify baseline performance, compare openstack versus LocalStack under equivalent workloads, and detect regressions over time.

Benchmarking must cover all implemented services while remaining practical in local development and CI. The design should reuse existing parity infrastructure where possible, avoid destabilizing correctness workflows, and produce machine-readable output that can feed dashboards and optimization prioritization.

## Goals / Non-Goals

**Goals:**
- Add a benchmark mode that runs equivalent workload scenarios against both openstack and LocalStack and emits comparative performance reports.
- Provide broad all-services coverage through a lightweight profile and deeper workloads for high-impact services.
- Record actionable metrics (latency percentiles, throughput, error rate, warmup handling, delta ratios).
- Ensure benchmark fairness/reproducibility through controlled execution and run metadata capture.
- Support CI and local execution without changing existing parity correctness semantics.

**Non-Goals:**
- Replacing or removing the existing parity mismatch comparison workflow.
- Building a long-running distributed load generator platform in this change.
- Defining per-service optimization implementations (this change creates measurement infrastructure).
- Guaranteeing absolute cross-machine comparability without environment controls.

## Decisions

### Decision: Introduce benchmark as a sibling mode to parity, not a replacement
- Rationale: parity and benchmarking serve different outcomes (correctness vs runtime). Keeping modes separate avoids coupling pass/fail compatibility checks to performance noise.
- Alternatives considered:
  - Extend parity reports with timing only: rejected because parity normalization/retry logic is optimized for behavior validation, not statistically robust benchmarking.
  - Separate repository/tool: rejected for now due to duplication of target orchestration and scenarios.

### Decision: Reuse TargetManager and scenario model with benchmark-specific execution settings
- Rationale: existing dual-target startup and command orchestration already solve endpoint equivalence; benchmark mode can add warmup/iterations/concurrency controls on top.
- Alternatives considered:
  - Build a new runner from scratch: rejected due to duplicate maintenance and slower delivery.
  - Only benchmark openstack: rejected because relative comparison to LocalStack is required for prioritization.

### Decision: Add tiered profiles (`all-services-smoke`, `hot-path-deep`, optional `stress`)
- Rationale: all-services breadth is needed for visibility, but deep workloads for every service would be expensive and unstable in CI.
- Alternatives considered:
  - Single monolithic profile: rejected due to runtime and flakiness concerns.
  - Only deep profile: rejected because it would miss service-level regressions outside hot paths.

### Decision: Report relative metrics and run metadata, not only raw timings
- Rationale: raw numbers without context are difficult to compare across runs. Ratio and delta metrics plus environment metadata improve interpretability.
- Alternatives considered:
  - Raw per-step durations only: rejected as insufficient for decision-making.

### Decision: Keep benchmark outputs machine-readable JSON and CI artifact friendly
- Rationale: JSON aligns with existing parity report patterns and can be consumed by scripts, dashboards, and regression checks.
- Alternatives considered:
  - Text-only summaries: rejected because they are hard to automate and trend.

## Risks / Trade-offs

- [Risk] Benchmark noise from shared CI runners and host contention can hide regressions or produce false alarms. -> Mitigation: capture environment metadata, use repeat runs, configurable thresholds, and recommend scheduled runs on constrained runners for trend decisions.
- [Risk] AWS CLI process startup overhead may dominate short operations, skewing results. -> Mitigation: include warmup iterations, configurable minimum operation counts, and focus deep profiles on representative payload sizes/concurrency.
- [Risk] Large all-services scenario set increases maintenance burden as service surface evolves. -> Mitigation: enforce profile ownership and keep smoke scenarios minimal with clear templates for additions.
- [Risk] LocalStack version drift can invalidate historical comparisons. -> Mitigation: pin default LocalStack image in config and record image/version in report metadata.
- [Risk] Running deep benchmarks in pull requests may increase CI runtime/cost. -> Mitigation: run smoke profile in PRs and schedule deep profile on main/nightly.

## Migration Plan

1. Introduce benchmark data structures, report schema, and benchmark runner entrypoint while preserving existing parity mode behavior.
2. Add initial benchmark profiles and baseline scenarios for all currently registered services.
3. Integrate CI workflow(s) for smoke profile and optional scheduled deep profile, publishing JSON artifacts.
4. Establish baseline report snapshots and document interpretation guidance for optimization work.
5. Optionally add regression gating thresholds after initial baseline stabilization window.

Rollback strategy:
- Disable benchmark CI jobs and artifact publication without impacting parity correctness runs.
- Keep code path feature-flagged/profile-driven so benchmark mode can be turned off if instability appears.

## Open Questions

- Should benchmark scenarios be colocated with parity scenario files or moved to a dedicated `tests/benchmark/scenarios` tree?
- What concurrency defaults should be used for local runs versus CI runs?
- Should regression thresholds be advisory first (non-blocking) for a fixed number of weeks before any gating?
