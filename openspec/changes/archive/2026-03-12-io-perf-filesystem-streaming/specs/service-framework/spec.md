## MODIFIED Requirements

### Requirement: Service lifecycle management
Each service provider has lifecycle states: `Available`, `Starting`, `Running`, `Stopping`, `Stopped`, `Error`. Must satisfy startup and concurrency budgets under full-service benchmark profiles.

The `RequestContext` type SHALL provide access to request body data through a `SpooledBody` handle (via `spooled_body` field) in addition to the existing `raw_body: Bytes` field. Services that need streaming access to the request body SHALL use `spooled_body`. For backward compatibility, `raw_body` SHALL continue to be populated for non-streaming dispatch paths.

- **Scenario: Service starts on first request** - Lazy start: transitions `Available` -> `Starting` -> `Running` on first request (unless `EAGER_SERVICE_LOADING`).
- **Scenario: Eager service loading** - `EAGER_SERVICE_LOADING=1` starts all services during startup.
- **Scenario: Service start failure** - Failed `start()` -> `Error` state, 503 on subsequent requests.
- **Scenario: Startup budget is enforced** - Startup metrics evaluated against configured budgets.
- **Scenario: RequestContext provides spooled body** - When a request is dispatched to a service provider, the `RequestContext` includes a `spooled_body` field containing the request body as a `SpooledBody` handle, allowing the provider to read it via `AsyncRead` without full memory buffering.

### Requirement: Thread-safe service access
Synchronized loading (no double-init), concurrent request handling. Must satisfy throughput and contention budgets.

- **Scenario: Concurrent requests during startup** - Only one initialization; second request waits.
- **Scenario: Concurrency contention budget is observable** - Framework exposes contention metrics for gate decisions.
