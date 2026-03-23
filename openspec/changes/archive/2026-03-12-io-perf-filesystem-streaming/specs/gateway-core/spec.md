## MODIFIED Requirements

### Requirement: Handler chain pipeline
Ordered handler chain: inspect/modify request context, short-circuit, or pass. Supports request handlers, response handlers, exception handlers, and finalizers. Must satisfy allocation and overhead budgets for required service-class performance targets.

The gateway SHALL support streaming response bodies from service providers. When a `DispatchResponse` contains a `ResponseBody::Streaming` variant, the gateway SHALL convert it to a streamed HTTP response using `Body::from_stream()` instead of `Body::from(bytes)`. The gateway SHALL set the `Content-Length` header when the streaming response provides a known content length. The gateway SHALL continue to support `ResponseBody::Buffered` responses identically to current behavior.

The gateway SHALL stream incoming request bodies into a `SpooledBody` rather than calling `axum::body::to_bytes()` to buffer the entire body upfront. For services that support streaming ingestion (S3), the gateway SHALL provide access to the body stream for direct-to-disk streaming.

- **Scenario: Request flows through handler chain** - Order: content decoding -> service detection -> request parsing -> auth extraction -> region extraction -> service dispatch -> response serialization.
- **Scenario: Handler short-circuits the chain** - CORS preflight `OPTIONS` returns immediately, skips dispatch.
- **Scenario: Request path budget regression is detectable** - Latency/allocation budget breaches are attributed to gateway-core path metrics.
- **Scenario: Streaming response is delivered via chunked transfer** - When a service provider returns `ResponseBody::Streaming`, the gateway delivers the response as a streamed HTTP body, and the client receives data incrementally.
- **Scenario: Buffered response is delivered as before** - When a service provider returns `ResponseBody::Buffered`, the gateway delivers the response identically to current behavior (single `Body::from(bytes)`).
- **Scenario: Request body is spooled not fully buffered** - When a request arrives, the gateway streams the body into a `SpooledBody` instead of calling `to_bytes()`, and the `SpooledBody` is passed to the service provider via `RequestContext`.
