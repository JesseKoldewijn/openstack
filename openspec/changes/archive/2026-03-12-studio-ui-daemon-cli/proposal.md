## Why

openstack currently relies on API-first workflows, parity harnesses, and integration suites, but lacks a first-party interactive surface for manual exploration and debugging. We need a built-in Studio SPA and daemon lifecycle CLI so contributors can run openstack as a managed local process, visually exercise services end-to-end, and validate behavior beyond scripted automation.

## What Changes

- Add a new Studio web application (Leptos SPA) served directly by openstack under a reserved internal route namespace, with Tailwind CSS v4 (CSS-config) and user-selectable light/dark mode.
- Add a new Studio interaction model combining service-aware workflows (guided flows for major services) and raw request exploration so users can test and inspect all supported service APIs through the same gateway path used by clients.
- Add new daemon lifecycle commands to the openstack CLI for process management (`start --daemon`, `stop`, `status`, `restart`, and log access), including single-instance protection and graceful shutdown semantics.
- Add Studio-specific backend endpoints for capability discovery, interaction metadata, and manual test orchestration while preserving existing AWS-compatible API surfaces.
- Add comprehensive test strategy and coverage requirements across unit, component, integration, and browser E2E layers to validate both UI behavior and UI-to-openstack API interactions.

## Capabilities

### New Capabilities
- `studio-ui`: Browser-based Studio SPA served by openstack for service exploration, interaction tracing, and manual test workflows.
- `daemon-cli-lifecycle`: CLI-managed daemon lifecycle for openstack process control, observability, and graceful state-preserving shutdown.
- `studio-e2e-validation`: End-to-end validation model and test harness for Studio user journeys and API interaction fidelity.

### Modified Capabilities
- `gateway-core`: Extend gateway requirements to serve Studio assets and Studio API routes safely alongside existing AWS and internal API paths.
- `internal-api`: Extend internal API requirements with Studio-oriented discovery/control endpoints and daemon-aware health/state reporting.

## Impact

- Affected code: `crates/openstack` (CLI parsing and daemon lifecycle), `crates/gateway` (route handling and static serving), `crates/internal-api` (Studio/daemon metadata endpoints), plus new Studio frontend crate/workspace members.
- Affected APIs: new `/_localstack/studio/*` and `/_localstack/studio-api/*` routes; expanded internal status/control response shapes.
- Dependencies/tooling: Leptos SPA toolchain, Tailwind CSS v4 pipeline, browser E2E framework, and test fixtures for deterministic service interaction replay.
- Operational behavior: openstack can run foreground or daemonized; status and shutdown behavior become externally controllable by CLI subcommands.
