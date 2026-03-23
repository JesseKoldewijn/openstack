## 1. Benchmark gate policy implementation

- [x] 1.1 Add a benchmark regression gate script/mode that evaluates required lane metrics against prior successful baseline runs.
- [x] 1.2 Implement week 3+ thresholds (p95 +8%, p99 +12%, throughput -8%) with lane-aware pass/fail output.
- [x] 1.3 Implement strict failure behavior for required lanes when baseline is missing.
- [x] 1.4 Implement strict data-quality failures for required lanes when performance scenarios are missing or all skipped.

## 2. Consolidated reporting

- [x] 2.1 Add consolidated benchmark markdown generation that merges available fairness lanes (`fair-low`, `fair-medium`, `fair-high`, `fair-extreme`) into one report.
- [x] 2.2 Include required lane gate verdicts and threshold context in consolidated summary output.
- [x] 2.3 Ensure consolidated report is published to workflow summary and as an artifact for PR review.

## 3. CI wiring and required checks

- [x] 3.1 Update `ci.yml` benchmark jobs to execute regression gates for required lanes (`fair-low` non-main PRs, `fair-medium` main PRs).
- [x] 3.2 Wire required-check jobs to depend on benchmark gate outcomes, not only benchmark execution completion.
- [x] 3.3 Keep `fair-high` and `fair-extreme` reporting in scheduled/non-blocking workflows while preserving visibility in consolidated summaries.

## 4. Validation and documentation

- [x] 4.1 Add tests for gate logic including pass, threshold breach, missing baseline, and skipped-only scenarios.
- [x] 4.2 Add tests for consolidated report composition across multi-lane inputs.
- [x] 4.3 Update benchmark docs with regression policy, threshold rationale, and baseline seeding/recovery guidance.
- [x] 4.4 Run CI dry-runs (or local equivalents) for one pass case and one intentional regression-fail case and capture output examples.
