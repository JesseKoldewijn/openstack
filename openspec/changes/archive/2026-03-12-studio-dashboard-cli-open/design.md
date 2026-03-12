## Context

The current Studio surface is intentionally minimal: gateway serves a static shell page and lightweight placeholder assets, while Studio-related domain logic and API contracts already exist across `crates/studio-ui`, `crates/internal-api`, and gateway Studio routes. This creates an architectural asymmetry where backend capabilities outpace frontend usability. At the same time, CLI daemon lifecycle commands already exist, but there is no direct command for launching Studio in the browser.

This design introduces a cohesive dashboard experience and a CLI browser-open affordance without regressing existing Studio API consumers, guided-flow infrastructure, or CI security posture. It also institutionalizes a testing matrix that treats Studio as a first-class product surface with layered validation.

## Goals / Non-Goals

**Goals:**
- Deliver a real Studio dashboard UX that enables users to explore and operate all supported services through guided and raw workflows.
- Introduce a stable CLI command for opening Studio in the default browser with deterministic behavior across Linux/macOS/Windows and CI/headless environments.
- Preserve backward compatibility for existing Studio API endpoints while adding/normalizing contracts needed by the dashboard.
- Define and enforce comprehensive Studio test coverage (unit, render/component integration, contract integration, E2E).
- Keep Semgrep workflow and severity semantics functionally unchanged while ensuring Studio operations include security regression tests.

**Non-Goals:**
- Building an entire service-specific visual console for each AWS-compatible service in this change (we focus on shared dashboard patterns and operation surfaces).
- Replacing guided manifest semantics, protocol adapter architecture, or support-tier governance introduced by existing Studio guided-flow changes.
- Introducing production auth/multi-user access controls for Studio in this change.

## Decisions

### 1) Replace placeholder Studio shell with bundled dashboard frontend served by gateway

**Decision:** Serve a compiled Studio frontend bundle through existing Studio asset routes and preserve SPA-style route handling under `/_localstack/studio/*`.

**Rationale:**
- Maintains current gateway ownership for Studio serving.
- Avoids introducing a second web server/deployment surface.
- Aligns with existing route and cache-control behavior already tested by Studio E2E.

**Alternatives considered:**
- Separate dev/prod Studio server process (rejected for operational complexity).
- Keep shell static and rely on API-only clients (rejected for poor usability and discoverability).

### 2) Use API composition model for dashboard data instead of introducing heavy monolithic endpoint

**Decision:** Compose dashboard state from existing/extended endpoints (`services`, `flows/catalog`, `flows/{service}`, `flows/coverage`, execution/replay paths) with targeted contract additions where needed.

**Rationale:**
- Reuses tested API primitives.
- Keeps contracts explicit and capability-oriented.
- Reduces blast radius of backend changes and simplifies incremental rollout.

**Alternatives considered:**
- Single mega endpoint for all dashboard state (rejected due to tight coupling and reduced testability).

### 3) Introduce `openstack studio` CLI command with opener abstraction and graceful fallback

**Decision:** Add a CLI subcommand that:
- resolves Studio URL from runtime config,
- attempts default-browser open via platform-specific command,
- falls back to printing URL with actionable message if opener unavailable.

**Rationale:**
- Improves first-time and recurring UX substantially.
- Keeps behavior deterministic in headless environments and CI.
- Enables direct testing of command parsing and opener behavior through abstraction/mocking.

**Alternatives considered:**
- `openstack start --studio` flag only (rejected due to discoverability and command coupling).
- always print URL without browser open (rejected as lower usability).

### 4) Adopt explicit Studio test matrix as a release gate

**Decision:** Require coverage across four layers:
- unit/domain,
- render/component integration,
- API/gateway contract integration,
- end-to-end user journeys (including CLI-open behavior).

**Rationale:**
- Studio spans UI + API + gateway + CLI; single-layer testing is insufficient.
- Existing tests already show a base; formal matrix prevents future coverage drift.

**Alternatives considered:**
- E2E-heavy strategy only (rejected for slow feedback and poor root-cause localization).

### 5) Keep Semgrep behavior as-is and document non-blocking findings policy

**Decision:** No semantic change to Semgrep gate criteria in this change. Preserve non-blocking warning behavior and rely on Cloud policy for comment behavior while adding Studio security regressions in tests.

**Rationale:**
- User requested Semgrep remain as-is.
- Prevents policy churn while dashboard work is in-flight.

**Alternatives considered:**
- Switching to CE or changing block thresholds now (rejected as out-of-scope for this change).

## Risks / Trade-offs

- **[Risk] Frontend bundling/asset serving mismatch** → **Mitigation:** add gateway asset contract tests and Studio shell asset integrity checks in CI.
- **[Risk] Dashboard data fan-out introduces UI latency** → **Mitigation:** staged data loading + lightweight aggregate view model + cached service metadata refresh policy.
- **[Risk] CLI browser-open varies by platform** → **Mitigation:** platform adapter abstraction + unit tests for command resolution + fallback print-path tests.
- **[Risk] Test suite runtime growth** → **Mitigation:** classify tests by layer, keep most checks at unit/component level, reserve full E2E for representative paths.
- **[Risk] API contract drift between dashboard and backend** → **Mitigation:** internal-api contract snapshots + integration tests tied to spec scenarios.
- **[Risk] Security regressions in raw/guided operations UI paths** → **Mitigation:** keep existing gateway payload/method guardrails, add targeted regression tests for each Studio operation surface.

## Migration Plan

1. **Introduce dashboard frontend scaffold and gateway asset wiring** behind existing Studio route surface.
2. **Add/normalize API contracts** needed for dashboard composition without removing existing contracts.
3. **Integrate guided/raw/history dashboard workflows** using existing `studio-ui` domain models and runtime primitives.
4. **Add CLI `studio` open command** with fallback behavior and tests.
5. **Expand test matrix and CI gates** for render/integration/E2E coverage and security regressions.
6. **Rollout validation:** run full Studio-focused CI suites and existing global checks.

**Rollback strategy:**
- Revert to static Studio shell asset serving while preserving backend endpoints.
- Disable or hide CLI `studio` open command if needed without affecting daemon lifecycle commands.

## Open Questions

- Should dashboard default landing prioritize service catalog cards or recent interaction history for faster return-user workflows?
- Should `openstack studio` implicitly auto-start daemon when not running, or only print actionable guidance?
- Do we want parity snapshots (golden render outputs) for dashboard component rendering across theme modes?
- Should Studio dashboard expose per-service capability health badges derived from flow coverage quality metrics at first release?
