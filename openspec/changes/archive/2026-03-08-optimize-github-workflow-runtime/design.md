## Context

Current GitHub Actions workflows appear to have avoidable critical-path delay from serial job execution, repeated setup steps, and inconsistent cache/artifact reuse. The repository is Rust-based and likely uses compute-heavy operations (build/test/lint/integration) that can benefit from explicit dependency graphing and matrix fan-out. The goal is to reduce pull request feedback time without reducing signal quality or CI safety.

## Goals / Non-Goals

**Goals:**
- Define a repeatable method to profile workflow runtime and identify the true critical path.
- Restructure workflows so independent validation tasks run in parallel with explicit fan-in gates.
- Improve cache and artifact strategies to eliminate duplicated work across jobs.
- Add governance (metrics and guardrails) so runtime regressions are visible and actionable.

**Non-Goals:**
- Replacing GitHub Actions with another CI platform.
- Reducing test coverage or quality bars just to improve runtime.
- Performing large-scale refactors of application code unrelated to CI orchestration.

## Decisions

1. Establish a baseline-first optimization loop.
   - Decision: Capture median/p95 workflow and key job durations before and after changes.
   - Rationale: Prevents subjective tuning and ensures optimizations are measurable.
   - Alternatives considered: direct restructuring without metrics (faster to start but higher risk of non-impactful changes).

2. Model workflow dependencies explicitly and parallelize by stage.
   - Decision: Convert linear pipelines into fan-out (independent checks) and fan-in (single required gate) using `needs` only where necessary.
   - Rationale: Shortens critical path while preserving deterministic gating.
   - Alternatives considered: fully independent workflows (can fragment status reporting and complicate required checks management).

3. Standardize matrix strategy with controlled concurrency.
   - Decision: Use matrix builds for independent dimensions (OS/toolchain/features), with `max-parallel` tuned to runner capacity.
   - Rationale: Enables scalable parallelism while avoiding runner saturation and queue thrash.
   - Alternatives considered: fixed, manually duplicated jobs (harder to maintain and slower to evolve).

4. Treat caches and artifacts as first-class performance primitives.
   - Decision: Introduce stable cache keys with restore prefixes and share build outputs via artifacts where recomputation is expensive.
   - Rationale: Reduces repeated dependency and compilation overhead across jobs.
   - Alternatives considered: cache-only or artifact-only approaches (either can underperform for mixed workloads).

5. Add selective execution where safe.
   - Decision: Use path and change filters to skip unaffected jobs, but keep a conservative fallback for critical checks.
   - Rationale: Avoids running expensive pipelines for unrelated changes while maintaining trust in CI results.
   - Alternatives considered: always-run strategy (simpler but slower/costlier).

## Risks / Trade-offs

- [Over-parallelization increases queue/wait time or flakiness] -> Mitigation: tune `max-parallel`, monitor queue latency, and cap concurrency groups.
- [Cache poisoning or stale outputs] -> Mitigation: include lockfile/toolchain in keys, use scoped keys per job purpose, and periodic cache key rotation.
- [Artifact transfer overhead outweighs recomputation] -> Mitigation: apply artifact reuse only to large/expensive outputs and set retention appropriately.
- [Selective execution misses required validation] -> Mitigation: default critical checks to always run and add fallback full-run triggers.
- [Workflow complexity reduces maintainability] -> Mitigation: document dependency graph and centralize reusable actions/composite steps.
