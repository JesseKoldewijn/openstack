## ADDED Requirements

### Requirement: Service provider trait
Each AWS service SHALL be implemented as a Rust struct that implements a generated provider trait. The trait SHALL define one async method per AWS API operation, accepting a `RequestContext` and typed input, returning a typed output or service-specific error.

#### Scenario: Provider implements operation
- **WHEN** the SQS provider receives a `CreateQueue` dispatch
- **THEN** the `SqsProvider::create_queue(&self, ctx, input)` method SHALL be called with the parsed `CreateQueueInput`

#### Scenario: Unimplemented operation
- **WHEN** a provider trait method is called that uses the default implementation
- **THEN** it SHALL return a `NotImplemented` error with the operation name

### Requirement: Service lifecycle management
Each service provider SHALL have a lifecycle with states: `Available`, `Starting`, `Running`, `Stopping`, `Stopped`, `Error`. The framework SHALL manage transitions and expose the current state.

#### Scenario: Service starts on first request
- **WHEN** a request targets a service that is in `Available` state and `EAGER_SERVICE_LOADING` is not set
- **THEN** the framework SHALL transition the service to `Starting`, invoke its `start()` method, then transition to `Running` before dispatching the request

#### Scenario: Eager service loading
- **WHEN** `EAGER_SERVICE_LOADING=1` is set
- **THEN** all configured services SHALL be started during server startup, before accepting requests

#### Scenario: Service start failure
- **WHEN** a service's `start()` method returns an error
- **THEN** the service SHALL transition to `Error` state and subsequent requests SHALL return `503 Service Unavailable`

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

### Requirement: Thread-safe service access
Service providers SHALL be safely accessible from multiple concurrent requests. The framework SHALL ensure that service loading is synchronized (preventing double-initialization) while request handling runs concurrently.

#### Scenario: Concurrent requests during service startup
- **WHEN** two requests arrive simultaneously for a service that hasn't been loaded yet
- **THEN** only one initialization SHALL occur; the second request SHALL wait for initialization to complete before being dispatched
