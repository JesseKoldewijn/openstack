## Why

The native HTTP transport migration is functionally complete, but readiness is not: core parity now passes only `s3`, `sqs`, and `sts`, all-services smoke parity passes only `sqs` and `sts`, and the remaining 22 README baseline services are surfaced as explicit follow-up-required gaps. We need a focused follow-up change now so those validated gaps become either LocalStack-aligned behavior, explicitly governed accepted differences, or documented exclusions with benchmark signal-quality coverage that is complete enough to support CI and remediation work.

## What Changes

- Resolve or govern the validated README baseline parity gaps exposed by the native HTTP smoke profiles, keeping LocalStack as the reference target and preserving machine-readable follow-up visibility until each gap is closed.
- Preserve the current all-services smoke scenarios as the authoritative readiness baseline while clarifying how richer core or lifecycle scenarios relate to, but do not replace, that baseline inventory.
- Tighten parity governance around unresolved baseline mismatches so every README-listed service ends up in one of three explicit states: equivalent, accepted difference, or follow-up-required.
- Complete native benchmark readiness for required fair-core lanes by adding missing write/read role coverage or explicit auditable exclusions, with special attention to the current `sts` aux-only classification gap.
- Improve benchmark diagnostics completeness so missing runtime evidence, such as unavailable OpenStack RSS measurements, remains visible without being mistaken for transport failure.
- Refresh readiness evidence and follow-up inventory artifacts so the post-migration state is clean, current, and actionable.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `parity-harness`: clarify README baseline readiness classification, preserve the all-services smoke probe contract as the authoritative migration baseline, and require explicit governance for unresolved semantic mismatches.
- `benchmark-harness`: tighten required fair-core native benchmark readiness so read/write role completeness or explicit exclusions are visible in machine-readable outputs, and keep missing runtime evidence visible in reports.
- `benchmark-service-workload-matrix`: refine role and exclusion requirements for required benchmark lanes so scenario classifications such as `aux` cannot silently satisfy read/write completeness.

## Impact

- Affected code: `crates/tests/integration/src/parity.rs`, `crates/tests/integration/src/native_http.rs`, `crates/tests/integration/src/benchmark.rs`, service-specific translators and normalizers, benchmark workload matrix logic, and report-generation paths.
- Affected data/config: parity known-difference governance, parity and benchmark scenario definitions, benchmark workload-role metadata, and validation/readiness notes under `openspec/changes/`.
- Affected systems: core parity CI, all-services smoke parity reporting, fair-core benchmark reporting and gate interpretation, and maintainer workflow for classifying unresolved native HTTP gaps.
- Risk surface: over-normalizing true semantic differences, under-governing accepted differences, widening the follow-up scope beyond the validated baseline, and leaving benchmark lanes non-interpretable despite native transport execution succeeding.

## Current Readiness

The parity side of this follow-up is now effectively complete: core parity and the authoritative 24-service all-services smoke baseline are green with no accepted differences, and `tests/parity/known_differences.json` remains empty.

The benchmark side is now materially improved and more trustworthy, but not fully green:

- required fair-core lanes now use explicit read/write role accounting rather than the prior read-only downgrade for low-tier/core profiles
- fair-core scenario files now include real write workloads for `dynamodb`, `firehose`, `iam`, `kinesis`, `s3`, `secretsmanager`, and `sns`
- `sts` no longer relies on implicit `aux` handling; reports now carry an explicit write-role exclusion with `reason_code: service-write-not-applicable`
- benchmark summaries now include lane-level `missing_runtime_evidence`, preserving the asymmetric OpenStack RSS limitation as explicit diagnostics

Representative reruns confirm the intended contract shift:

- `fair-low-core` now reports `role_coverage_mode: read-write-required`, explicit `missing_runtime_evidence.openstack`, and explicit `sts` write exclusion
- `fair-medium-core` does the same, and no longer hides role incompleteness behind heuristic lane policy

Those reruns also show the remaining fair-core red is now higher-signal than before:

- `fair-low-core` still has 2 missing required role gaps, driven by invalid `dynamodb-core-write` and `secretsmanager-core-write` outcomes plus their resulting missing write-role coverage
- `fair-medium-core` still has 6 missing required role gaps, driven by invalid `firehose-core-write`, `iam-core-read`, `s3-core-write`, `secretsmanager-core-write`, `secretsmanager-core-read`, and `sns-core-read` outcomes plus their resulting role-coverage gaps

This means the change achieved its benchmark signal-quality goal even though the fair-core lanes are not yet archive-green by behavior:

- role metadata, exclusions, and lane policy are now explicit and auditable
- missing runtime evidence is visible without being conflated with transport failure
- remaining non-interpretable fair-core results are now attributable to specific scenario/runtime/product performance problems rather than missing role metadata or hidden lane heuristics
