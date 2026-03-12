## MODIFIED Requirements

### Requirement: Single-port HTTP gateway
The system SHALL expose all AWS services on a single HTTP port (default 4566) configurable via the `GATEWAY_LISTEN` environment variable. The gateway SHALL accept multiple comma-separated bind addresses (e.g., `0.0.0.0:4566,[::]:4566`).

The gateway SHALL also serve first-party Studio assets and Studio API endpoints under reserved internal namespaces without changing behavior of AWS-compatible service routes.

The Studio namespace SHALL serve bundled dashboard assets and SHALL continue SPA fallback semantics for client routes.

#### Scenario: Default port binding
- **WHEN** the server starts with no `GATEWAY_LISTEN` configured
- **THEN** it SHALL bind to `0.0.0.0:4566`

#### Scenario: Custom port binding
- **WHEN** `GATEWAY_LISTEN` is set to `127.0.0.1:5000`
- **THEN** the server SHALL bind to `127.0.0.1:5000`

#### Scenario: Multiple bind addresses
- **WHEN** `GATEWAY_LISTEN` is set to `0.0.0.0:4566,[::]:4566`
- **THEN** the server SHALL bind to both IPv4 and IPv6 on port 4566

#### Scenario: Studio routes are served from reserved namespace
- **WHEN** a client requests `/_localstack/studio` or `/_localstack/studio/*`
- **THEN** the gateway SHALL serve Studio assets or SPA fallback responses from the reserved namespace

#### Scenario: AWS routes remain unaffected by Studio routing
- **WHEN** a client requests standard AWS-compatible paths or host-based service routes
- **THEN** the gateway SHALL dispatch to service providers exactly as before and SHALL NOT route those requests to Studio handlers

#### Scenario: Studio dashboard assets are cache-safe and discoverable
- **WHEN** Studio dashboard assets are requested under `/_localstack/studio/assets/*`
- **THEN** the gateway SHALL return correct content types and cache behavior compatible with dashboard runtime loading

### Requirement: Handler chain pipeline
Ordered handler chain: inspect/modify request context, short-circuit, or pass. Supports request handlers, response handlers, exception handlers, and finalizers. Must satisfy allocation and overhead budgets for required service-class performance targets.

The gateway SHALL support streaming response bodies from service providers. When a `DispatchResponse` contains a `ResponseBody::Streaming` variant, the gateway SHALL convert it to a streamed HTTP response using `Body::from_stream()` instead of `Body::from(bytes)`. The gateway SHALL set the `Content-Length` header when the streaming response provides a known content length. The gateway SHALL continue to support `ResponseBody::Buffered` responses identically to current behavior.

The gateway SHALL stream incoming request bodies into a `SpooledBody` rather than calling `axum::body::to_bytes()` to buffer the entire body upfront. For services that support streaming ingestion (S3), the gateway SHALL provide access to the body stream for direct-to-disk streaming.

The gateway SHALL route internal namespaces in deterministic precedence order so `/_localstack/studio/*` and `/_localstack/studio-api/*` are handled by Studio/internal handlers before generic AWS service detection.

The gateway SHALL enforce Studio guided-flow safety guardrails for Studio API execution traffic, including method allow-listing and payload-bound constraints for guided-flow execution endpoints.

- **Scenario: Request flows through handler chain** - Order: content decoding -> service detection -> request parsing -> auth extraction -> region extraction -> service dispatch -> response serialization.
- **Scenario: Handler short-circuits the chain** - CORS preflight `OPTIONS` returns immediately, skips dispatch.
- **Scenario: Request path budget regression is detectable** - Latency/allocation budget breaches are attributed to gateway-core path metrics.
- **Scenario: Streaming response is delivered via chunked transfer** - When a service provider returns `ResponseBody::Streaming`, the gateway delivers the response as a streamed HTTP body, and the client receives data incrementally.
- **Scenario: Buffered response is delivered as before** - When a service provider returns `ResponseBody::Buffered`, the gateway delivers the response identically to current behavior (single `Body::from(bytes)`).
- **Scenario: Request body is spooled not fully buffered** - When a request arrives, the gateway streams the body into a `SpooledBody` instead of calling `to_bytes()`, and the `SpooledBody` is passed to the service provider via `RequestContext`.
- **Scenario: Studio API route bypasses AWS service inference** - When a client requests `/_localstack/studio-api/services`, the gateway routes to Studio/internal API handling and does not attempt AWS protocol/service inference.
- **Scenario: Studio guided execution rejects disallowed method** - When a Studio guided execution endpoint receives a disallowed HTTP method, the gateway/internal path rejects the request with method-not-allowed semantics.
- **Scenario: Studio guided execution enforces payload bounds** - When a Studio guided execution request exceeds configured payload limits, the gateway rejects the request with payload-too-large semantics and does not dispatch to service providers.
