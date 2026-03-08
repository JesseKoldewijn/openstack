## Docker and cross-compile optimization implementation notes

## Baseline snapshot (pre-change)

Data captured from GitHub Actions API on 2026-03-06.

### Docker workflow

- Workflow: `.github/workflows/docker.yml`
- Latest observed run: `https://github.com/JesseKoldewijn/openstack/actions/runs/22774338070`
- Status at analysis time: in progress (full completion time not yet available)

Per-step timing observed:

- Setup + checkout + qemu + buildx + login + metadata: ~16s combined
- `Build and push image`: dominant long-running step (still in progress at sample time)

Critical path before refactor:

`single build-and-push job` -> `multi-arch build (amd64+arm64) in one step`

Interpretation:

- Docker wall-clock time is dominated by a single serialized multi-arch build/push step.
- Arm64 QEMU path likely contributes substantial runtime variance and tail latency.

### Cross-compile workflow

- Workflow: `.github/workflows/cross-compile.yml`
- Baseline run: `https://github.com/JesseKoldewijn/openstack/actions/runs/22773818875`
- Recent run: `https://github.com/JesseKoldewijn/openstack/actions/runs/22774338090`

Observed durations:

- Baseline run total: ~106s (17:10:33 -> 17:12:19)
- Recent run total: ~103s (17:24:53 -> 17:26:36)

Dominant per-job step:

- `Cross-compile release binary` step on both targets (~67-76s)

Critical path before and after changes remains matrix max:

`max(Build x86_64, Build aarch64)`

## Implemented workflow changes

### Docker workflow (`.github/workflows/docker.yml`)

- Split previous single job into fan-out/fan-in topology:
  - `metadata`
  - `build-amd64`
  - `build-arm64`
  - `publish-manifest`
- Added architecture-specific cache scopes:
  - `docker-amd64`
  - `docker-arm64`
- Added manifest creation and platform validation (`linux/amd64`, `linux/arm64`).
- Preserved existing trigger semantics and tagging strategy via shared metadata outputs.

### Dockerfile (`Dockerfile`)

- Pinned builder image from `rust:latest` to `rust:1.85-bookworm` for deterministic base/toolchain behavior.
- Removed cache-hostile forced source touching (`find ... touch`) that invalidated build layers.
- Removed the intentionally failing dependency-build warm-up pattern (`|| true`) and made dependency build deterministic.

### Cross-compile workflow (`.github/workflows/cross-compile.yml`)

- Kept existing matrix and concurrency settings (`fail-fast: false`, `max-parallel: 2`).
- Strengthened cache determinism per target with lockfile-derived keying:
  - `shared-key: cross-compile-${{ matrix.target }}`
  - `key: cross-${{ matrix.target }}-${{ hashFiles('**/Cargo.lock') }}`

## Guardrails

- Preserve multi-arch parity by requiring both arch image builds before manifest publication.
- Keep Docker build cache scopes separated by architecture.
- Keep cross-compile matrix bounded to reduce queue and runner saturation risk.
- Maintain existing trigger filters to avoid unnecessary workflow execution.

## Rollback procedure

If regressions are observed:

1. Revert `.github/workflows/docker.yml` to previous single-job build flow.
2. Revert `Dockerfile` cache strategy changes.
3. Revert `.github/workflows/cross-compile.yml` cache key changes.
4. Re-run Docker and cross-compile workflows on a representative commit.
5. Compare restored runtime behavior to baseline evidence above.

## Validation status

- Completed:
  - Workflow structure and cache strategy changes implemented.
  - Cross-compile deterministic cache and guardrails verified by static config review.
  - Baseline and bottleneck documentation captured.
- Pending live-run validation:
  - Full post-change Docker median/p95 comparison (requires completed post-change runs).
  - Required-check behavior in protected branch configuration.
