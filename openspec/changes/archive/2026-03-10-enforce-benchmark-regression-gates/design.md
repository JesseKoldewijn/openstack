## Context

The benchmark harness now runs fairness-oriented low/medium/high/extreme lanes, but CI visibility is split across lane-specific markdown fragments and there is no hard performance gate to stop regressions. As optimization work accelerates in this branch, we need a deterministic CI policy that preserves gains and surfaces regressions with clear, actionable failure messages.

Current reporting script behavior is trend-oriented and lane-local, but not designed as a strict gate. Required checks currently validate benchmark jobs ran, not whether benchmark performance stayed within an accepted bar.

## Goals / Non-Goals

**Goals:**
- Produce one consolidated, human-readable benchmark summary in CI that includes all fairness lanes relevant to the run.
- Introduce strict week 3+ regression gate behavior for required lanes (`fair-low` on non-main PRs, `fair-medium` on main PRs).
- Enforce stable, explicit thresholds for p95/p99 latency and throughput ratio regression against prior successful baseline runs.
- Fail fast and clearly when baseline data is missing, performance scenarios are absent, or all scenarios are skipped on required lanes.
- Expose gate verdicts in workflow summary and PR comment output.

**Non-Goals:**
- Gating on `fair-high` or `fair-extreme` in this change (these remain trend/diagnostic lanes).
- Replacing benchmark scenario definitions or fairness runtime mechanics introduced previously.
- Changing production service behavior.

## Decisions

### 1) Separate reporting from enforcement
Implement a dedicated benchmark gate script (or gate mode in existing script) that returns non-zero on regression violations, independent of markdown formatting logic.

**Why:** Enforcement logic should be deterministic and testable without coupling to presentation formatting.

**Alternatives considered:**
- Keep a single script for both responsibilities: simpler file count, but higher coupling and harder to reason about failure paths.

### 2) Strict lane-specific week 3+ thresholds
Use fixed, explicit thresholds for required lanes:
- p95 latency ratio regression limit: +8%
- p99 latency ratio regression limit: +12%
- throughput ratio regression limit: -8%

Regression is computed against the previous successful run baseline for the same workflow lane.

**Why:** Fixed thresholds are easier to audit and communicate than adaptive heuristics during active optimization.

**Alternatives considered:**
- Dynamic statistical bands: more adaptive but adds complexity and lower explainability.

### 3) Strict missing-baseline behavior for required lanes
If no previous successful baseline can be resolved for a required lane, the gate fails with a clear error and remediation text.

**Why:** Week 3+ strategy requires hard protection; missing baselines should not silently allow regressions.

**Alternatives considered:**
- Warn-only baseline missing: useful for early rollout but weaker protection.

### 4) Consolidated CI benchmark summary artifact
Generate a single markdown output combining lane summaries (low/medium/high/extreme as available), and include gate verdict/status rows per required lane.

**Why:** One summary improves readability and review speed.

**Alternatives considered:**
- Keep lane-fragment markdown only: works, but makes PR review and troubleshooting slower.

### 5) Keep high/extreme non-blocking but visible
`fair-high` and `fair-extreme` stay non-blocking in scheduled workflow; their results are still included in consolidated reporting with explicit skip/failure notes.

**Why:** Heavy lanes are valuable for trend detection but can be noisy/resource-sensitive for required CI checks.

## Risks / Trade-offs

- [Risk] Shared-runner variability can produce false regressions near thresholds -> Mitigation: use weighted lane metrics and separate p95/p99/throughput thresholds with moderate tolerance.
- [Risk] Strict missing-baseline policy may fail early runs on new branches -> Mitigation: add clear remediation guidance and baseline seeding instructions.
- [Risk] Consolidated summary can hide lane-specific nuance -> Mitigation: include links/sections for each lane and preserve raw JSON artifacts.
- [Risk] Thresholds may become stale after major architecture shifts -> Mitigation: document threshold review cadence and change-control process in this capability.

## Migration Plan

1. Add/extend benchmark script support for strict gate evaluation and machine-readable verdict output.
2. Add consolidated summary generation that composes per-lane results into one markdown artifact.
3. Wire gate execution into `ci.yml` benchmark jobs for required lanes.
4. Ensure required-check jobs depend on gate-passing jobs.
5. Update PR comment generation to include consolidated summary + gate verdicts.
6. Validate with controlled dry runs (pass + intentional fail cases).

## Open Questions

- Should we add service-level guardrails (for specific hotspot services) in addition to lane aggregate thresholds in a follow-up?
- Do we want separate thresholds for push vs PR workflows, or one unified policy?
