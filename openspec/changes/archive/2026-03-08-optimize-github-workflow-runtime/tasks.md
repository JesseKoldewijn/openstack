## 1. Baseline and workflow dependency mapping

- [x] 1.1 Inventory all active CI workflows and jobs under `.github/workflows/`, including current `needs` relationships and required status checks.
- [x] 1.2 Capture baseline median and p95 duration for each targeted workflow and identify the top runtime-contributing jobs.
- [x] 1.3 Produce a critical-path map for PR and main workflows showing which jobs are truly blocking merge readiness.

## 2. Parallelization and matrix restructuring

- [x] 2.1 Refactor workflow job graphs so independent checks (for example lint, format, unit tests) run in parallel and only required dependencies remain in `needs`.
- [x] 2.2 Introduce or normalize matrix-based execution for independent dimensions (such as OS/toolchain/features) with explicit `max-parallel` limits.
- [x] 2.3 Add or update an aggregate gate job (where needed) that depends only on required upstream jobs used for branch protection.

## 3. Cache, artifact, and selective execution optimization

- [x] 3.1 Implement deterministic cache keys and restore strategies tied to lockfiles/toolchain inputs for dependency and build caches.
- [x] 3.2 Add artifact publish/consume steps for expensive reusable outputs to avoid recomputation in downstream jobs.
- [x] 3.3 Configure safe path/change filters so optional non-impacted jobs are skipped while critical checks always run.

## 4. Validation, guardrails, and rollout

- [ ] 4.1 Validate workflow correctness and required-check behavior across pull request and protected branch scenarios.
- [ ] 4.2 Compare post-change workflow durations against baseline and confirm measurable runtime improvements.
- [x] 4.3 Document optimization rules, concurrency guardrails, and rollback steps in CI maintenance documentation.
