## Context

The completed `native-http-parity-benchmarks` change established native HTTP as the canonical transport for parity and benchmark execution, removed AWS CLI from core harness execution paths, and added machine-readable `follow_up_required` reporting for README baseline parity outcomes. Runtime validation then showed a clear readiness split:

- Core parity passes for `s3`, `sqs`, and `sts`, with two remaining mismatches (`dynamodb` missing-table error `content-type`, and the SNS disabled-service compatibility mismatch).
- All-services smoke parity passes only for `sqs` and `sts`; the other 22 README-listed services execute natively but remain semantically non-equivalent to LocalStack and are now explicitly marked `follow-up-required`.
- Representative native benchmarks execute successfully without CLI fallback, but required fair-core signal quality is incomplete because several services still lack write-role coverage, `sts` remains aux-only, and OpenStack RSS evidence is missing.

This follow-up change is not a second transport migration. It is a readiness and governance change that must turn validated gaps into one of three explicit outcomes: fixed parity, accepted difference, or explicit exclusion/follow-up that remains visible in reports.

## Goals / Non-Goals

**Goals:**
- Preserve the current all-services smoke scenarios as the authoritative README baseline while making their follow-up semantics precise and durable.
- Close the highest-signal parity equivalence gaps exposed by native HTTP validation, starting with services and behaviors already observed in the completed reports.
- Define a clear rule for when unresolved mismatches become accepted differences versus when they remain follow-up-required parity failures.
- Make required fair-core benchmark lanes role-complete or explicitly excluded, so `lane_interpretable` reflects real workload completeness rather than partial native execution success.
- Keep benchmark diagnostics transparent when runtime evidence is incomplete, especially for OpenStack RSS collection.
- Refresh readiness evidence artifacts so future archive or rollout work does not rely on stale blocker notes or implicit tribal knowledge.

**Non-Goals:**
- Replacing the 24-service smoke probes with richer lifecycle scenarios in the same change.
- Broadening service scope beyond the README-listed baseline already validated.
- Reworking the scenario schema away from command-vector definitions.
- Performing repository code commits or rollout actions as part of this proposal.

## Decisions

### 1. Treat the all-services smoke profile as the authoritative readiness contract, not as disposable probe scaffolding

The design keeps `tests/parity/scenarios/all-services-smoke.json` and `tests/benchmark/scenarios/all-services-smoke.json` as the canonical README baseline contract. Richer lifecycle scenarios may continue to exist in core/deep profiles, but they do not replace the all-services smoke inventory for readiness reporting.

Rationale:
- The README claim is about supported services, not just deep-service subsets.
- The current validation evidence is already keyed to this 24-service smoke inventory.
- Replacing the smoke contract now would blur whether readiness improved or the yardstick changed.

Alternatives considered:
- Replace smoke probes immediately with richer lifecycle scenarios: attractive long-term, but it would invalidate the just-established baseline.
- Treat all-services smoke as temporary and measure readiness only by core/deep profiles: too weak for README service accountability.

### 2. Classify unresolved parity gaps into explicit governance buckets

Every validated mismatch in the README baseline should end up in one of three buckets:

- **Equivalent**: no unaccepted mismatch remains.
- **Accepted difference**: the mismatch is semantically understood, intentionally tolerated, and recorded in `tests/parity/known_differences.json` with full metadata.
- **Follow-up-required**: the mismatch remains unresolved and must continue to surface in machine-readable parity reporting.

This design does not permit “soft hiding” through normalization unless the difference is proven non-semantic.

Rationale:
- The previous migration already introduced machine-readable follow-up reporting; this change must decide how gaps graduate out of it.
- Several observed mismatches are policy questions, not only code defects, such as DynamoDB error `content-type` and SNS disabled-service compatibility.
- The known-difference registry is already the correct mechanism for tolerated divergence.

Alternatives considered:
- Normalize more mismatches until reports look green: unsafe because it can erase real compatibility differences.
- Leave all 22 services as indefinite follow-up-required results: useful for visibility, but insufficient as a readiness outcome.

### 3. Prioritize parity fixes by mismatch class, not by service popularity alone

Implementation work should be sequenced by the type of parity gap:

```text
Priority order

1. Cross-cutting false differences
   - header/content-type policy
   - compatibility governance

2. “openstack succeeds / LocalStack fails” probe bugs
   - cloudformation
   - cloudwatch
   - ec2

3. “openstack not implemented / wrong error shape” gaps
   - acm, apigateway, ecr, events, etc.

4. Service-specific header/error-shape refinements
   - s3, kms, secretsmanager, ssm, route53, ses, sns
```

Rationale:
- Some gaps are likely one-line policy or normalization/governance choices that unlock multiple services.
- Success-vs-failure mismatches distort the baseline more than content-type-only mismatches.
- This ordering prevents time being spent on low-signal formatting differences before semantic probe behavior is corrected.

Alternatives considered:
- Implement strictly alphabetically by service: easy to track, but ignores leverage.
- Focus only on core profile gaps first: insufficient because the all-services smoke contract is the baseline readiness measure.

### 4. Required fair-core benchmark lanes must be role-complete or explicitly excluded per service

The benchmark design should treat read/write role completeness as a hard interpretability prerequisite for required lanes. The workload matrix must no longer allow ambiguous scenario roles such as `aux` to quietly satisfy service coverage expectations.

Expected outcomes:
- services with only read scenarios gain measured write scenarios, or
- role exclusions are declared with machine-readable reason codes, or
- the lane remains non-interpretable by design until coverage is fixed.

Rationale:
- The current `fair-low-core` report runs successfully but still reports 9 missing required roles.
- `sts-core-call` being `aux` demonstrates that successful execution alone is not enough to make a lane analytically valid.
- The workload matrix spec already provides the right contract for explicit roles and exclusions.

Alternatives considered:
- Continue allowing partial role coverage as “good enough”: weakens benchmark signal quality guarantees.
- Remove services from required lanes until roles are complete: obscures coverage debt instead of surfacing it.

### 5. Missing runtime evidence should remain diagnostic, not silently omitted

Benchmark report generation should preserve missing runtime evidence such as absent OpenStack RSS collection and make it explicit in summary fields and readiness notes, rather than silently treating the lane as fully observed.

Rationale:
- The current report already shows `missing targets=openstack` for memory RSS; that is useful signal.
- Readiness decisions should distinguish “transport succeeded” from “all observability dimensions are present.”
- This approach matches the broader philosophy of explicit gaps over hidden fallback.

Alternatives considered:
- Hide incomplete diagnostics from the main report: reduces noise, but makes memory readiness harder to reason about.
- Fail the whole lane on missing RSS alone: too strict relative to the rest of the benchmark signal.

## Risks / Trade-offs

- [Accepted-difference policy becomes a dumping ground] -> Mitigation: require narrow scope, rationale, owner/reviewer, and expiry for each new entry; prefer fixes when behavior is clearly unintended.
- [The follow-up change grows into a service-by-service rewrite] -> Mitigation: preserve the smoke baseline, prioritize by mismatch class, and explicitly defer richer lifecycle scenario expansion.
- [Additional normalization hides semantic incompatibilities] -> Mitigation: only normalize values proven nondeterministic or structurally irrelevant; keep status, error family, and meaningful error message differences visible.
- [Benchmark role completeness adds too much scenario-authoring work in one change] -> Mitigation: permit explicit exclusions with reason codes where realistic workloads are not yet ready, but keep required lanes non-interpretable until those exclusions are consciously declared.
- [Readiness notes drift again after validation] -> Mitigation: make validation artifacts part of the task list and treat stale blocker text as documentation debt to remove before closure.

## Migration Plan

1. Audit the validated parity mismatches and assign each to fix vs accepted-difference vs still-follow-up-required decisions.
2. Implement the highest-signal parity corrections and update known-difference governance only for consciously accepted semantics.
3. Re-run core and all-services smoke parity profiles and refresh service-level readiness evidence.
4. Add or explicitly exclude missing write/read benchmark roles in required fair-core lanes, including the `sts` role decision.
5. Re-run representative benchmark lanes and confirm signal-quality summaries reflect the intended coverage state.
6. Clean readiness notes and follow-up inventories so they describe the new post-fix state without stale blocker sections.

Rollback strategy:
- Because this work is limited to harness behavior, scenario governance, and readiness reporting, rollback is code/config reversion rather than data migration.
- Any accepted-difference additions should remain auditable so they can be removed individually if a service fix lands later.

## Resolved During Implementation

- The DynamoDB error `content-type` mismatch and disabled-service SNS compatibility mismatch were resolved by implementation and governance updates that brought the README-authoritative parity baseline to `24/24` passed with `0` accepted differences.
- `sts` remains outside fair-core write-role coverage by explicit design; required lanes now report that state as an auditable write-role exclusion with `reason_code: service-write-not-applicable` instead of relying on implicit `aux` handling.
- Missing OpenStack RSS evidence remains a harness observability limitation, but it is now surfaced explicitly through lane-level `missing_runtime_evidence` rather than being silently omitted or conflated with transport/runtime failure.
- Remaining fair-core benchmark red is therefore treated as higher-signal scenario/runtime/performance follow-up rather than unresolved role-metadata or reporting-policy ambiguity.
