## Why

Our GitHub workflows currently run longer than necessary because independent jobs execute sequentially and expensive steps are repeated across workflows. Reducing end-to-end CI time now will speed up feedback loops for developers and lower compute usage.

## What Changes

- Add a structured workflow optimization plan for identifying critical path bottlenecks and parallelizable jobs.
- Introduce standardized patterns for safe parallel execution (job fan-out/fan-in, dependency mapping, and concurrency controls).
- Define caching and reuse improvements (dependency caches, artifact reuse, and selective execution) to avoid redundant work.
- Add measurable performance targets and validation criteria so workflow runtime improvements are tracked over time.

## Capabilities

### New Capabilities
- `github-workflow-runtime-optimization`: Framework for analyzing, restructuring, and validating GitHub Actions workflows to reduce total runtime through parallel execution and reduced duplication.

### Modified Capabilities
- None.

## Impact

- Affected systems: GitHub Actions workflow definitions under `.github/workflows/` and any shared CI scripts/actions.
- Affected processes: CI orchestration, caching strategy, matrix execution, and job dependency management.
- Expected outcomes: lower median and p95 workflow duration, faster PR feedback, and potentially reduced CI cost.
