## Context

The repository now has two distinct readiness layers:

- the README-baseline native parity contract, which is green in `core` and `all-services-smoke`
- the broader parity and benchmark suites, which still show failures and invalid lanes after the baseline parity work completed

Recent suite investigation showed those remaining failures are not all the same class of problem. The current red surface includes:

- low-signal broader-parity mismatches in `extended`, currently dominated by S3 content-type and XML-format differences rather than broken behavior
- benchmark harness wiring defects, such as `fair-high` resolving to zero scenarios even though the profile is defined
- scenario metadata defects, such as `sts-core-call` becoming `aux` because role inference depends on scenario naming rather than explicit role declarations
- scenario-contract defects, where benchmark steps use guessed identifiers, warmup-poisoned write operations, or read paths without durable seeded state, leading to repeated `insufficient cross-target successful operations`
- a smaller set of likely real product/runtime gaps that remain after harness noise is discounted, especially Lambda deep-lane failures and S3 deep write-path errors under load

This change is therefore not a general parity remediation pass. It is a signal-quality cleanup pass for broader parity and benchmark suites so future failures map more directly to real product behavior.

## Goals / Non-Goals

**Goals:**
- Make broader parity profiles report only meaningful remaining mismatches, with low-signal normalization noise removed or explicitly categorized.
- Ensure every benchmark profile either resolves to an intentional scenario set or produces an explicit, machine-readable configuration outcome.
- Replace benchmark role inference for critical lanes with explicit role metadata and lane-aware completeness rules.
- Repair unsound benchmark scenarios so invalid results are not caused by benchmark setup mistakes, target-specific hardcoding, or warmup/measurement contract violations.
- Preserve explicit reporting for harness limitations such as missing in-process OpenStack RSS, without allowing those diagnostics to masquerade as product regression.
- Re-run broader parity and benchmark lanes after cleanup and leave a smaller, more trustworthy backlog of true product parity and performance issues.

**Non-Goals:**
- Re-opening the already-green README baseline parity contract unless broader-suite cleanup uncovers a true baseline regression.
- Solving every remaining service performance problem in the same change.
- Replacing the command-vector scenario schema with a new benchmark DSL.
- Making non-required diagnostic lanes block PR workflows by default.

## Decisions

### 1. Separate suite failures into four explicit buckets before remediation

The change will treat remaining failures as one of four categories:

```text
1. Real product gaps
2. Harness/profile wiring defects
3. Scenario-contract defects
4. Low-signal normalization noise
```

Rationale:
- The current suite output mixes all four, which makes aggregate failure counts misleading.
- Product fixes should follow only after the harness and scenario contracts stop generating false red.
- Proposal, tasks, and follow-up evidence become clearer when each failure belongs to a named category.

Alternatives considered:
- Continue treating all failing/invalid results as equivalent backlog items: simpler, but it obscures root cause and wastes remediation effort.

### 2. Keep broader parity stricter than ad hoc normalization, but allow protocol-aware suppression of proven non-semantic S3 noise

The broader parity cleanup will only normalize differences already shown to be non-semantic in the investigated `extended` failures, such as XML media-type variations, empty-element formatting, or equivalent default object content-type representations when the scenario does not set one explicitly.

Rationale:
- `extended` is currently red for S3 differences that do not indicate broken lifecycle behavior.
- The existing parity policy still forbids normalizing status codes, error families, or meaningful message differences.
- This keeps the broader suite useful without weakening the README baseline discipline.

Alternatives considered:
- Force exact LocalStack fidelity for all XML and content-type details: possible, but higher implementation cost for low signal.
- Accept `extended` as permanently noisy: undermines broader-suite usefulness.

### 3. Make benchmark role semantics explicit and lane-aware

Required-lane coverage SHALL be determined from explicit scenario role metadata and lane policy, not from inferred scenario names alone and not from a globally uniform interpretation of every lane.

Implications:
- `fair-low-core` and `fair-medium-core` remain strict required lanes
- `fair-high` and `fair-extreme` remain diagnostic/non-blocking lanes with explicit reporting semantics
- partial deep lanes such as `hot-path-deep` should not be penalized as if they promised complete write/read service coverage across every included service

Rationale:
- `sts-core-call` is currently invalidated due to heuristic inference, not behavior.
- `hot-path-deep` is intentionally partial, yet current role accounting makes it look structurally incomplete in a misleading way.
- Explicit metadata makes benchmark results auditable and deterministic.

Alternatives considered:
- Expand name-based inference rules further: brittle and likely to regress again.
- Keep all lanes under the same completeness rule: simpler but semantically wrong for diagnostic/deep profiles.

### 4. Treat repeated `insufficient cross-target successful operations` as scenario-contract debt until proven otherwise

The change will assume this invalid reason points first to benchmark contract review, not direct product blame. Scenarios must be checked for:

- valid setup and cleanup
- stable identifiers derived from setup outputs rather than guessed LocalStack-shaped values
- warmup behavior that does not poison measured writes
- read operations that only depend on state guaranteed to exist

Only after those conditions hold should remaining invalid runs be treated as product evidence.

Rationale:
- broad-lane failures currently include many scenarios whose contracts are weak or target-specific
- otherwise we risk turning benchmark authoring bugs into service remediation backlog

Alternatives considered:
- Triage each invalid scenario directly as a product defect: fast, but low confidence and likely wrong in many cases.

### 5. Preserve missing OpenStack RSS as explicit diagnostics, but adapt collection/reporting to asymmetric runtime reality

The benchmark harness should continue surfacing missing OpenStack RSS evidence, but the report should distinguish:

- missing because measurement failed unexpectedly
- unavailable because OpenStack ran in-process with no container-backed memory probe

Rationale:
- the current output is useful because it does not hide observability gaps
- but it is also predictable noise in every asymmetric run because only Docker-backed targets are currently measurable

Alternatives considered:
- remove RSS diagnostics from asymmetric runs entirely: simpler, but hides an observability gap
- fail lanes on missing RSS: too strict for current benchmark goals

### 6. Preserve non-required fairness lanes as visible diagnostics even when scenario availability is partial

`fair-high` and `fair-extreme` should continue to appear in reports and CI summaries, but their status must be explicit: configured, misconfigured, skipped-by-policy, or executed.

Rationale:
- `fair-high` currently fails as an empty profile wiring problem instead of reporting a usable diagnostic state
- `fair-extreme` currently reports skipped heavy scenarios, but that state should remain clearly non-blocking and machine-readable

Alternatives considered:
- drop these lanes entirely until they are production-ready: loses trend visibility
- make them required immediately: too much coupling to unresolved harness debt

## Risks / Trade-offs

- [Broader parity normalization grows beyond non-semantic noise] -> Mitigation: restrict normalization changes to investigated low-signal classes and keep service/error semantics untouched.
- [Scenario cleanup turns into service-by-service benchmark redesign] -> Mitigation: prioritize scenarios that currently dominate invalid counts or block required lanes first.
- [Lane-aware completeness rules become too permissive] -> Mitigation: keep required-vs-diagnostic lane policy explicit in specs and reports, and preserve strict gating for required lanes.
- [Real product gaps get deferred behind harness cleanup indefinitely] -> Mitigation: re-run suites after harness/scenario cleanup and capture the remaining red set as explicit product follow-up work.
- [Historical benchmark trend comparability weakens] -> Mitigation: document which failures were reclassified as harness/scenario debt so future comparisons are interpreted correctly.

## Migration Plan

1. Reclassify the current broader parity and benchmark failures into normalization, harness, scenario, and likely product buckets.
2. Adjust broader parity comparison/normalization only for proven non-semantic S3 noise in deeper profiles.
3. Repair benchmark profile resolution, explicit role metadata, and lane-aware completeness rules.
4. Repair unsound benchmark scenarios that currently manufacture invalid results.
5. Improve machine-readable runtime diagnostics, especially OpenStack RSS reporting in asymmetric mode.
6. Re-run `extended`, fairness lanes, deep lanes, and diagnostic lanes; capture the smaller remaining set of true product issues.

Rollback strategy:
- Revert harness/profile/scenario changes if broader-suite outputs become less trustworthy.
- Keep any product-gap findings documented separately so harness rollback does not lose investigation state.

## Open Questions

- For `extended` S3 parity, should the preferred fix be normalization-only, service-fidelity alignment, or a mixed approach?
- Should required role completeness be attached to profiles, scenario classes, or explicit lane policy metadata?
- For `fair-high`, is the intended source of truth a dedicated scenario file, filtered reuse of `hot-path-deep`, or both?
- Once benchmark scenario contracts are repaired, which remaining invalid scenarios should be promoted immediately into product-remediation follow-up work?
