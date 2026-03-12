## 1. Dashboard frontend foundation

- [x] 1.1 Replace Studio placeholder shell assets with bundled dashboard frontend entrypoint and static asset serving path.
- [x] 1.2 Implement dashboard home view model composition from service catalog, flow catalog, and coverage endpoints.
- [x] 1.3 Implement service detail layout with guided-flow panel, raw-console panel, and history panel regions.
- [x] 1.4 Add dashboard navigation and route-state handling for home, service detail, and replay contexts.

## 2. Guided and raw operation experiences

- [x] 2.1 Integrate guided-flow execution UI with manifest-backed input forms and runtime state transitions.
- [x] 2.2 Implement guided execution result visualization (step timeline, assertions, captures, cleanup, and error guidance).
- [x] 2.3 Integrate raw interaction editor and response viewer into dashboard service detail surface.
- [x] 2.4 Implement interaction history capture and replay flows for both guided and raw workspaces.

## 3. Studio API and gateway contract alignment

- [x] 3.1 Validate and refine internal Studio API response contracts needed for dashboard composition and operation workflows.
- [x] 3.2 Ensure gateway Studio route handling serves bundled assets and preserves SPA fallback semantics.
- [x] 3.3 Add/extend gateway security guardrail coverage for Studio execution endpoints (method allow-list and payload limits).
- [x] 3.4 Ensure no regression in AWS route dispatch and protocol handling while Studio route surface expands.

## 4. CLI Studio browser-open capability

- [x] 4.1 Add CLI command parsing for `openstack studio` and `openstack studio --print-url`.
- [x] 4.2 Implement Studio URL resolution from runtime config and daemon-aware readiness checks.
- [x] 4.3 Implement cross-platform browser opener abstraction with deterministic fallback behavior.
- [x] 4.4 Add lifecycle-compatible UX messaging for runtime unavailable and opener-failure states.

## 5. Studio test matrix expansion

- [x] 5.1 Add unit tests for dashboard state composition, guided/raw workspace state transitions, and replay state restoration.
- [x] 5.2 Add render/component integration tests for dashboard views, service detail interactions, and accessibility-critical controls.
- [x] 5.3 Add/extend internal-api and gateway integration contract tests for Studio dashboard data and asset serving behavior.
- [x] 5.4 Add E2E journeys covering dashboard load, service selection, guided execution, raw execution, and history replay.
- [x] 5.5 Add CLI tests for Studio command behavior across normal, fallback, and print-url modes.

## 6. CI and quality gates

- [x] 6.1 Update CI jobs to enforce Studio test matrix layers for Studio-affecting changes.
- [x] 6.2 Ensure Studio coverage summaries report protocol/service operation coverage and highlight gaps.
- [x] 6.3 Keep Semgrep behavior as-is while documenting non-blocking finding expectations in Studio workflow docs.
- [x] 6.4 Run full validation sweep (fmt, clippy, Studio unit/integration/E2E, runtime-image checks) and resolve regressions.
