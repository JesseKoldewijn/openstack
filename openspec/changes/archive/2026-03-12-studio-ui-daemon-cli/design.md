## Context

openstack already provides broad AWS API compatibility and strong automated validation via parity and benchmark harnesses, but manual exploratory testing is fragmented and mostly CLI/script driven. The current runtime model is also foreground-only, which makes long-lived local testing sessions and tool integrations awkward.

This change introduces two connected user experiences:
1. A first-party Studio SPA served by openstack itself for visual exploration, interaction inspection, and guided service workflows.
2. A daemon lifecycle CLI surface for process control (`start --daemon`, `stop`, `status`, `restart`, `logs`) so Studio and manual tests can run against a persistent managed process.

Constraints and existing architecture realities:
- Gateway currently handles `/_localstack/*` paths inline and must continue preserving AWS API behavior.
- Internal API has a richer route model but is not yet fully delegated through the gateway path.
- Existing service matrix is large; UI cannot rely solely on handcrafted per-service pages.
- This must remain local-first, deterministic, and testable in CI without brittle environmental assumptions.

## Goals / Non-Goals

**Goals:**
- Provide a built-in Studio SPA at a stable route namespace (for example `/_localstack/studio`) that is shipped with openstack and version-aligned with the backend.
- Provide daemonized lifecycle controls in CLI with reliable single-instance semantics, health-aware status checks, and graceful shutdown behavior.
- Support both guided service workflows and raw request exploration so manual testing can cover all implemented services, not just curated subsets.
- Build a deep test strategy that includes frontend unit tests, frontend component/integration tests, API contract tests, and browser E2E tests against real openstack runtime.
- Reuse existing benchmark/parity knowledge as seed definitions for Studio coverage and smoke flows where practical.

**Non-Goals:**
- Replacing existing benchmark/parity infrastructure.
- Implementing every possible advanced AWS workflow in Studio v1.
- Introducing remote multi-user auth/authorization for Studio (local developer mode first).
- Changing core AWS protocol semantics or compatibility guarantees.

## Decisions

### Decision 1: Serve Studio as first-party static assets from openstack
**Choice:** Bundle Studio build artifacts and serve them under reserved internal gateway paths.

**Rationale:**
- Eliminates cross-origin issues by keeping Studio same-origin with gateway and service endpoints.
- Guarantees frontend/backend compatibility per release.
- Simplifies user onboarding (`openstack` alone is enough).

**Alternatives considered:**
- External dev server only: fast iteration but weak distribution story and version drift risk.
- Separate standalone Studio process: increases operational complexity and breaks single-binary ergonomics.

### Decision 2: Introduce a dedicated Studio API namespace
**Choice:** Reserve a namespace such as `/_localstack/studio-api/*` for Studio metadata and control operations.

**Rationale:**
- Clean separation from AWS-compatible routes and existing internal API compatibility endpoints.
- Allows schema and versioning strategy for Studio-specific APIs.
- Enables strict route allow-listing and auditability.

**Alternatives considered:**
- Reusing generic `/_localstack/*` endpoints without namespacing: increases coupling and discoverability ambiguity.

### Decision 3: Support dual interaction modes in Studio (guided + raw)
**Choice:** Implement both service-aware guided workflows and a universal raw request console.

**Rationale:**
- Guided workflows improve usability and onboarding.
- Raw console prevents coverage blind spots and enables quick testing of unsupported or new endpoints.
- Combined mode helps manual parity validation scale with service count.

**Alternatives considered:**
- Guided-only UX: easier UX but poor long-tail coverage.
- Raw-only UX: maximal flexibility but steep learning curve and lower productivity.

### Decision 4: Daemon lifecycle as first-class CLI commands
**Choice:** Add explicit lifecycle commands with one process ownership model and PID/state tracking.

**Rationale:**
- Stable and scriptable operational model for local developer workflows.
- Enables Studio and E2E suites to target persistent instances.
- Reduces accidental duplicate process collisions and port contention.

**Alternatives considered:**
- Keep foreground only + shell scripts: fragile and inconsistent across platforms.
- Implicit daemon behavior by default: can surprise existing users and complicate debugging.

### Decision 5: Testing pyramid with contract-first E2E expansion
**Choice:** Define required test layers and quality gates before implementation completion.

**Rationale:**
- UI and daemon logic introduce new failure modes not covered by existing parity tests.
- Contract tests reduce frontend/backend drift risk.
- Browser E2E ensures real user flows across Studio + gateway + services remain valid.

**Alternatives considered:**
- Relying on unit tests only: insufficient for integration-heavy behavior.
- E2E-only: too slow and brittle for full confidence alone.

### Decision 6: Phased rollout with feature-gated expansion
**Choice:** Deliver in phases: foundation routes + daemon controls, then Studio MVP, then breadth expansion by service classes.

**Rationale:**
- Reduces risk in a cross-cutting change.
- Enables early CI stability and user feedback.
- Keeps parity with ongoing backend evolution.

**Alternatives considered:**
- Big-bang launch of full service UI: high integration risk and long stabilization cycle.

## Risks / Trade-offs

- **[Route collision or behavioral regressions in gateway path handling]** → Mitigation: explicit route precedence tests for AWS routes vs `/_localstack/studio/*` and `/_localstack/studio-api/*`; require zero regression in existing gateway integration suites.
- **[Frontend/backend contract drift over time]** → Mitigation: generated or schema-validated Studio API contract tests in CI, plus compatibility assertions in E2E flows.
- **[Daemon state desynchronization (stale PID, zombie process, false status)]** → Mitigation: health endpoint verification combined with PID ownership checks; robust stale lock cleanup policy.
- **[UI scope explosion due to many services]** → Mitigation: capability matrix with tiered support labels (guided, raw, unsupported), and a backlog process tied to service coverage metrics.
- **[E2E flakiness from startup timing and async service readiness]** → Mitigation: deterministic readiness probes, standardized fixture setup/teardown, retry envelopes only at transport boundaries.
- **[Asset size and startup cost increase from embedded SPA]** → Mitigation: optimized production build, compression, lazy-loaded Studio modules, and startup performance budgets.

## Migration Plan

1. Add daemon lifecycle command surface without changing current foreground default behavior.
2. Introduce process state files and health-aware status checks.
3. Add Studio static route serving with minimal landing shell (feature-flagged if needed).
4. Add Studio API discovery endpoints and capability metadata.
5. Implement Studio core screens: service catalog, request console, interaction viewer, theme controls.
6. Add guided workflows for a representative first wave of services (S3/SQS/SNS/DynamoDB plus one REST-style service).
7. Expand automated test coverage across frontend, backend contract, and browser E2E; enforce quality gates in CI.
8. Document usage (`start --daemon`, `status`, `stop`, Studio URL, troubleshooting), then graduate feature from experimental to default.

Rollback strategy:
- Keep daemon mode opt-in until stabilized.
- Keep Studio routes behind a runtime flag in early phases.
- If regressions appear, disable Studio serving and daemon mode independently while preserving core gateway behavior.

## Open Questions

- Should daemon logs be file-backed only, or also queryable via internal API for Studio display?
- Should Studio interaction history persist across restarts by default, and if yes, where/how is retention managed?
- Which browser E2E framework best fits repo constraints and CI speed targets?
- Should Studio support opening externally (non-localhost) in v1, or remain strictly local-first?
- How should capability metadata be authored: static manifest, generated from service registry, or hybrid?
