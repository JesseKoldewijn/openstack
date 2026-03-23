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

### Requirement: Service provider trait
Each AWS service SHALL be implemented as a Rust struct that implements a generated provider trait. The trait SHALL define one async method per AWS API operation, accepting a `RequestContext` and typed input, returning a typed output or service-specific error.

#### Scenario: Provider implements operation
- **WHEN** the SQS provider receives a `CreateQueue` dispatch
- **THEN** the `SqsProvider::create_queue(&self, ctx, input)` method SHALL be called with the parsed `CreateQueueInput`

#### Scenario: Unimplemented operation
- **WHEN** a provider trait method is called that uses the default implementation
- **THEN** it SHALL return a `NotImplemented` error with the operation name

### Requirement: Lazy service loading
Services SHALL be loaded on first request by default (lazy loading). The `SERVICES` environment variable SHALL control which services are available. If `SERVICES` is set, only listed services SHALL be loadable.

#### Scenario: Default all services available
- **WHEN** `SERVICES` is not set
- **THEN** all implemented services SHALL be available for lazy loading

#### Scenario: Restricted service list
- **WHEN** `SERVICES=s3,sqs,dynamodb` is set
- **THEN** only S3, SQS, and DynamoDB SHALL be available; requests to other services SHALL return an error

### Requirement: Provider override support
The framework SHALL support `PROVIDER_OVERRIDE_<SERVICE>` environment variables to select alternative provider implementations for a service (e.g., `PROVIDER_OVERRIDE_DYNAMODB=v2`).

#### Scenario: Override selects alternative provider
- **WHEN** `PROVIDER_OVERRIDE_SQS=v2` is set and both `default` and `v2` SQS providers are registered
- **THEN** the `v2` provider SHALL be used for all SQS requests

#### Scenario: Override with unknown provider
- **WHEN** `PROVIDER_OVERRIDE_SQS=unknown` is set but no provider named `unknown` exists
- **THEN** the framework SHALL log an error and fall back to the `default` provider

### Requirement: Skeleton dispatch
The framework SHALL provide a `ServiceSkeleton` that maps AWS operation names to provider trait methods. The skeleton SHALL deserialize the `RequestContext` into typed inputs, call the provider method, and serialize the output into an HTTP response.

#### Scenario: Operation dispatched to provider
- **WHEN** a parsed request with operation `CreateQueue` reaches the SQS skeleton
- **THEN** the skeleton SHALL deserialize the request into `CreateQueueInput`, call `provider.create_queue()`, and serialize the `CreateQueueOutput` into the appropriate protocol response

#### Scenario: Provider returns error
- **WHEN** a provider method returns `Err(SqsError::QueueAlreadyExists(...))`
- **THEN** the skeleton SHALL serialize it as a `QueueAlreadyExists` AWS error response with the correct HTTP status code
