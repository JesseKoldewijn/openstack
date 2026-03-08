## 1. Project Scaffolding

- [x] 1.1 Initialize Cargo workspace with root `Cargo.toml` defining all member crates
- [x] 1.2 Create crate skeleton for `crates/openstack` (binary entry point with main.rs)
- [x] 1.3 Create crate skeleton for `crates/config` (env var parsing, configuration structs)
- [x] 1.4 Create crate skeleton for `crates/gateway` (HTTP server, handler chain)
- [x] 1.5 Create crate skeleton for `crates/aws-protocol` (AWS request/response parsing)
- [x] 1.6 Create crate skeleton for `crates/service-framework` (provider traits, lifecycle)
- [x] 1.7 Create crate skeleton for `crates/state` (AccountRegionBundle, persistence)
- [x] 1.8 Create crate skeleton for `crates/internal-api` (/_localstack/* endpoints)
- [x] 1.9 Create crate skeleton for `crates/dns` (embedded DNS server)
- [x] 1.10 Add shared dependencies to workspace: tokio, axum, hyper, serde, serde_json, tracing, dashmap, bollard, hickory-dns
- [x] 1.11 Set up workspace-level rustfmt.toml and clippy configuration
- [x] 1.12 Create initial Dockerfile for multi-stage build (builder + minimal runtime image)
- [x] 1.13 Create `.gitignore` for Rust/Cargo artifacts

## 2. Configuration System

- [x] 2.1 Implement env var parser for all LocalStack-compatible variables (GATEWAY_LISTEN, PERSISTENCE, SERVICES, DEBUG, LS_LOG, etc.)
- [x] 2.2 Implement `Directories` struct with paths for state, cache, tmp, logs, init scripts (matching LocalStack's layout)
- [x] 2.3 Implement `SERVICES` parsing to enable/disable specific services
- [x] 2.4 Implement `PROVIDER_OVERRIDE_*` env var parsing
- [x] 2.5 Implement `LOCALSTACK_HOST` and URL generation utilities
- [x] 2.6 Implement log level configuration from `LS_LOG` and `DEBUG` env vars using tracing-subscriber
- [x] 2.7 Add unit tests for all config parsing (defaults, overrides, edge cases)

## 3. AWS Protocol Layer

- [ ] 3.1 Obtain and vendor AWS Smithy/botocore JSON service model files for all target services
- [ ] 3.2 Implement build.rs code generator that reads Smithy models and generates Rust types (input/output structs, error enums) for each service
- [x] 3.3 Implement query protocol request parser (form-urlencoded body with Action parameter)
- [x] 3.4 Implement query protocol response serializer (XML response format)
- [x] 3.5 Implement json protocol request parser (JSON body with X-Amz-Target header)
- [x] 3.6 Implement json protocol response serializer (JSON response)
- [x] 3.7 Implement rest-json protocol request parser (RESTful paths with JSON body)
- [x] 3.8 Implement rest-json protocol response serializer
- [x] 3.9 Implement rest-xml protocol request parser (RESTful paths with XML body, used by S3)
- [x] 3.10 Implement rest-xml protocol response serializer
- [x] 3.11 Implement ec2 protocol request parser (variant of query protocol)
- [x] 3.12 Implement ec2 protocol response serializer
- [x] 3.13 Implement AWS error response serialization for each protocol type
- [x] 3.14 Add integration tests: round-trip parse/serialize for each protocol using real AWS SDK request samples

## 4. Gateway Core

- [x] 4.1 Implement the HTTP server using axum + hyper with configurable bind addresses from GATEWAY_LISTEN
- [x] 4.2 Implement the handler chain framework (ordered pipeline of request handlers, response handlers, exception handlers, finalizers)
- [x] 4.3 Implement `RequestContext` struct carrying: request, service model, operation, region, account_id, protocol, service_request, service_response
- [x] 4.4 Implement `ServiceNameParser` handler: detect target service from Authorization header credential scope, Host header, X-Amz-Target header, and URL path patterns
- [x] 4.5 Implement `ServiceRequestParser` handler: parse AWS operation and parameters from the raw HTTP request using the protocol layer
- [x] 4.6 Implement SigV4 `Authorization` header parser to extract access key, region, service (no signature validation)
- [x] 4.7 Implement `AccountIdEnricher` handler: derive account ID from access key
- [x] 4.8 Implement `RegionContextEnricher` handler: extract region from credential scope with fallback to us-east-1
- [x] 4.9 Implement `RegionRewriter` handler: validate region, fall back to us-east-1 unless ALLOW_NONSTANDARD_REGIONS=1
- [x] 4.10 Implement CORS handler: preflight OPTIONS responses, add CORS headers to all responses, respect DISABLE_CORS_* and EXTRA_CORS_* env vars
- [x] 4.11 Implement `ServiceRequestRouter`: dispatch parsed requests to the correct service skeleton
- [x] 4.12 Implement `ServiceExceptionSerializer`: serialize service errors into protocol-appropriate HTTP error responses
- [x] 4.13 Implement response logging handler (service, operation, status code, latency)
- [x] 4.14 Implement default authorization injection for requests missing Authorization header
- [x] 4.15 Implement pre-signed URL detection and handling for S3
- [x] 4.16 Implement graceful shutdown (SIGTERM/SIGINT handling, drain connections, run shutdown hooks)
- [x] 4.17 Add integration tests: end-to-end request routing from raw HTTP to mock service provider and back

## 5. Service Framework

- [x] 5.1 Define the `ServiceProvider` base trait with lifecycle methods: `start()`, `stop()`, `check()`, `name()`, `service_name()`
- [x] 5.2 Implement `ServiceState` enum (Available, Starting, Running, Stopping, Stopped, Error) with thread-safe transitions
- [x] 5.3 Implement `ServiceContainer` wrapping a provider with state tracking and a loading lock
- [x] 5.4 Implement `ServicePluginManager` as the central registry: register providers, lazy-load on first request, respect SERVICES and EAGER_SERVICE_LOADING
- [x] 5.5 Implement `ServiceSkeleton` generic dispatch: map operation name to provider method, deserialize input, call method, serialize output
- [x] 5.6 Implement provider override resolution: check PROVIDER_OVERRIDE_<SERVICE> and select the correct provider implementation
- [x] 5.7 Generate provider traits from Smithy models (one trait per service with one method per operation)
- [x] 5.8 Implement default `not_implemented` responses for all trait methods
- [x] 5.9 Add tests: service lifecycle transitions, lazy loading, concurrent access, provider overrides

## 6. State Management & Persistence

- [x] 6.1 Implement `AccountRegionBundle<S>` generic store with DashMap for concurrent access keyed by (AccountId, Region)
- [x] 6.2 Implement attribute scoping: LocalAttribute (per account+region), CrossRegionAttribute (per account), CrossAccountAttribute (global)
- [x] 6.3 Implement state serialization using serde: serialize all stores to JSON files organized by {service}/{account_id}/{region}/
- [x] 6.4 Implement state deserialization: load JSON files from disk into store structs
- [x] 6.5 Implement `SNAPSHOT_SAVE_STRATEGY` (ON_SHUTDOWN, ON_REQUEST, SCHEDULED, MANUAL) with configurable flush interval
- [x] 6.6 Implement `SNAPSHOT_LOAD_STRATEGY` (ON_STARTUP, ON_REQUEST, MANUAL)
- [x] 6.7 Implement state lifecycle hooks trait: on_before_state_{save,load,reset}, on_after_state_{save,load,reset}
- [x] 6.8 Implement state reset (clear all stores, invoke hooks)
- [x] 6.9 Add tests: multi-account isolation, multi-region isolation, cross-region attributes, persistence round-trip, snapshot strategies

## 7. Internal API

- [x] 7.1 Implement `GET /_localstack/health` returning service states, edition, version
- [x] 7.2 Implement `HEAD /_localstack/health` returning 200 OK (liveness probe)
- [x] 7.3 Implement `POST /_localstack/health` with restart and kill actions
- [x] 7.4 Implement `GET /_localstack/info` returning version, edition, system info, uptime, session_id
- [x] 7.5 Implement init script runner: discover and execute .sh scripts from /etc/localstack/init/{boot,start,ready,shutdown}.d/ in alphabetical order
- [x] 7.6 Implement `GET /_localstack/init` and `GET /_localstack/init/<stage>` returning script execution status
- [x] 7.7 Implement `GET /_localstack/plugins` returning service provider information
- [x] 7.8 Implement `GET /_localstack/diagnose` (when DEBUG=1) with config dump, file tree, and service stats
- [x] 7.9 Implement `GET/POST /_localstack/config` (when ENABLE_CONFIG_UPDATES=1) for runtime config read/update
- [x] 7.10 Add integration tests for all internal API endpoints

## 8. Core AWS Services - S3

- [x] 8.1 Implement S3 provider struct with S3Store (buckets, objects stored in memory with optional disk backing)
- [x] 8.2 Implement bucket operations: CreateBucket, DeleteBucket, HeadBucket, ListBuckets, GetBucketLocation
- [x] 8.3 Implement object operations: PutObject, GetObject, HeadObject, DeleteObject, DeleteObjects, CopyObject
- [x] 8.4 Implement ListObjectsV2 with prefix, delimiter, continuation token, and max-keys support
- [x] 8.5 Implement multipart upload: CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload, ListMultipartUploads
- [x] 8.6 Implement bucket and object ACLs (GetBucketAcl, PutBucketAcl, GetObjectAcl, PutObjectAcl)
- [x] 8.7 Implement bucket policy (GetBucketPolicy, PutBucketPolicy, DeleteBucketPolicy)
- [x] 8.8 Implement object versioning (GetBucketVersioning, PutBucketVersioning, ListObjectVersions)
- [x] 8.9 Implement pre-signed URL generation support
- [x] 8.10 Implement S3 event notifications (for SNS/SQS/Lambda integration)
- [x] 8.11 Add integration tests using aws-sdk-rust S3 client against the running server

## 9. Core AWS Services - SQS

- [x] 9.1 Implement SQS provider struct with SqsStore (queues, messages, visibility tracking)
- [x] 9.2 Implement queue operations: CreateQueue, DeleteQueue, ListQueues, GetQueueUrl, GetQueueAttributes, SetQueueAttributes, PurgeQueue
- [x] 9.3 Implement message operations: SendMessage, ReceiveMessage (with WaitTimeSeconds long polling), DeleteMessage, ChangeMessageVisibility
- [x] 9.4 Implement batch operations: SendMessageBatch, DeleteMessageBatch, ChangeMessageVisibilityBatch
- [x] 9.5 Implement message attributes and system attributes
- [x] 9.6 Implement visibility timeout mechanics (message becomes visible again after timeout)
- [x] 9.7 Implement dead-letter queue (redrive policy, maxReceiveCount)
- [x] 9.8 Implement FIFO queue support (.fifo suffix, MessageGroupId, MessageDeduplicationId, exactly-once delivery)
- [x] 9.9 Implement delay queues (DelaySeconds on queue and per-message)
- [x] 9.10 Add integration tests using aws-sdk-rust SQS client

## 10. Core AWS Services - SNS

- [x] 10.1 Implement SNS provider struct with SnsStore (topics, subscriptions)
- [x] 10.2 Implement topic operations: CreateTopic, DeleteTopic, ListTopics, GetTopicAttributes, SetTopicAttributes
- [x] 10.3 Implement subscription operations: Subscribe, Unsubscribe, ListSubscriptions, ListSubscriptionsByTopic, GetSubscriptionAttributes, SetSubscriptionAttributes
- [x] 10.4 Implement Publish and PublishBatch
- [x] 10.5 Implement SQS subscription protocol (deliver messages to SQS queues)
- [x] 10.6 Implement HTTP/HTTPS subscription protocol (POST notifications to endpoints)
- [x] 10.7 Implement Lambda subscription protocol (invoke Lambda functions)
- [x] 10.8 Implement subscription filter policies (message attribute filtering)
- [x] 10.9 Implement FIFO topics
- [x] 10.10 Add integration tests using aws-sdk-rust SNS client

## 11. Core AWS Services - DynamoDB

- [x] 11.1 Implement DynamoDB provider struct with DynamoDbStore (tables, items indexed by key schema)
- [x] 11.2 Implement table operations: CreateTable, DeleteTable, DescribeTable, ListTables, UpdateTable
- [x] 11.3 Implement item operations: PutItem, GetItem, DeleteItem, UpdateItem
- [x] 11.4 Implement Query with key conditions, filter expressions, and projection expressions
- [x] 11.5 Implement Scan with filter expressions and projection expressions
- [x] 11.6 Implement batch operations: BatchGetItem, BatchWriteItem
- [x] 11.7 Implement transactions: TransactGetItems, TransactWriteItems
- [x] 11.8 Implement Global Secondary Indexes (GSI): creation, querying, projection
- [x] 11.9 Implement Local Secondary Indexes (LSI)
- [x] 11.10 Implement condition expressions for conditional writes
- [x] 11.11 Implement update expressions (SET, REMOVE, ADD, DELETE)
- [x] 11.12 Implement DynamoDB Streams: enable/disable streams, DescribeStream, GetRecords, GetShardIterator, ListStreams
- [x] 11.13 Add integration tests using aws-sdk-rust DynamoDB client

## 12. Core AWS Services - Lambda

- [x] 12.1 Implement Lambda provider struct with LambdaStore (functions, versions, aliases, event source mappings)
- [x] 12.2 Implement function management: CreateFunction, DeleteFunction, GetFunction, ListFunctions, UpdateFunctionCode, UpdateFunctionConfiguration
- [x] 12.3 Implement Invoke (synchronous RequestResponse) via Docker executor using bollard
- [x] 12.4 Implement async invocation (Event invocation type) with internal queue
- [x] 12.5 Implement Docker executor: pull runtime image, create container with function code mounted, invoke via Runtime Interface Client protocol
- [x] 12.6 Implement execution environment lifecycle: cold start, warm reuse (LAMBDA_KEEPALIVE_MS), cleanup (LAMBDA_REMOVE_CONTAINERS)
- [x] 12.7 Implement function environment variables injection
- [x] 12.8 Implement function timeout enforcement
- [x] 12.9 Implement hot-reload support (BUCKET_MARKER_LOCAL for local file mount)
- [x] 12.10 Implement Lambda layers (GetLayerVersion, PublishLayerVersion)
- [x] 12.11 Implement event source mappings for SQS, DynamoDB Streams, and Kinesis
- [x] 12.12 Add integration tests: create and invoke Python/Node.js Lambda functions

## 13. Core AWS Services - Kinesis & Firehose

- [x] 13.1 Implement Kinesis provider struct with KinesisStore (streams, shards, records)
- [x] 13.2 Implement stream management: CreateStream, DeleteStream, DescribeStream, ListStreams
- [x] 13.3 Implement data operations: PutRecord, PutRecords, GetRecords, GetShardIterator (TRIM_HORIZON, LATEST, AT_SEQUENCE_NUMBER, AFTER_SEQUENCE_NUMBER)
- [x] 13.4 Implement shard operations: SplitShard, MergeShard
- [x] 13.5 Implement Firehose provider struct with delivery stream management
- [x] 13.6 Implement Firehose PutRecord, PutRecordBatch with S3 destination delivery
- [x] 13.7 Add integration tests using aws-sdk-rust Kinesis and Firehose clients

## 14. Extended AWS Services - Identity & Security

- [x] 14.1 Implement IAM provider: CreateUser, DeleteUser, ListUsers, GetUser, CreateRole, DeleteRole, AssumeRole, CreatePolicy, AttachUserPolicy, AttachRolePolicy, PutRolePolicy, CreateGroup, AddUserToGroup (cross-region store)
- [x] 14.2 Implement STS provider: GetCallerIdentity, AssumeRole, GetSessionToken, GetAccessKeyInfo
- [x] 14.3 Implement KMS provider: CreateKey, DescribeKey, ListKeys, Encrypt, Decrypt, GenerateDataKey, Sign, Verify, CreateAlias, ListAliases, EnableKey, DisableKey, ScheduleKeyDeletion
- [x] 14.4 Implement Secrets Manager provider: CreateSecret, GetSecretValue, PutSecretValue, DeleteSecret, ListSecrets, DescribeSecret, UpdateSecret
- [x] 14.5 Implement SSM Parameter Store provider: PutParameter, GetParameter, GetParameters, GetParametersByPath, DeleteParameter, DescribeParameters
- [x] 14.6 Implement ACM provider: RequestCertificate, DescribeCertificate, ListCertificates, DeleteCertificate (auto-issue)
- [x] 14.7 Add integration tests for IAM, STS, KMS, Secrets Manager, SSM, ACM

## 15. Extended AWS Services - Infrastructure & Compute

- [x] 15.1 Implement CloudFormation provider: CreateStack, DeleteStack, DescribeStacks, ListStacks, UpdateStack with template engine
- [x] 15.2 Implement CloudFormation resource handlers for: AWS::S3::Bucket, AWS::SQS::Queue, AWS::SNS::Topic, AWS::DynamoDB::Table, AWS::Lambda::Function, AWS::IAM::Role, AWS::IAM::Policy
- [x] 15.3 Implement CloudFormation intrinsic functions: Ref, Fn::GetAtt, Fn::Join, Fn::Sub, Fn::Select, Fn::Split, Fn::If, Fn::Equals
- [x] 15.4 Implement CloudFormation dependency graph resolution (DependsOn, implicit dependencies)
- [x] 15.5 Implement CloudWatch provider: PutMetricData, GetMetricData, GetMetricStatistics, ListMetrics, PutMetricAlarm, DescribeAlarms, DeleteAlarms, SetAlarmState
- [x] 15.6 Implement CloudWatch Logs provider: CreateLogGroup, DeleteLogGroup, DescribeLogGroups, CreateLogStream, DescribeLogStreams, PutLogEvents, GetLogEvents, FilterLogEvents
- [x] 15.7 Implement EventBridge provider: CreateEventBus, DeleteEventBus, PutRule, DeleteRule, ListRules, PutTargets, RemoveTargets, PutEvents with event pattern matching and target dispatch (SQS, Lambda, SNS)
- [x] 15.8 Implement Step Functions provider: CreateStateMachine, DeleteStateMachine, DescribeStateMachine, StartExecution, DescribeExecution, ListExecutions with ASL interpreter for Task, Pass, Wait, Choice, Parallel, Map, Succeed, Fail states
- [x] 15.9 Implement API Gateway provider: CreateRestApi, DeleteRestApi, CreateResource, GetResources, PutMethod, PutIntegration, CreateDeployment with Lambda proxy integration routing
- [x] 15.10 Add integration tests for CloudFormation, CloudWatch, EventBridge, Step Functions, API Gateway

## 16. Extended AWS Services - Networking & Other

- [x] 16.1 Implement EC2 provider (metadata-only): CreateVpc, DescribeVpcs, DeleteVpc, CreateSubnet, DescribeSubnets, CreateSecurityGroup, DescribeSecurityGroups, AuthorizeSecurityGroupIngress, RunInstances, DescribeInstances, TerminateInstances
- [x] 16.2 Implement Route53 provider: CreateHostedZone, DeleteHostedZone, ListHostedZones, ChangeResourceRecordSets, ListResourceRecordSets
- [x] 16.3 Implement SES provider: VerifyEmailIdentity, ListIdentities, SendEmail, SendRawEmail (store emails in memory for test verification)
- [x] 16.4 Implement ECR provider: CreateRepository, DeleteRepository, DescribeRepositories, PutImage, BatchGetImage, ListImages
- [x] 16.5 Implement OpenSearch provider: CreateDomain, DeleteDomain, DescribeDomain (stub or with optional embedded engine)
- [x] 16.6 Implement Redshift provider (metadata-only): CreateCluster, DeleteCluster, DescribeClusters
- [x] 16.7 Add integration tests for EC2, Route53, SES, ECR, OpenSearch, Redshift

## 17. Multi-Tenancy

- [x] 17.1 Implement access key to account ID mapping with default account `000000000000`
- [x] 17.2 Implement deterministic account ID derivation from unknown access keys
- [x] 17.3 Implement ARN generation utility that uses request context account ID and region
- [x] 17.4 Integrate multi-tenancy into all service providers (ensure all stores use AccountRegionBundle)
- [x] 17.5 Add integration tests: multi-account isolation, multi-region isolation, cross-account access patterns

## 18. DNS Server

- [x] 18.1 Implement embedded DNS server using hickory-dns, configurable via DNS_ADDRESS and DNS_PORT
- [x] 18.2 Implement wildcard resolution for *.localhost.localstack.cloud to DNS_RESOLVE_IP
- [x] 18.3 Implement upstream DNS forwarding for non-localstack queries
- [x] 18.4 Add tests for DNS resolution

## 19. Docker & Deployment

- [x] 19.1 Finalize multi-stage Dockerfile: builder stage (rust:latest) + runtime stage (debian-slim or scratch)
- [x] 19.2 Configure Docker image entrypoint to start the openstack binary with appropriate defaults
- [x] 19.3 Add docker-compose.yml matching LocalStack's format (ports, volumes, env vars)
- [x] 19.4 Implement external service port allocator (range 4510-4560)
- [x] 19.5 Set up CI workflow for building and testing (cargo test, cargo clippy, cargo fmt --check)
- [x] 19.6 Set up CI workflow for Docker image build and push
- [x] 19.7 Add cross-compilation targets for linux-amd64 and linux-arm64

## 20. Integration Testing & Compatibility Validation

- [x] 20.1 Create integration test harness that starts the server, runs tests via aws-sdk-rust, and validates responses
- [x] 20.2 Write smoke tests for each service: create a resource, read it back, delete it
- [x] 20.3 Write compatibility tests using awslocal CLI (pip install awscli-local) to verify real-world usage patterns
- [x] 20.4 Write Terraform compatibility test: apply a simple Terraform config (S3 + SQS + DynamoDB) against the server
- [x] 20.5 Benchmark startup time (target: <1 second)
- [x] 20.6 Benchmark memory usage at idle (target: <50MB) and under load
- [x] 20.7 Write docker-compose integration test: start via docker-compose, run test suite, verify all services respond
