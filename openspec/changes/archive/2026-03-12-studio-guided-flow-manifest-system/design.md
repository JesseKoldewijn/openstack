## Context

Openstack Studio currently includes foundational capabilities (daemon lifecycle controls, Studio routes, Studio metadata endpoints, typed API client foundations, early guided flow primitives), but these are still implementation-centric and not yet systematized for universal guided coverage. The project now needs an architecture that scales guided interactions to all supported services while preserving consistency, correctness, and maintainability.

Current constraints:
- Service surface area is broad (core + extended AWS-compatible services) with heterogeneous protocols.
- Existing service implementations differ in operation shape and response format (XML/JSON/query-target variants).
- Studio must remain same-origin through the openstack gateway and must not weaken existing AWS protocol compatibility.
- We need deterministic CI verification and governance, not “best-effort” coverage.
- We must avoid bespoke per-service UI implementations that become maintenance bottlenecks.

Stakeholders:
- Runtime/service maintainers who need a scalable manual validation UI.
- Studio/frontend maintainers who need a stable contract to build against.
- CI/reliability owners who need measurable all-service guided coverage.

## Goals / Non-Goals

**Goals:**
- Define a versioned, machine-validated guided flow manifest contract that supports all service protocols used in openstack.
- Introduce protocol adapters that execute a normalized operation model while isolating serialization/parsing specifics.
- Provide a guided flow execution engine capable of deterministic step orchestration, capture/binding, assertions, and cleanup semantics.
- Achieve baseline L1 guided flow coverage for all supported services using reusable manifest patterns.
- Enforce coverage and quality in CI with clear pass/fail governance.
- Enable safe contract evolution through schema versioning and compatibility policies.

**Non-Goals:**
- Replacing existing parity harnesses or smoke suites.
- Implementing advanced L3 guided workflows for every service in the first milestone.
- Building service-specific bespoke UI pages as the primary architecture.
- Introducing arbitrary scripting/eval in manifest expressions.

## Decisions

### Decision 1: Manifest-first architecture with versioned schema contract
**Choice:** Define a canonical JSON schema for guided flow manifests (v1) and treat manifests as first-class source of truth.

**Rationale:**
- Decouples guided behavior definition from UI implementation details.
- Enables linting, static validation, and CI policy enforcement.
- Supports long-term evolution with explicit versioning and migration paths.

**Alternatives considered:**
- Code-defined flows only (hard to govern, less discoverable, poor tooling ergonomics).
- Ad-hoc YAML/JSON without schema discipline (high drift and ambiguity risk).

### Decision 2: Canonical normalized operation model + protocol adapter execution
**Choice:** Use a single normalized operation structure (`method`, `path`, `headers`, `query`, `body`) and route execution through protocol adapters (`query`, `json_target`, `rest_xml`, `rest_json`).

**Rationale:**
- Gives Studio one renderer and one execution state machine while still honoring protocol specifics.
- Keeps protocol complexity localized and testable.
- Reduces duplicated per-service request-building logic.

**Alternatives considered:**
- Distinct operation model per protocol (higher complexity in engine and UI).
- Distinct operation model per service (does not scale).

### Decision 3: Strictly constrained expression and binding model
**Choice:** Allow a safe expression subset (`inputs`, `context`, prior `captures`, built-ins like `rand8`, `timestamp`) and disallow arbitrary script execution.

**Rationale:**
- Prevents security and determinism issues from dynamic scripting.
- Keeps manifests portable and auditable.
- Enables deterministic execution/replay.

**Alternatives considered:**
- Embedded scripting DSL/JS (higher power, but large security/perf/debug burden).

### Decision 4: L1-for-all service strategy with explicit maturity ladder
**Choice:** Require at least one L1 lifecycle flow per service now, then grow to L2/L3 incrementally.

**Rationale:**
- Delivers the user requirement (“guided flow for all services”) quickly and objectively.
- Preserves focus by defining clear progression states.
- Allows uniform governance metrics across services.

**Alternatives considered:**
- Deep flows for selected services only (violates requirement).
- Full deep parity immediately for all services (delivery risk too high).

### Decision 5: Flow quality gates and coverage governance as merge blockers
**Choice:** Add CI gates for manifest validation, contract tests, coverage reporting, and protocol representative E2E guided tests.

**Rationale:**
- Prevents regressions and partial coverage drift.
- Ensures new services/features cannot bypass guided experience requirements.

**Alternatives considered:**
- Informational reporting only (insufficient enforcement).

### Decision 6: Studio API enhancements for manifest/coverage discovery
**Choice:** Expose Studio manifest index and coverage metadata through internal Studio API endpoints.

**Rationale:**
- UI can render from live registry metadata rather than static compile-time assumptions.
- Enables introspection for CLI/tools and release diagnostics.

**Alternatives considered:**
- Bundle manifests only in frontend artifact (increases drift risk between runtime and UI).

### Decision 7: Backward-compatible schema evolution policy
**Choice:** Maintain semver-like manifest versioning rules:
- Minor version: additive backward-compatible fields.
- Major version: breaking structural/semantic changes with migration plan.

**Rationale:**
- Prevents ecosystem breakage as guided system evolves.
- Supports progressive adoption and manifest migrations.

**Alternatives considered:**
- Unversioned schema (high breakage risk).

## Risks / Trade-offs

- **[Manifest complexity growth]** → Mitigation: enforce style guide, lint rules, and schema constraints; keep expression language intentionally minimal.
- **[Protocol edge-case divergence]** → Mitigation: adapter contract tests per protocol + golden request/response fixtures + representative E2E scenarios.
- **[All-service requirement causes shallow low-value flows]** → Mitigation: define L1 quality bar (create/use/verify/cleanup) and enforce assertion requirements.
- **[Coverage governance false confidence]** → Mitigation: combine static coverage checks with runtime E2E execution checks and post-condition assertions.
- **[Service changes outpace manifest updates]** → Mitigation: add CI checks coupling service registry changes to manifest coverage updates.
- **[Performance overhead in Studio when loading many manifests]** → Mitigation: lazy load manifest details by service, cache parsed manifests, precompute index artifacts.
- **[Security concerns from expression injection]** → Mitigation: no dynamic code eval, strict parser, bounded substitution domains, payload bounds.

## Migration Plan

1. Introduce manifest schema v1 and validation toolchain (non-blocking warnings initially).
2. Build protocol adapter abstractions and conformance tests.
3. Implement guided flow runtime engine and wire to current Studio foundations.
4. Add manifest registry endpoints and coverage report endpoints in internal API.
5. Author L1 manifests for all services and validate through CI coverage checks.
6. Enable hard CI gating for missing/invalid manifests and contract test failures.
7. Expand representative E2E suites to cover all protocol classes and selected per-service flows.
8. Document authoring conventions and release governance.
9. Transition from warning mode to blocking mode for all-service guided completeness.

Rollback strategy:
- Keep guided-manifest execution behind feature toggle until validated.
- Preserve raw interaction console as fallback path.
- If adapter regression occurs, disable affected adapter/flow class while retaining other guided paths.

## Open Questions

- Where should manifests be stored long-term (repo static assets vs generated from provider metadata hybrid)?
- Should manifest validation run at build-time only, runtime startup only, or both?
- How strict should L1 be for services with naturally asynchronous eventual consistency behavior?
- Should coverage enforcement treat disabled services differently from enabled-by-default services?
- Do we need protocol-specific extension fields in v1, or can all required behavior remain in normalized operations + adapter metadata?
