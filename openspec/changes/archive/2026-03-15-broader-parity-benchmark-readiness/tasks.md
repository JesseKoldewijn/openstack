## 1. Broader Parity Signal Cleanup

- [x] 1.1 Audit the current `extended` parity failures and classify each mismatch as low-signal normalization noise versus real semantic divergence.
- [x] 1.2 Update broader-profile parity normalization/comparison rules for the investigated non-semantic S3 XML and content-type differences without weakening README-baseline parity strictness.
- [x] 1.3 Re-run `extended` parity and confirm remaining failures, if any, represent material behavior differences rather than formatting noise.

## 2. Benchmark Harness and Profile Wiring

- [x] 2.1 Fix benchmark profile resolution so every configured profile, including `fair-high`, resolves to an intentional scenario set or explicit machine-readable configuration status.
- [x] 2.2 Update benchmark reporting to classify asymmetric in-process OpenStack RSS limitations explicitly instead of emitting generic missing-target diagnostics.
- [x] 2.3 Preserve explicit non-required lane reporting for `fair-high` and `fair-extreme` in machine-readable outputs and consolidated summaries.

## 3. Role Metadata and Lane Semantics

- [x] 3.1 Add explicit scenario role metadata to required benchmark scenarios that currently depend on inference, including `sts-core-call` in fair-core lanes.
- [x] 3.2 Make workload-matrix completeness checks lane-aware so required lanes stay strict while deep/diagnostic lanes report partial coverage without misleading missing-role failures.
- [x] 3.3 Re-run `fair-low-core`, `fair-medium-core`, `hot-path-deep`, and diagnostic lanes to verify role accounting and lane interpretability now match intended policy.

## 4. Benchmark Scenario Contract Repair

- [x] 4.1 Audit invalid benchmark scenarios that currently report `insufficient cross-target successful operations` and identify setup, warmup, seeding, or identifier-capture defects.
- [x] 4.2 Repair unsound all-services realistic scenarios so measured write/read operations use valid setup state and portable resource identifiers across both targets.
- [x] 4.3 Repair deep benchmark scenarios whose names, setup, or measured workload semantics currently misrepresent what the lane is testing.

## 5. Remaining Product-Gap Isolation

- [x] 5.1 Re-run broader fairness and deep benchmark lanes after harness/scenario cleanup and capture the reduced set of remaining likely product/runtime gaps.
- [x] 5.2 Document any remaining high-confidence product issues, including Lambda deep-lane failures and S3 deep write-path degradation if they persist after scenario repair.
- [x] 5.3 Refresh proposal/readiness notes or follow-up artifacts so broader-suite failures are categorized as normalization debt, harness debt, scenario debt, or true product follow-up.
