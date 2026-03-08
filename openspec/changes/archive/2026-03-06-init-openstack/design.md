## Context

This is a greenfield Rust project to create a drop-in replacement for LocalStack Community Edition. There is no existing codebase -- we are building from scratch. The reference implementation is [localstack/localstack](https://github.com/localstack/localstack), a Python-based AWS cloud emulator with 64k+ GitHub stars.

LocalStack's architecture centers on: (1) a single-port HTTP gateway (port 4566) that parses AWS requests and routes them to service providers, (2) a plugin-based service provider framework where each AWS service is an independent module, (3) a state management system with multi-account/multi-region isolation, and (4) a persistence layer with configurable snapshot strategies.

The target users are developers who already use LocalStack and want faster startup, lower memory, and native performance -- with zero configuration changes.

## Goals / Non-Goals

**Goals:**
- 100% API-compatible with LocalStack Community Edition for all supported AWS services
- Drop-in replacement: same port (4566), same env vars, same `/_localstack/*` endpoints, same Docker image entrypoints
- Sub-second startup time (vs. LocalStack's ~5-10s)
- Memory usage under 50MB idle (vs. LocalStack's ~500MB+)
- True multi-threaded concurrency via tokio async runtime
- Single statically-linked binary (~20MB) plus Docker image
- Full multi-account and multi-region isolation
- State persistence compatible with LocalStack's snapshot model
- Lambda execution via Docker containers (matching LocalStack's executor model)

**Non-Goals:**
- LocalStack Pro/Team/Enterprise features (Cloud Pods sharing, team collaboration, advanced services)
- Pro-only AWS services (Cognito, ECS, EKS, Athena, Glue, RDS, etc. that require a license)
- Moto compatibility layer (we implement services natively in Rust, not via Python Moto)
- Python plugin API (providers are Rust modules, not Python plugins)
- `localstack` CLI reimplementation (the existing CLI should work against our gateway; we provide our own `openstack` binary)
- AWS parity testing certification (we target LocalStack parity, not direct AWS parity)

## Decisions

### 1. HTTP Framework: Axum + Hyper + Tokio

**Choice:** Axum on top of Hyper and Tokio.

**Rationale:** Axum provides a composable middleware/handler model that maps well to LocalStack's handler chain pattern. Hyper gives low-level HTTP control needed for AWS protocol quirks. Tokio is the industry standard async runtime for Rust with proven performance at scale.

**Alternatives considered:**
- *Actix-web*: Higher throughput in benchmarks but less composable middleware model, harder to implement the handler chain pattern
- *Warp*: Filter-based API is elegant but harder to debug and less flexible for our complex routing needs
- *Raw Hyper*: Maximum control but too much boilerplate for 50+ services

### 2. Project Structure: Cargo Workspace with Per-Service Crates

**Choice:** Mono-repo Cargo workspace with these crates:
```
openstack/
├── Cargo.toml              (workspace root)
├── crates/
│   ├── openstack/          (binary entry point)
│   ├── gateway/            (HTTP gateway, routing, handler chain)
│   ├── aws-protocol/       (AWS request/response parsing for all protocols)
│   ├── service-framework/  (provider traits, lifecycle, skeleton dispatch)
│   ├── state/              (AccountRegionBundle, persistence, snapshots)
│   ├── config/             (env var parsing, configuration)
│   ├── internal-api/       (/_localstack/* endpoints)
│   ├── dns/                (DNS server for *.localhost.localstack.cloud)
│   └── services/
│       ├── s3/
│       ├── sqs/
│       ├── sns/
│       ├── dynamodb/
│       ├── lambda/
│       ├── iam/
│       ├── sts/
│       ├── kms/
│       ├── cloudformation/
│       ├── cloudwatch/
│       ├── kinesis/
│       ├── eventbridge/
│       ├── stepfunctions/
│       ├── apigateway/
│       ├── ec2/
│       ├── route53/
│       ├── ses/
│       ├── ssm/
│       ├── secretsmanager/
│       ├── acm/
│       └── ...              (one crate per service)
├── tests/                   (integration tests)
└── Dockerfile
```

**Rationale:** Per-service crates enable parallel compilation, clean dependency boundaries, and the ability to conditionally compile services. The workspace mirrors LocalStack's `localstack/services/<name>/` structure for navigability.

**Alternatives considered:**
- *Single crate with feature flags*: Simpler Cargo.toml but 10+ minute compile times and poor IDE performance
- *Separate repos per service*: Too much overhead for a single project

### 3. AWS Protocol Parsing: Smithy Model Code Generation

**Choice:** Use AWS Smithy models (the same JSON models used by botocore) to generate Rust types and request/response parsers at build time via a `build.rs` code generator.

**Rationale:** LocalStack itself uses botocore's service models to parse requests. By using the same Smithy/botocore JSON models, we guarantee identical request/response shapes. Code generation avoids hand-writing thousands of request/response structs and serializers.

**Alternatives considered:**
- *Hand-written parsers*: More control but unsustainable for 50+ services with thousands of operations
- *aws-sdk-rust smithy-rs*: Too tightly coupled to the actual AWS SDK client; we need the server-side (request parsing) not client-side
- *Runtime reflection*: Not idiomatic Rust and would sacrifice type safety

### 4. Service Dispatch: Trait-Based Provider Pattern

**Choice:** Each service defines a trait (e.g., `SqsProvider`) with one method per AWS operation. A `ServiceSkeleton` dispatches parsed requests to the correct trait method using a generated match table.

```rust
#[async_trait]
pub trait SqsProvider: Send + Sync {
    async fn create_queue(&self, ctx: &RequestContext, input: CreateQueueInput) -> Result<CreateQueueOutput, SqsError>;
    async fn send_message(&self, ctx: &RequestContext, input: SendMessageInput) -> Result<SendMessageOutput, SqsError>;
    // ... one method per operation
}
```

**Rationale:** Mirrors LocalStack's ASF provider pattern. Compile-time dispatch via traits is zero-cost. Each provider implementation is a concrete struct that holds its state stores.

### 5. State Management: Generic Store with Scoped Access

**Choice:** A `Store<T>` generic over the service-specific state, wrapped in `AccountRegionBundle<Store<T>>` for multi-tenancy isolation. Scoping is enforced at the type level.

```rust
pub struct AccountRegionBundle<S> {
    stores: DashMap<(AccountId, Region), S>,
}

pub struct SqsStore {
    pub queues: HashMap<String, Queue>,       // LocalAttribute (per account+region)
}
```

`DashMap` provides concurrent access without a global lock.

**Rationale:** Matches LocalStack's `AccountRegionBundle` → `RegionBundle` → `Store` hierarchy. Using generics instead of dynamic dispatch avoids boxing overhead for the hot path (every request accesses state).

### 6. Persistence: Serde-Based Snapshots to Disk

**Choice:** All state structs derive `Serialize`/`Deserialize`. The persistence layer serializes state to JSON files on disk, organized by `{data_dir}/state/{service}/{account_id}/{region}/`.

**Rationale:** JSON is human-readable and debuggable. Serde is zero-cost for serialization in Rust. The directory structure matches LocalStack's persistence layout.

**Alternatives considered:**
- *SQLite*: More structured but adds a dependency and doesn't match LocalStack's file-based model
- *bincode/MessagePack*: Faster serialization but not human-readable, harder to debug

### 7. Lambda Execution: Docker via Bollard

**Choice:** Use the `bollard` crate (async Docker API client) to manage Lambda execution containers, matching LocalStack's `docker` executor model.

**Rationale:** Lambda is the most complex service because it spawns real containers. Bollard is the mature async Rust Docker client and integrates natively with Tokio.

### 8. DNS Server: trust-dns

**Choice:** Embed a DNS server using `hickory-dns` (formerly trust-dns) to resolve `*.localhost.localstack.cloud` to `127.0.0.1`.

**Rationale:** LocalStack runs an embedded DNS server for service-specific hostnames. Hickory-dns is the standard Rust DNS library with async support.

## Risks / Trade-offs

**[Scope magnitude]** Full parity with 30+ AWS services is massive (~50k+ LOC). → *Mitigation:* Implement in waves. Core services (S3, SQS, SNS, DynamoDB, Lambda) first, then progressively add services. Each service is an independent crate that can be developed in parallel.

**[AWS API surface accuracy]** AWS APIs have undocumented behaviors, edge cases, and version quirks. → *Mitigation:* Use Smithy models for structural accuracy. Run LocalStack's integration test suite against our implementation to catch behavioral gaps.

**[Lambda cold start complexity]** Docker container management for Lambda is the most complex subsystem. → *Mitigation:* Start with a basic executor that runs single-invocation containers, then optimize with keep-alive pools matching LocalStack's `LAMBDA_KEEPALIVE_MS` behavior.

**[CloudFormation engine complexity]** CloudFormation requires a full template parser, dependency graph resolver, and resource lifecycle manager. → *Mitigation:* Implement a subset of resource types first (the most commonly used: S3 Bucket, SQS Queue, SNS Topic, DynamoDB Table, Lambda Function, IAM Role). Expand coverage incrementally.

**[No Moto fallback]** LocalStack falls back to Moto for unimplemented operations. We won't have this safety net. → *Mitigation:* Return explicit `NotImplemented` errors for unimplemented operations so users get clear feedback. Track coverage with a compatibility matrix.

**[Binary size]** Compiling 30+ services into one binary could produce a large (100MB+) executable. → *Mitigation:* Use Cargo feature flags to allow compiling subsets of services. Use `strip` and LTO for release builds. Target ~20-30MB for the full binary.

## Open Questions

1. **Should we support LocalStack's Python init scripts?** LocalStack runs `.py` files via `exec()`. We could embed a Python interpreter (via PyO3) or only support `.sh` scripts. Recommendation: start with `.sh` only, add Python support later if demanded.

2. **What serialization format for snapshots?** JSON matches LocalStack but is slower for large state. Should we offer bincode as an option? Recommendation: JSON default with optional bincode behind a feature flag.

3. **Should we attempt state import from existing LocalStack snapshots?** This would ease migration. Recommendation: defer to a future change -- focus on functional parity first.
