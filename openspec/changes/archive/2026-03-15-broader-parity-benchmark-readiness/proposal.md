## Why

The authoritative README-baseline parity profiles are now green, but the broader parity suite and benchmark suite still produce a noisy mix of real product gaps, low-signal wire-format mismatches, miswired benchmark profiles, and unsound benchmark scenarios. We need a focused follow-up change so the remaining red in those suites becomes trustworthy signal instead of a blend of service behavior issues and harness mistakes.

## What Changes

- Tighten broader parity comparisons so deeper profiles like `extended` stop failing on proven non-semantic S3 response noise while preserving the stricter README baseline contract.
- Repair benchmark profile resolution and reporting so diagnostic lanes such as `fair-high` and `fair-extreme` produce explicit, machine-readable outcomes instead of empty-profile failures or misleading invalidation.
- Make benchmark role metadata explicit and lane-aware so required role coverage is evaluated according to lane intent rather than inferred from scenario names or applied uniformly to partial diagnostic lanes.
- Fix unsound benchmark scenarios whose setup, warmup behavior, or target-specific identifiers currently manufacture `insufficient cross-target successful operations` failures that do not cleanly represent product behavior.
- Improve benchmark diagnostics so reports distinguish scenario-contract failures, harness limitations, and likely real OpenStack-vs-LocalStack behavior gaps, including the current missing in-process OpenStack RSS evidence path.
- Re-run the broader parity and benchmark suites after harness cleanup and capture the smaller set of remaining true product gaps for follow-up remediation.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `parity-harness`: refine broader-profile normalization and comparison so non-semantic wire noise does not masquerade as deeper parity failure while baseline readiness reporting stays strict.
- `benchmark-harness`: repair profile resolution, scenario validity handling, machine-readable diagnostics, and asymmetric runtime evidence reporting for benchmark lanes.
- `benchmark-service-workload-matrix`: make role requirements explicit per lane and per scenario so required-lane completeness and exclusions reflect intended benchmark coverage.
- `benchmark-signal-quality`: distinguish true product/runtime signal from harness or scenario-contract invalidation in benchmark outputs.
- `benchmark-regression-gate`: ensure non-required diagnostic lanes remain visible and auditable even when scenarios are unavailable or intentionally skipped.

## Impact

- Affected code: `crates/tests/integration/src/parity.rs`, `crates/tests/integration/src/native_http.rs`, `crates/tests/integration/src/benchmark.rs`, selected service implementations surfaced by broader suites, and benchmark scenario/profile loading paths.
- Affected data/config: `tests/parity/scenarios/extended.json`, benchmark scenario definitions under `tests/benchmark/scenarios/`, workload-role metadata, benchmark reports under `target/benchmark-reports/`, and parity reports under `target/parity-reports/`.
- Affected systems: broader parity CI visibility, fairness/deep benchmark reporting, benchmark regression summaries, and maintainer workflow for separating harness debt from real compatibility or performance gaps.
- Risk surface: over-normalizing meaningful differences, understating true product gaps by treating them as scenario debt, and broadening remediation scope unless the change stays anchored on measured suite failures and explicit diagnostic categories.

## Current Readiness

Post-cleanup reruns show this change achieved its signal-quality goal: broader-parity normalization debt, benchmark profile/reporting harness debt, and the highest-value scenario-contract defects were materially reduced before product follow-up was assessed.

- `extended` parity is green again, so the investigated broader-parity S3 XML/content-type noise is no longer obscuring readiness.
- Benchmark harness/profile debt is no longer the dominant explanation for red lanes: `fair-high` resolves intentionally, `fair-extreme` reports `skipped-by-policy`, non-required lanes remain visible, and asymmetric OpenStack RSS reporting stays explicit as `runtime-observability-limitation` rather than generic missing-target noise.
- Scenario-contract debt was materially reduced by repairing identifier capture, seeded read state, cleanup ordering, runtime-backed setup, and deep-lane scenario semantics. `hot-path-deep` now executes with `missing_required_role_count: 0`, and formerly misleading invalidations such as CloudWatch deep are gone.

The remaining evidence from `target/benchmark-reports/fair-low-latest.json`, `target/benchmark-reports/fair-medium-latest.json`, `target/benchmark-reports/fair-high-latest.json`, `target/benchmark-reports/hot-path-deep-latest.json`, and `target/benchmark-reports/fair-extreme-latest.json` now falls into clearer follow-up buckets:

- Low-signal normalization debt: addressed for the investigated broader-parity failures; no new high-priority normalization class was identified in the latest reruns.
- Harness/profile wiring debt: addressed for this change; latest invalid realistic lanes are no longer explained by empty-profile resolution, misleading lane completeness, or generic RSS diagnostics.
- Scenario-contract debt: materially reduced for realistic and deep lanes; the repaired setup/capture/seeding defects no longer explain the remaining invalid set, though future service-specific contract bugs can still be investigated separately if new evidence appears.
- True product/runtime follow-up: the remaining trustworthy red set now includes `lambda-list-functions-deep` (OpenStack-only failure under deep load), `s3-put-readme-payload-burst` (deep S3 write-path throughput degradation and OpenStack-only errors), and residual realistic-lane failures that persist after scenario repair across `s3`, `sqs`, `sns`, `dynamodb`, `kinesis`, `opensearch`, and `lambda`.

This leaves the suite in a better readiness state:

- `hot-path-deep` is now mostly trustworthy, with the remaining invalid scenarios acting as high-confidence product/runtime follow-up rather than harness noise.
- `fair-extreme` is behaving as intended for this environment and remains explicitly non-blocking until heavy-object benchmarking is enabled.
- `fair-low`, `fair-medium`, and `fair-high` still retain realistic-lane invalid scenarios, but those failures should now be treated as likely provider/runtime differences or service-level portability issues rather than the setup/capture/seeding defects repaired in this change.
