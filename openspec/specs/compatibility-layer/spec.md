## MODIFIED Requirements

### Requirement: Environment variable compatibility
The system SHALL support all LocalStack Community Edition environment variables with identical names and semantics. At minimum: `GATEWAY_LISTEN`, `LOCALSTACK_HOST`, `PERSISTENCE`, `DEBUG`, `LS_LOG`, `SERVICES`, `EAGER_SERVICE_LOADING`, `USE_SSL`, `MAIN_CONTAINER_NAME`, `ALLOW_NONSTANDARD_REGIONS`, `GATEWAY_WORKER_COUNT`, `DISABLE_CORS_HEADERS`, `DISABLE_CORS_CHECKS`, `EXTRA_CORS_ALLOWED_ORIGINS`, `EXTRA_CORS_ALLOWED_HEADERS`, `DOCKER_SOCK`, `LAMBDA_DOCKER_NETWORK`, `LAMBDA_DOCKER_FLAGS`, `LAMBDA_REMOVE_CONTAINERS`, `LAMBDA_KEEPALIVE_MS`, `LAMBDA_RUNTIME_ENVIRONMENT_TIMEOUT`, `BUCKET_MARKER_LOCAL`, `SNAPSHOT_SAVE_STRATEGY`, `SNAPSHOT_LOAD_STRATEGY`, `SNAPSHOT_FLUSH_INTERVAL`, `DNS_ADDRESS`, `DNS_PORT`, `DNS_RESOLVE_IP`, `ENABLE_CONFIG_UPDATES`, `FAIL_FAST`, and `PROVIDER_OVERRIDE_*`.

#### Scenario: GATEWAY_LISTEN controls bind address
- **WHEN** `GATEWAY_LISTEN=0.0.0.0:5000` is set
- **THEN** the server SHALL listen on port 5000 instead of 4566

#### Scenario: DEBUG enables verbose logging
- **WHEN** `DEBUG=1` is set
- **THEN** the server SHALL output debug-level logs and enable diagnostic endpoints

#### Scenario: SERVICES restricts available services
- **WHEN** `SERVICES=s3,sqs` is set
- **THEN** only S3 and SQS SHALL be available; requests to DynamoDB SHALL return an error

#### Scenario: Environment variable behavior is verified by parity harness
- **WHEN** compatibility behavior that depends on environment variables is covered by parity scenarios
- **THEN** the parity harness SHALL compare openstack and LocalStack outcomes and report regressions in CI-visible parity results

### Requirement: URL format compatibility
The system SHALL return URLs in API responses using the format `http://{LOCALSTACK_HOST}/{path}` (or `https://` when `USE_SSL=1`). Service-specific URLs (e.g., SQS queue URLs) SHALL use the same format as LocalStack: `http://{service}.{region}.localhost.localstack.cloud:{port}/{account}/{resource}`.

#### Scenario: SQS queue URL format
- **WHEN** a queue `my-queue` is created with default settings
- **THEN** the queue URL SHALL be `http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/my-queue`

#### Scenario: Custom LOCALSTACK_HOST
- **WHEN** `LOCALSTACK_HOST=myhost:4566` is set
- **THEN** URLs in responses SHALL use `myhost:4566` as the host

### Requirement: Docker image compatibility
The system SHALL be distributable as a Docker image with the same entrypoint behavior as LocalStack's Docker image. The image SHALL support volume mounts at `/var/lib/localstack` for state persistence and `/etc/localstack/init` for init scripts.

#### Scenario: Docker volume mount for persistence
- **WHEN** the Docker container is started with `-v ./data:/var/lib/localstack` and `PERSISTENCE=1`
- **THEN** state SHALL be persisted to the host's `./data` directory

#### Scenario: Docker init scripts
- **WHEN** the container is started with `-v ./init:/etc/localstack/init`
- **THEN** scripts in the mounted directory SHALL be executed at the appropriate lifecycle stages

### Requirement: Native binary distribution
The system SHALL be compilable to a single statically-linked binary that runs without Docker. The binary SHALL accept the same environment variables and behave identically to the Docker deployment.

#### Scenario: Run as native binary
- **WHEN** the `openstack` binary is executed directly
- **THEN** the server SHALL start on the configured port with all configured services available

### Requirement: DNS server for service hostnames
The system SHALL optionally run an embedded DNS server (configurable via `DNS_ADDRESS` and `DNS_PORT`) that resolves `*.localhost.localstack.cloud` to the IP specified by `DNS_RESOLVE_IP` (default `127.0.0.1`).

#### Scenario: DNS resolves service hostname
- **WHEN** the DNS server is running and a client queries `sqs.us-east-1.localhost.localstack.cloud`
- **THEN** the DNS server SHALL respond with the configured resolve IP (default `127.0.0.1`)

#### Scenario: DNS disabled
- **WHEN** `DNS_ADDRESS=0` is set
- **THEN** the DNS server SHALL NOT be started

### Requirement: External service port range
The system SHALL reserve port range 4510-4560 for external service processes (configurable via `EXTERNAL_SERVICE_PORTS_START` and `EXTERNAL_SERVICE_PORTS_END`). Services that require external processes (e.g., OpenSearch) SHALL allocate ports from this range.

#### Scenario: OpenSearch uses external port
- **WHEN** an OpenSearch domain is created
- **THEN** the system SHALL allocate a port from the 4510-4560 range for the OpenSearch process

### Requirement: Logging compatibility
The system SHALL use structured logging configurable via `LS_LOG` environment variable with levels: `trace`, `trace-internal`, `debug`, `info`, `warn`, `error`. Request/response logging SHALL include the AWS service, operation, account ID, region, status code, and latency.

#### Scenario: Log level configuration
- **WHEN** `LS_LOG=debug` is set
- **THEN** debug-level and above log messages SHALL be output

#### Scenario: Request logging
- **WHEN** a request is processed
- **THEN** a log entry SHALL be emitted with the service name, operation, status code, and response time

### Requirement: Graceful shutdown
The system SHALL handle SIGTERM and SIGINT by: (1) stopping acceptance of new connections, (2) running shutdown init scripts, (3) saving state if persistence is enabled, (4) stopping all services, and (5) exiting cleanly.

#### Scenario: Graceful shutdown on SIGTERM
- **WHEN** SIGTERM is sent to the process while persistence is enabled and there is in-memory state
- **THEN** the system SHALL save state to disk, run shutdown scripts, and exit with code 0

### Requirement: Differential compatibility verification in CI
The system SHALL run a parity harness in CI that compares openstack and LocalStack behavior for a defined core compatibility profile and surfaces regressions as CI-visible failures.

#### Scenario: Core parity profile is executed on pull requests
- **WHEN** a pull request modifies compatibility-relevant behavior
- **THEN** CI SHALL run the core parity profile and publish pass/fail parity results

#### Scenario: Parity regression blocks required checks
- **WHEN** a non-accepted parity mismatch is detected in the required profile
- **THEN** the CI parity check SHALL fail and block merge until resolved or explicitly accepted

### Requirement: Accepted-difference policy and traceability
The system SHALL maintain an explicit, versioned accepted-differences policy for compatibility mismatches, including rationale, scope, and review metadata.

#### Scenario: Accepted mismatch includes explicit policy metadata
- **WHEN** a parity mismatch is classified as accepted
- **THEN** the policy entry SHALL include scope, rationale, owner/reviewer, and review/expiry metadata

#### Scenario: Unsupported acceptance entry cannot silently mask regressions
- **WHEN** an acceptance entry is malformed, missing required metadata, or expired
- **THEN** the matching parity mismatch SHALL fail CI until the policy entry is corrected
