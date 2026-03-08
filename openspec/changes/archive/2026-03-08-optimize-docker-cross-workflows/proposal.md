## Why

The Docker workflow has become the dominant CI bottleneck, with multi-architecture container builds frequently taking much longer than core CI checks and delaying feedback/release readiness. We need targeted optimization now so Docker and cross-compile pipelines scale with repository growth without compromising artifact quality.

## What Changes

- Re-architect Docker workflow execution so architecture-specific image builds can run in parallel and publish a final multi-arch manifest in a controlled fan-in step.
- Reduce redundant Docker build work by tightening cache strategy and eliminating avoidable invalidation patterns in the Rust container build path.
- Separate binary production concerns from container packaging concerns where beneficial, so expensive compilation is reused rather than repeated.
- Add workflow-level telemetry and validation criteria for Docker and cross-compile jobs (median/p95 duration, critical step timing, and regression guardrails).
- Define selective execution and concurrency behavior specifically for Docker and cross-compile workflows to avoid wasteful runs while preserving required release checks.

## Capabilities

### New Capabilities
- `docker-workflow-performance-optimization`: Defines required behavior for parallel Docker image builds, cache reuse, artifact reuse, and manifest publication with measurable runtime targets.

### Modified Capabilities
- `github-workflow-runtime-optimization`: Extends existing workflow runtime optimization requirements with explicit Docker and cross-compile optimization coverage and validation expectations.

## Impact

- Affected code: `.github/workflows/docker.yml`, `.github/workflows/cross-compile.yml`, and Docker build context files such as `Dockerfile`.
- Affected systems: GitHub Actions runners, Buildx/QEMU multi-arch build path, container registry publish flow, and CI cache storage.
- Affected process: Release artifact production, branch feedback loop timing, and CI reliability governance for container-related checks.
