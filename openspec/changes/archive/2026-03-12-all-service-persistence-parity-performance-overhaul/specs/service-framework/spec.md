## MODIFIED Requirements

### Requirement: Service lifecycle management
Each service provider SHALL have a lifecycle with states: `Available`, `Starting`, `Running`, `Stopping`, `Stopped`, `Error`. The framework SHALL manage transitions and expose the current state. Lifecycle behavior SHALL satisfy startup and concurrency budgets under full-service benchmark profiles.

#### Scenario: Service starts on first request
- **WHEN** a request targets a service that is in `Available` state and `EAGER_SERVICE_LOADING` is not set
- **THEN** the framework SHALL transition the service to `Starting`, invoke its `start()` method, then transition to `Running` before dispatching the request

#### Scenario: Eager service loading
- **WHEN** `EAGER_SERVICE_LOADING=1` is set
- **THEN** all configured services SHALL be started during server startup, before accepting requests

#### Scenario: Service start failure
- **WHEN** a service's `start()` method returns an error
- **THEN** the service SHALL transition to `Error` state and subsequent requests SHALL return `503 Service Unavailable`

#### Scenario: Startup budget is enforced for required profiles
- **WHEN** required benchmark profiles execute startup-sensitive lanes
- **THEN** service-framework startup and readiness metrics SHALL be evaluated against configured startup budgets

### Requirement: Thread-safe service access
Service providers SHALL be safely accessible from multiple concurrent requests. The framework SHALL ensure that service loading is synchronized (preventing double-initialization) while request handling runs concurrently. Concurrency behavior SHALL satisfy required-lane throughput and contention budgets.

#### Scenario: Concurrent requests during service startup
- **WHEN** two requests arrive simultaneously for a service that hasn't been loaded yet
- **THEN** only one initialization SHALL occur; the second request SHALL wait for initialization to complete before being dispatched

#### Scenario: Concurrency contention budget is observable
- **WHEN** high-concurrency lanes execute full-service workloads
- **THEN** framework diagnostics SHALL expose contention-related metrics used for gate decisions
