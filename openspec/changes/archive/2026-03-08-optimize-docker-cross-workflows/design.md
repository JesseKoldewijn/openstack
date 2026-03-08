## Context

The repository already has baseline CI optimization work in progress, but Docker and cross-compile pipelines remain the slowest and least predictable parts of release-oriented automation. The Docker workflow currently performs a multi-platform build (`linux/amd64`, `linux/arm64`) in a single job, which serializes expensive work and can amplify QEMU overhead. The cross-compile workflow is faster than Docker but still depends on runner capacity, cache quality, and matrix tuning.

Constraints:
- Preserve release quality and parity (no reduction in artifact correctness checks).
- Maintain compatibility with GitHub Container Registry publishing and current tagging behavior.
- Keep required-check semantics stable for protected branches.

Stakeholders:
- Contributors needing faster PR feedback.
- Maintainers publishing release artifacts.
- CI cost and reliability owners.

## Goals / Non-Goals

**Goals:**
- Reduce Docker workflow wall-clock time by parallelizing architecture-specific image build paths.
- Improve Docker build cache hit rates by avoiding unnecessary invalidation in Dockerfile and workflow cache configuration.
- Keep cross-compile workflow consistently fast and predictable under load using explicit concurrency and matrix guardrails.
- Add measurable before/after performance tracking for Docker and cross-compile workflows (median and p95).

**Non-Goals:**
- Replacing GitHub Actions with another CI platform.
- Changing application runtime behavior or service functionality.
- Expanding artifact coverage to additional platforms beyond current targets in this change.

## Decisions

1. Split Docker multi-arch build into parallel architecture jobs with manifest fan-in.
   - Decision: Build `amd64` and `arm64` images in separate jobs and publish a final multi-arch manifest in a dependent fan-in job.
   - Rationale: Reduces critical-path waiting from single-job serialized overhead and makes per-arch bottlenecks visible.
   - Alternatives considered:
     - Keep one multi-platform Buildx invocation (simpler but slower and opaque).
     - Build only `amd64` on PRs and full multi-arch on push (faster PRs but weaker parity).

2. Make Dockerfile/build path cache-stable.
   - Decision: Remove cache-hostile invalidation patterns and adopt deterministic layering so dependency layers survive source-only edits where possible.
   - Rationale: Rust container builds are compilation-heavy; cache misses dominate runtime.
   - Alternatives considered:
     - Keep current Dockerfile and tune workflow cache knobs only (limited gains).
     - Fully separate binary build from Docker build immediately (strong gains, but larger workflow refactor risk).

3. Keep cross-compile matrix bounded and deterministic.
   - Decision: Retain matrix fan-out with explicit `max-parallel` and stable cache key strategy per target.
   - Rationale: Preserves throughput without runner saturation and avoids noisy runtime variance.
   - Alternatives considered:
     - Unbounded matrix parallelism (can increase queueing and flakiness).
     - Sequential target builds (predictable but slower).

4. Add explicit performance observability and regression guardrails.
   - Decision: Record baseline/post-change durations for Docker and cross-compile workflows, including per-step timings for dominant build steps.
   - Rationale: Prevents subjective optimization claims and supports rollback decisions.
   - Alternatives considered:
     - One-time spot checks only (insufficient for p95 validation).

## Risks / Trade-offs

- [Parallel Docker jobs increase configuration complexity] -> Mitigation: Use a clear fan-out/fan-in pattern and document publishing contract.
- [Cache key strategy still misses due to broad context changes] -> Mitigation: tighten Docker context sensitivity and review Dockerfile layer boundaries.
- [QEMU emulation remains dominant for arm64] -> Mitigation: isolate arm64 duration metrics and consider staged optimization (native runner or scheduled build strategy) as follow-up.
- [Manifest publication can fail after successful arch builds] -> Mitigation: add explicit manifest validation and clear failure diagnostics in fan-in job.

## Migration Plan

1. Capture baseline Docker and cross-compile run metrics from current main-branch runs.
2. Introduce Docker fan-out/fan-in job model behind existing trigger conditions.
3. Apply Dockerfile/cache refinements and validate image correctness on both architectures.
4. Validate branch protection and required-check behavior in pull request scenarios.
5. Compare post-change median/p95 to baseline over a representative run window.
6. If regressions occur, revert workflow changes and restore previous pipeline while preserving measurement artifacts.

## Open Questions

- Should PR builds publish per-arch temporary artifacts for diagnostics, or only perform build validation without pushing images?
- Do we want to gate manifest publication on both architecture-specific smoke checks?
- Is it acceptable to defer deeper Dockerfile build-system refactoring (e.g., dedicated binary artifact handoff) to a follow-up change if initial gains are sufficient?
