## ADDED Requirements

### Requirement: DispatchResponse SHALL support streaming response bodies
The `DispatchResponse` type SHALL provide a `ResponseBody` enum with two variants: `Buffered(Bytes)` for complete in-memory responses, and `Streaming` for responses that produce data incrementally via an async stream. The `Streaming` variant SHALL include an optional `content_length` for responses where the total size is known.

#### Scenario: Buffered response works as before
- **WHEN** a service returns `ResponseBody::Buffered(bytes)` from dispatch
- **THEN** the gateway sends the complete body as a single HTTP response, identical to current behavior

#### Scenario: Streaming response sends data incrementally
- **WHEN** a service returns `ResponseBody::Streaming` from dispatch
- **THEN** the gateway sends the response body as a chunked HTTP stream, reading from the async stream

#### Scenario: Streaming response with known content length
- **WHEN** a service returns `ResponseBody::Streaming` with `content_length: Some(1048576)`
- **THEN** the gateway sets the `Content-Length` header to `1048576` and streams the body

### Requirement: Existing DispatchResponse helpers SHALL return buffered responses
The `ok_json()` and `ok_xml()` convenience methods on `DispatchResponse` SHALL continue to return `ResponseBody::Buffered`, ensuring all existing service providers compile and function without changes.

#### Scenario: ok_json returns buffered
- **WHEN** a service calls `DispatchResponse::ok_json(value)`
- **THEN** the response body is `ResponseBody::Buffered` containing the serialized JSON bytes

#### Scenario: ok_xml returns buffered
- **WHEN** a service calls `DispatchResponse::ok_xml(xml_string)`
- **THEN** the response body is `ResponseBody::Buffered` containing the XML bytes
