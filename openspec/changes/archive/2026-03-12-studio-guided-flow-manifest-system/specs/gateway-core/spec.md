## MODIFIED Requirements

### Requirement: Handler chain pipeline
Ordered handler chain: inspect/modify request context, short-circuit, or pass. Supports request handlers, response handlers, exception handlers, and finalizers. Must satisfy allocation and overhead budgets for required service-class performance targets.

The gateway SHALL support streaming response bodies from service providers. When a `DispatchResponse` contains a `ResponseBody::Streaming` variant, the gateway SHALL convert it to a streamed HTTP response using `Body::from_stream()` instead of `Body::from(bytes)`. The gateway SHALL set the `Content-Length` header when the streaming response provides a known content length. The gateway SHALL continue to support `ResponseBody::Buffered` responses identically to current behavior.

The gateway SHALL stream incoming request bodies into a `SpooledBody` rather than calling `axum::body::to_bytes()` to buffer the entire body upfront. For services that support streaming ingestion (S3), the gateway SHALL provide access to the body stream for direct-to-disk streaming.

The gateway SHALL route internal namespaces in deterministic precedence order so `/_localstack/studio/*` and `/_localstack/studio-api/*` are handled by Studio/internal handlers before generic AWS service detection.

The gateway SHALL enforce Studio guided-flow safety guardrails for Studio API execution traffic, including method allow-listing and payload-bound constraints for guided-flow execution endpoints.

#### Scenario: Request flows through handler chain
- **WHEN** a standard AWS request is received
- **THEN** request handling order SHALL remain content decoding -> service detection -> request parsing -> auth extraction -> region extraction -> service dispatch -> response serialization

#### Scenario: Handler short-circuits the chain
- **WHEN** a CORS preflight `OPTIONS` request arrives
- **THEN** the gateway SHALL return immediately without dispatching to service providers

#### Scenario: Request path budget regression is detectable
- **WHEN** gateway path latency/allocation budgets are exceeded
- **THEN** metrics and diagnostics SHALL attribute regressions to gateway-core request path stages

#### Scenario: Streaming response is delivered via chunked transfer
- **WHEN** a service provider returns `ResponseBody::Streaming`
- **THEN** the gateway SHALL deliver the response as a streamed HTTP body with incremental client-visible chunks

#### Scenario: Buffered response is delivered as before
- **WHEN** a service provider returns `ResponseBody::Buffered`
- **THEN** the gateway SHALL deliver the response with existing buffered behavior

#### Scenario: Request body is spooled not fully buffered
- **WHEN** a request arrives
- **THEN** the gateway SHALL spool request bytes into `SpooledBody` and pass it via request context instead of fully buffering via `to_bytes()`

#### Scenario: Studio API route bypasses AWS service inference
- **WHEN** a client requests `/_localstack/studio-api/services`
- **THEN** the gateway SHALL route to Studio/internal API handling and SHALL NOT attempt AWS protocol/service inference for that request

#### Scenario: Studio guided execution rejects disallowed method
- **WHEN** a Studio guided execution endpoint receives a disallowed HTTP method
- **THEN** the gateway/internal path SHALL reject the request with explicit method-not-allowed semantics

#### Scenario: Studio guided execution enforces payload bounds
- **WHEN** a Studio guided execution request exceeds configured payload limits
- **THEN** the request SHALL be rejected with explicit payload-too-large semantics and without dispatching to service providers
