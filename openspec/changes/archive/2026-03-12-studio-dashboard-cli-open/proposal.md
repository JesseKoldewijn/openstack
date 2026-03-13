## Why

Studio currently serves as a shell with backend metadata APIs but no operational dashboard that lets users discover, operate, and validate behavior across all supported services from one interface. We should close that gap now to improve local-cloud development ergonomics and reduce CLI/API context switching.

## What Changes

- Add a production-grade Studio dashboard frontend that replaces the current placeholder shell and provides service discovery, guided flow execution, raw request execution, and interaction history/replay in one coherent UI.
- Add Studio-specific backend aggregation and action contracts required by dashboard workflows, while preserving existing Studio API compatibility for current tests and clients.
- Add a new CLI command to open Studio in the user default browser, with robust cross-platform behavior and deterministic fallback when browser launch is unavailable.
- Expand test coverage strategy to enforce confidence at unit, render/component integration, contract integration, and end-to-end layers for all supported Studio operations.
- Keep Semgrep workflow behavior unchanged except for documenting expected non-blocking finding behavior and adding Studio-specific security regression coverage where needed.

## Capabilities

### New Capabilities
- `studio-dashboard-experience`: End-user dashboard experience for service catalog, guided operations, raw operations, and history workflows.
- `studio-cli-browser-open`: CLI capability to open Studio URL in the default browser with headless-safe fallback behavior.
- `studio-ui-test-matrix`: Layered Studio test strategy and required coverage guarantees for dashboard, execution flows, replay, and CLI-open behavior.

### Modified Capabilities
- `studio-ui`: Expand from shell-level behaviors to dashboard-level interactive behavior and rendering requirements.
- `internal-api`: Add or refine Studio API contracts used by dashboard aggregation and action execution paths.
- `gateway-core`: Update Studio asset serving and Studio route behavior to support bundled dashboard frontend and operation endpoints.
- `studio-e2e-validation`: Extend Studio E2E suite expectations from shell/guided spot checks to comprehensive dashboard and replay journeys.
- `daemon-cli-lifecycle`: Extend CLI behavior requirements with Studio-open command semantics and fallback behavior.

## Impact

- **Affected code**: `crates/gateway`, `crates/internal-api`, `crates/studio-ui`, `crates/openstack`, Studio integration tests, and CI workflow/test orchestration.
- **APIs**: Studio frontend and Studio API contracts under `/_localstack/studio*` and `/_localstack/studio-api/*`.
- **Tooling**: Test harness fixtures, render test scaffolding, E2E scenarios, and CI job expectations.
- **Security/compliance**: Preserve existing Semgrep workflow posture; ensure Studio dashboard changes include explicit security regression tests for operation endpoints.
