## Context

The current project has compatibility-oriented smoke tests that validate openstack behavior in isolation, but it does not continuously compare openstack behavior against LocalStack for the same request vectors. This leaves a blind spot where behavior can drift while single-target tests remain green. A parity harness must compare two live targets, normalize known nondeterministic fields, and produce stable, triage-friendly diffs across AWS protocol families already used in this codebase (rest-xml, query/xml, json, rest-json).

## Goals / Non-Goals

**Goals:**
- Build a dual-target parity runner that executes identical scenarios against openstack and LocalStack.
- Define protocol-aware normalization and comparison rules so diffs are high signal.
- Capture parity output as machine-readable artifacts (for CI gating and trend analysis) and human-readable summaries.
- Introduce a governed known-differences mechanism so accepted divergences are explicit, reviewed, and non-silent.
- Roll out in phases, starting with core services and expanding coverage safely.

**Non-Goals:**
- Reproducing the entire upstream LocalStack test suite in phase one.
- Asserting byte-for-byte equality for fields known to be nondeterministic (timestamps, generated IDs, request IDs).
- Replacing existing unit/integration/smoke tests; parity complements them.
- Committing to parity coverage for every service in one release.

## Decisions

1. **Dual-target differential architecture**
   - Decision: Model parity as one scenario executed against two targets, followed by normalization and comparison.
   - Rationale: Keeps test intent single-sourced and avoids drift between separate openstack/localstack test definitions.
   - Alternatives considered:
     - Maintain two independent test suites and compare aggregate pass rates (rejected: weak signal, no per-scenario diff trace).
     - Compare only SDK-level success/failure (rejected: misses payload/header/semantic mismatches).

2. **Protocol-aware comparators over raw body diffing**
   - Decision: Parse and normalize payloads based on protocol family before comparing.
   - Rationale: Query/XML ordering and formatting differences create noise under raw text diff.
   - Alternatives considered:
     - Raw string comparison only (rejected: too brittle).
     - Fully custom comparator per API operation (rejected: high maintenance cost).

3. **Known-differences registry with explicit lifecycle**
   - Decision: Introduce a versioned registry mapping scenario/service/field to accepted difference rationale and expiry/review metadata.
   - Rationale: Prevents permanent silent drift while allowing pragmatic progress.
   - Alternatives considered:
     - Ignore known diffs ad hoc in test code (rejected: hidden policy, hard to audit).
     - Block all diffs with strict-only mode from day one (rejected: slows adoption and creates noisy failures).

4. **Profile-based CI rollout**
   - Decision: Add parity profiles (e.g., core, extended) and gate required checks on core profile first.
   - Rationale: Contains CI cost and flakiness while establishing a reliable baseline.
   - Alternatives considered:
     - Run full parity matrix on every PR initially (rejected: likely unstable and expensive).

5. **Structured result contract**
   - Decision: Emit JSON results that include scenario id, service, request summary, normalized diffs, classification, and pass/fail.
   - Rationale: Supports CI annotations, trend dashboards, and deterministic triage workflows.
   - Alternatives considered:
     - Human-readable logs only (rejected: poor automation and regression tracking).

## Risks / Trade-offs

- **[Risk] LocalStack version changes alter oracle behavior** -> Mitigation: pin LocalStack image tag in parity CI and update intentionally with review.
- **[Risk] Flaky tests due to startup timing/network variability** -> Mitigation: explicit health checks, retries with bounded backoff, and deterministic scenario setup/teardown.
- **[Risk] Comparator over-normalization hides real incompatibilities** -> Mitigation: strict defaults, minimal normalization policy, and review required for new ignore rules.
- **[Risk] CI duration growth** -> Mitigation: profile-based execution, service sharding, and core-only required checks initially.
- **[Risk] Registry bloat of accepted differences** -> Mitigation: require rationale + owner + review date, fail checks on expired entries.

## Migration Plan

1. Introduce harness scaffold and dual-target runner behind a non-required CI job.
2. Add first protocol comparators and core scenarios (S3, SQS, DynamoDB, STS) with JSON reporting.
3. Add known-differences registry and triage documentation; convert repeated noisy diffs into governed entries.
4. Promote core profile to required CI check once stable failure rate and runtime targets are met.
5. Expand service coverage in slices, keeping each slice non-required until stabilized.

Rollback strategy: disable parity CI job requirement (or job itself) while keeping harness code intact; existing smoke/integration suites remain primary guardrails.

## Open Questions

- Should parity compare raw wire payloads only, or also include post-request state verification reads by default?
- What runtime budget should core parity checks target for PR gating?
- Which LocalStack edition/tag policy should be adopted for long-term stability vs feature freshness?
- Do we require service owners to approve new known-difference entries for their domains?
