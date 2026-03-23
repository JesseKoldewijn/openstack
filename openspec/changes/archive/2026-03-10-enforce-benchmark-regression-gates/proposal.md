## Why

We now have fairness-focused benchmark lanes, but CI performance output is fragmented by lane and does not enforce strict regression bars. As we start active optimization work in this branch, we need consolidated reporting and deterministic CI gates so performance regressions are blocked early.

## What Changes

- Add consolidated benchmark reporting that combines low/medium/high/extreme lane outputs into a single readable CI summary artifact.
- Add a benchmark regression gate that compares current lane metrics to a previous successful baseline run for the same lane.
- Enforce strict week 3+ thresholds for tail latency and throughput regression on required CI lanes.
- Add explicit gate behavior for missing baselines, missing performance scenarios, and skipped-only results so failures are actionable.
- Integrate gate outcomes into required CI checks and PR comments.

## Capabilities

### New Capabilities
- `benchmark-regression-gate`: Defines CI pass/fail policy for benchmark performance regression versus baselines.

### Modified Capabilities
- `benchmark-harness`: Extend benchmark reporting requirements to include a consolidated multi-lane summary artifact for CI readability.

## Impact

- Affected workflow files under `.github/workflows/` for benchmark execution, summary publishing, and required checks wiring.
- Affected benchmark reporting and analysis scripts under `scripts/`.
- Affected PR reporting paths that surface benchmark summaries/comments.
- No production API contract changes; this is CI and engineering workflow behavior.
