## MODIFIED Requirements

### Requirement: Handler chain pipeline
The gateway SHALL process requests through an ordered handler chain where each handler can inspect/modify the request context, short-circuit the chain, or pass to the next handler. The chain SHALL support request handlers, response handlers, exception handlers, and finalizers. The request path SHALL satisfy defined allocation and overhead budgets compatible with required service-class performance targets.

#### Scenario: Request flows through handler chain
- **WHEN** a valid AWS request arrives
- **THEN** it SHALL pass through: content decoding, service detection, request parsing, auth extraction, region extraction, service dispatch, and response serialization -- in that order

#### Scenario: Handler short-circuits the chain
- **WHEN** the CORS preflight handler detects an `OPTIONS` request
- **THEN** it SHALL return the CORS response immediately without invoking service dispatch

#### Scenario: Request path budget regression is detectable
- **WHEN** gateway request-path latency or allocation budget exceeds configured thresholds in required lanes
- **THEN** benchmark and gate diagnostics SHALL attribute the regression to gateway-core path metrics
