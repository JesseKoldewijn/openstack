## Why

LocalStack is the de facto standard for local AWS development, but it is written in Python, carries significant memory overhead (~500MB+), has slow cold-start times, and suffers from Python's GIL limitations under concurrent workloads. A Rust implementation ("openstack") would deliver dramatically lower memory usage, faster startup, native concurrency, and a single static binary distribution -- while remaining a 100% API-compatible drop-in replacement that works with existing AWS SDKs, Terraform, CDK, and the `localstack` CLI.

## What Changes

- Create a new Rust project (`openstack`) that emulates the full LocalStack Community Edition AWS API surface
- Implement the core HTTP gateway on port 4566 with AWS request parsing (SigV4, protocol detection, service routing)
- Build a plugin-based service provider framework mirroring LocalStack's ASF architecture
- Implement all Community Edition AWS service emulations (S3, SQS, SNS, DynamoDB, Lambda, IAM, STS, KMS, CloudFormation, CloudWatch, Kinesis, EventBridge, Step Functions, API Gateway, EC2, Route53, SES, SSM, Secrets Manager, ACM, and more)
- Implement state persistence with snapshot save/load strategies compatible with LocalStack's persistence model
- Support multi-account and multi-region isolation with the same scoping semantics (local, cross-region, cross-account attributes)
- Expose identical internal APIs (`/_localstack/health`, `/_localstack/info`, `/_localstack/init`, etc.)
- Support all LocalStack environment variables (`GATEWAY_LISTEN`, `PERSISTENCE`, `SERVICES`, `PROVIDER_OVERRIDE_*`, etc.)
- Provide both a native binary and Docker image distribution
- Support init scripts (`/etc/localstack/init/{boot,start,ready,shutdown}.d/`)

## Capabilities

### New Capabilities
- `gateway-core`: HTTP gateway on port 4566, AWS SigV4 auth parsing, service/operation detection from headers and paths, handler chain pipeline, CORS handling, request/response serialization for all AWS protocols (json, query, rest-json, rest-xml, ec2)
- `service-framework`: Plugin-based service provider architecture with lazy loading, lifecycle management (start/stop/check), service state tracking, provider override support, and skeleton dispatch mapping operations to handler methods
- `core-aws-services`: Emulation of foundational AWS services -- S3, SQS, SNS, DynamoDB, DynamoDB Streams, Lambda (with Docker-based execution), Kinesis, and Firehose
- `extended-aws-services`: Emulation of IAM, STS, KMS, CloudFormation, CloudWatch, CloudWatch Logs, EventBridge, Step Functions, API Gateway, EC2, Route53, SSM, Secrets Manager, SES, ACM, ECR, Redshift, OpenSearch, and remaining Community Edition services
- `state-persistence`: State container model (AccountRegionBundle/RegionBundle/Store), persistence to disk, snapshot strategies (ON_SHUTDOWN, ON_STARTUP, ON_REQUEST, SCHEDULED, MANUAL), and state lifecycle hooks
- `multi-tenancy`: Multi-account isolation via access key to account ID mapping, multi-region isolation, attribute scoping (LocalAttribute, CrossRegionAttribute, CrossAccountAttribute), default account `000000000000`
- `internal-api`: Health endpoint (`/_localstack/health`), info endpoint (`/_localstack/info`), init script runner, plugins endpoint, diagnostics, config endpoint, and usage reporting
- `compatibility-layer`: Full environment variable compatibility, CLI response format compatibility, Docker image with identical entrypoints, DNS server for `*.localhost.localstack.cloud` resolution, external service port range (4510-4560)

### Modified Capabilities
(none -- this is a greenfield project)

## Impact

- **New codebase**: Entire Rust project created from scratch (~50k+ LOC estimated)
- **Dependencies**: Rust ecosystem (tokio, hyper, axum/actix-web, serde, aws-smithy models)
- **Docker**: New Dockerfile for the Rust binary, compatible with existing `docker-compose.yml` setups
- **Testing**: Needs comprehensive integration tests that validate API compatibility against AWS SDK calls -- ideally reusing LocalStack's own test suite patterns
- **Users**: Zero-config migration for existing LocalStack users -- same port, same env vars, same API surface
