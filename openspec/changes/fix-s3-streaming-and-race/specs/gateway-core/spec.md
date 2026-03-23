## MODIFIED Requirements

### Requirement: Handler chain pipeline
Ordered handler chain: inspect/modify request context, short-circuit, or pass. Supports request handlers, response handlers, exception handlers, and finalizers. Must satisfy allocation and overhead budgets for required service-class performance targets.

The gateway SHALL support streaming response bodies from service providers. When a `DispatchResponse` contains a `ResponseBody::Streaming` variant, the gateway SHALL convert it to a streamed HTTP response using `Body::from_stream()` instead of `Body::from(bytes)`. The gateway SHALL set the `Content-Length` header when the streaming response provides a known content length. The gateway SHALL continue to support `ResponseBody::Buffered` responses identically to current behavior.

The gateway SHALL stream incoming request bodies into a `SpooledBody` and SHALL NOT call `to_bytes()` to fully buffer the body in memory. The `SpooledBody` SHALL be passed to service providers via `RequestContext`. Services that require the full body as `Bytes` SHALL obtain it by calling a lazy materialization accessor on `RequestContext`, which reads from the `SpooledBody` on first access and caches the result. Services that do not need the raw body SHALL NOT pay any materialization cost.

The gateway SHALL route internal namespaces in deterministic precedence order so `/_localstack/studio/*` and `/_localstack/studio-api/*` are handled by Studio/internal handlers before generic AWS service detection.

The gateway SHALL enforce Studio guided-flow safety guardrails for Studio API execution traffic, including method allow-listing and payload-bound constraints for guided-flow execution endpoints.

- **Scenario: Request flows through handler chain** - Order: content decoding -> service detection -> request parsing -> auth extraction -> region extraction -> service dispatch -> response serialization.
- **Scenario: Handler short-circuits the chain** - CORS preflight `OPTIONS` returns immediately, skips dispatch.
- **Scenario: Request path budget regression is detectable** - Latency/allocation budget breaches are attributed to gateway-core path metrics.
- **Scenario: Streaming response is delivered via chunked transfer** - When a service provider returns `ResponseBody::Streaming`, the gateway delivers the response as a streamed HTTP body, and the client receives data incrementally.
- **Scenario: Buffered response is delivered as before** - When a service provider returns `ResponseBody::Buffered`, the gateway delivers the response identically to current behavior (single `Body::from(bytes)`).
- **Scenario: Studio API route bypasses AWS service inference** - When a client requests `/_localstack/studio-api/services`, the gateway routes to Studio/internal API handling and does not attempt AWS protocol/service inference.
- **Scenario: Studio guided execution rejects disallowed method** - When a Studio guided execution endpoint receives a disallowed HTTP method, the gateway/internal path rejects the request with method-not-allowed semantics.
- **Scenario: Studio guided execution enforces payload bounds** - When a Studio guided execution request exceeds configured payload limits, the gateway rejects the request with payload-too-large semantics and does not dispatch to service providers.

#### Scenario: Request body is spooled not fully buffered
- **WHEN** a request arrives with any body size
- **THEN** the gateway streams the body into a `SpooledBody` and SHALL NOT call `to_bytes()` or any equivalent that allocates the full body in heap memory before dispatch

#### Scenario: Service that needs raw bytes gets lazy materialization
- **WHEN** a service calls the `RequestContext` raw body accessor for the first time
- **THEN** the `SpooledBody` is read into a `Bytes` value, the result is cached on the context, and subsequent calls return the cached value without re-reading

#### Scenario: Service that does not use raw body pays zero allocation
- **WHEN** a JSON-protocol service (e.g., DynamoDB) handles a request
- **THEN** `RequestContext.raw_body` is never materialized, and no heap allocation for the body beyond the spool buffer occurs during dispatch

#### Scenario: Large S3 PutObject body does not appear in loaded RSS
- **WHEN** a PutObject request arrives with a 100 MiB body
- **THEN** after dispatch completes, process RSS does not include a 100 MiB byte buffer; the body has been streamed to disk and the spool file released
