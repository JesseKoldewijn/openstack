## 1. Baseline and workflow profiling

- [ ] 1.1 Capture baseline median and p95 durations for Docker and cross-compile workflows using recent successful main-branch runs.
- [x] 1.2 Record per-step timing breakdown for Docker build stages (setup, metadata, build/push, manifest-related steps) and identify dominant bottlenecks.
- [x] 1.3 Document current critical-path dependency graph for Docker and cross-compile workflows before refactor.

## 2. Docker workflow architecture optimization

- [x] 2.1 Refactor `.github/workflows/docker.yml` into fan-out architecture-specific build jobs (`amd64`, `arm64`) and a fan-in manifest publication job.
- [x] 2.2 Ensure manifest publication executes only after all required architecture jobs succeed and validates the resulting multi-arch reference.
- [x] 2.3 Preserve existing tagging/publishing semantics and branch/event behavior while introducing parallel job topology.

## 3. Docker build performance and cache improvements

- [x] 3.1 Update Docker build strategy to eliminate cache-hostile invalidation patterns and improve deterministic layer reuse.
- [x] 3.2 Tune Buildx cache configuration (`cache-from`/`cache-to`) for architecture-specific reuse and stable lockfile-based invalidation behavior.
- [ ] 3.3 Validate that source-only changes avoid full dependency recompilation in container builds where dependency metadata is unchanged.

## 4. Cross-compile throughput and guardrails

- [x] 4.1 Review and tune cross-compile matrix/concurrency settings to maintain predictable runtimes under runner load.
- [x] 4.2 Ensure cross-compile cache key strategy is deterministic per target and supports consistent reuse across runs.
- [x] 4.3 Verify cross-compile trigger and path filtering behavior aligns with intended release and change-scope rules.

## 5. Validation, observability, and documentation

- [ ] 5.1 Validate required-check behavior and release-path correctness for pull request and protected-branch scenarios after workflow changes.
- [ ] 5.2 Compare post-change Docker and cross-compile median/p95 durations against baseline and confirm measurable improvement or document regressions.
- [x] 5.3 Update change-level implementation notes with optimization decisions, runtime comparison evidence, guardrails, and rollback procedure.
