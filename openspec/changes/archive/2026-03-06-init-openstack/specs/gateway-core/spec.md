## ADDED Requirements

### Requirement: Single-port HTTP gateway
The system SHALL expose all AWS services on a single HTTP port (default 4566) configurable via the `GATEWAY_LISTEN` environment variable. The gateway SHALL accept multiple comma-separated bind addresses (e.g., `0.0.0.0:4566,[::]:4566`).

#### Scenario: Default port binding
- **WHEN** the server starts with no `GATEWAY_LISTEN` configured
- **THEN** it SHALL bind to `0.0.0.0:4566`

#### Scenario: Custom port binding
- **WHEN** `GATEWAY_LISTEN` is set to `127.0.0.1:5000`
- **THEN** the server SHALL bind to `127.0.0.1:5000`

#### Scenario: Multiple bind addresses
- **WHEN** `GATEWAY_LISTEN` is set to `0.0.0.0:4566,[::]:4566`
- **THEN** the server SHALL bind to both IPv4 and IPv6 on port 4566

### Requirement: AWS service detection from request
The gateway SHALL determine which AWS service a request targets by examining (in priority order): (1) the `Authorization` header's credential scope, (2) the `Host` header (e.g., `sqs.us-east-1.localhost.localstack.cloud`), (3) the `X-Amz-Target` header, (4) URL path patterns.

#### Scenario: Service detection via Authorization header
- **WHEN** a request arrives with `Authorization: AWS4-HMAC-SHA256 Credential=.../us-east-1/sqs/aws4_request`
- **THEN** the gateway SHALL route the request to the SQS service provider

#### Scenario: Service detection via Host header
- **WHEN** a request arrives with `Host: s3.us-east-1.localhost.localstack.cloud:4566`
- **THEN** the gateway SHALL route the request to the S3 service provider

#### Scenario: Service detection via X-Amz-Target
- **WHEN** a request arrives with header `X-Amz-Target: DynamoDB_20120810.GetItem`
- **THEN** the gateway SHALL route the request to the DynamoDB service provider

### Requirement: AWS protocol support
The gateway SHALL parse and serialize requests/responses for all five AWS protocols: `json`, `query`, `rest-json`, `rest-xml`, and `ec2`.

#### Scenario: Query protocol (SQS)
- **WHEN** an SQS `CreateQueue` request arrives with `Action=CreateQueue` as a query parameter
- **THEN** the gateway SHALL parse it using the query protocol and return an XML response

#### Scenario: REST-XML protocol (S3)
- **WHEN** an S3 `PutObject` request arrives as `PUT /bucket/key`
- **THEN** the gateway SHALL parse it using the rest-xml protocol

#### Scenario: JSON protocol (DynamoDB)
- **WHEN** a DynamoDB `GetItem` request arrives with JSON body and `X-Amz-Target` header
- **THEN** the gateway SHALL parse it using the json protocol and return a JSON response

#### Scenario: REST-JSON protocol (API Gateway)
- **WHEN** an API Gateway request arrives as a RESTful JSON request
- **THEN** the gateway SHALL parse it using the rest-json protocol

### Requirement: Handler chain pipeline
The gateway SHALL process requests through an ordered handler chain where each handler can inspect/modify the request context, short-circuit the chain, or pass to the next handler. The chain SHALL support request handlers, response handlers, exception handlers, and finalizers.

#### Scenario: Request flows through handler chain
- **WHEN** a valid AWS request arrives
- **THEN** it SHALL pass through: content decoding, service detection, request parsing, auth extraction, region extraction, service dispatch, and response serialization -- in that order

#### Scenario: Handler short-circuits the chain
- **WHEN** the CORS preflight handler detects an `OPTIONS` request
- **THEN** it SHALL return the CORS response immediately without invoking service dispatch

### Requirement: SigV4 auth parsing
The gateway SHALL parse AWS Signature Version 4 `Authorization` headers to extract the access key ID, region, service name, and signed headers. The gateway SHALL NOT validate signatures (all requests are accepted).

#### Scenario: Extract account context from SigV4
- **WHEN** a request has `Authorization: AWS4-HMAC-SHA256 Credential=AKID123/20260306/us-east-1/s3/aws4_request`
- **THEN** the gateway SHALL extract access key `AKID123`, region `us-east-1`, and service `s3`

#### Scenario: Missing Authorization header
- **WHEN** a request arrives without an `Authorization` header
- **THEN** the gateway SHALL inject a default authorization with account `000000000000` and region `us-east-1`

### Requirement: CORS handling
The gateway SHALL add CORS headers to all responses. CORS behavior SHALL be configurable via `DISABLE_CORS_HEADERS`, `DISABLE_CORS_CHECKS`, `EXTRA_CORS_ALLOWED_ORIGINS`, and `EXTRA_CORS_ALLOWED_HEADERS` environment variables.

#### Scenario: CORS preflight request
- **WHEN** an `OPTIONS` request arrives with `Origin` and `Access-Control-Request-Method` headers
- **THEN** the gateway SHALL respond with `200 OK` and appropriate `Access-Control-Allow-*` headers

#### Scenario: CORS disabled
- **WHEN** `DISABLE_CORS_HEADERS=1` is set
- **THEN** the gateway SHALL NOT add any CORS headers to responses

### Requirement: Region handling
The gateway SHALL extract the AWS region from the Authorization header's credential scope. If the region is not a valid AWS region, the gateway SHALL fall back to `us-east-1` unless `ALLOW_NONSTANDARD_REGIONS=1` is set.

#### Scenario: Standard region extraction
- **WHEN** the credential scope contains `us-west-2`
- **THEN** the gateway SHALL set the request context region to `us-west-2`

#### Scenario: Invalid region falls back
- **WHEN** the credential scope contains `my-custom-region` and `ALLOW_NONSTANDARD_REGIONS` is not set
- **THEN** the gateway SHALL fall back to `us-east-1` with a warning log

#### Scenario: Non-standard region allowed
- **WHEN** the credential scope contains `my-custom-region` and `ALLOW_NONSTANDARD_REGIONS=1`
- **THEN** the gateway SHALL accept `my-custom-region` as the region
