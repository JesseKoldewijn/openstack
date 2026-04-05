# openstack

[![CI](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/ci.yml?branch=main&label=CI&logo=github)](https://github.com/JesseKoldewijn/openstack/actions/workflows/ci.yml)
[![Docker](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/docker.yml?branch=main&label=Docker&logo=docker&logoColor=white)](https://github.com/JesseKoldewijn/openstack/actions/workflows/docker.yml)
[![Stable release](https://img.shields.io/github/v/release/JesseKoldewijn/openstack?sort=semver&label=stable%20release)](https://github.com/JesseKoldewijn/openstack/releases)
[![RC tag](https://img.shields.io/github/v/tag/JesseKoldewijn/openstack?filter=v*-rc-*&label=rc%20tag)](https://github.com/JesseKoldewijn/openstack/tags)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Docker Image](https://img.shields.io/badge/ghcr.io-JesseKoldewijn%2Fopenstack-blue?logo=github)](https://github.com/JesseKoldewijn/openstack/pkgs/container/openstack)

> **Work in progress.** openstack is under active development. APIs may be incomplete, behaviour may differ from AWS or LocalStack in edge cases, and breaking changes may occur between releases. Production use is not recommended at this time.

A Rust reimplementation of [LocalStack](https://localstack.cloud) Community Edition — a 100% API-compatible, drop-in replacement for the Python original.

---

## Service Coverage

<details>
<summary>Full service coverage table (click to expand)</summary>

| Service | Status | Protocol |
|---|---|---|
| Account Management | not implemented | — |
| Amplify | not implemented | — |
| API Gateway | ✅ supported | rest-json |
| AppConfig | not implemented | — |
| Application Auto Scaling | not implemented | — |
| AppSync | not implemented | — |
| Athena | not implemented | — |
| Auto Scaling | not implemented | — |
| Backup | not implemented | — |
| Batch | not implemented | — |
| Bedrock | not implemented | — |
| Certificate Manager (ACM) | ✅ supported | json |
| Cloud Control | not implemented | — |
| CloudFormation | ✅ supported | query (XML) |
| CloudFront | not implemented | — |
| CloudTrail | not implemented | — |
| CloudWatch (metrics + alarms) | ✅ supported | json |
| CloudWatch Logs | ✅ supported | json |
| CodeArtifact | not implemented | — |
| CodeBuild | not implemented | — |
| CodeCommit | not implemented | — |
| CodeConnections | not implemented | — |
| CodeDeploy | not implemented | — |
| CodePipeline | not implemented | — |
| Cognito | not implemented | — |
| Config | not implemented | — |
| Cost Explorer | not implemented | — |
| Data Firehose | ✅ supported | json |
| Database Migration Service (DMS) | not implemented | — |
| DocumentDB (DocDB) | not implemented | — |
| DynamoDB | ✅ supported | json |
| DynamoDB Streams | ✅ supported | json |
| EC2 | ✅ supported | ec2 query |
| ECR | ✅ supported | json |
| Elastic Beanstalk | not implemented | — |
| Elastic Container Service (ECS) | not implemented | — |
| Elastic File System (EFS) | not implemented | — |
| Elastic Kubernetes Service (EKS) | not implemented | — |
| Elastic Load Balancing (ELB) | not implemented | — |
| Elastic MapReduce (EMR) | not implemented | — |
| ElastiCache | not implemented | — |
| Elasticsearch Service | not implemented | — |
| Elemental MediaConvert | not implemented | — |
| EventBridge | ✅ supported | json |
| EventBridge Pipes | not implemented | — |
| EventBridge Scheduler | not implemented | — |
| Fault Injection Service (FIS) | not implemented | — |
| Glacier | not implemented | — |
| Glue | not implemented | — |
| Identity and Access Management (IAM) | ✅ supported | query (XML) |
| Identity Store | not implemented | — |
| IoT | not implemented | — |
| IoT Data | not implemented | — |
| IoT Wireless | not implemented | — |
| Key Management Service (KMS) | ✅ supported | json |
| Kinesis Data Streams | ✅ supported | json |
| Lake Formation | not implemented | — |
| Lambda | ✅ supported | json + Docker |
| Managed Blockchain (AMB) | not implemented | — |
| Managed Service for Apache Flink | not implemented | — |
| Managed Streaming for Kafka (MSK) | not implemented | — |
| Managed Workflows for Apache Airflow (MWAA) | not implemented | — |
| MemoryDB | not implemented | — |
| MQ | not implemented | — |
| Neptune | not implemented | — |
| OpenSearch Service | ✅ supported | rest-json |
| Organizations | not implemented | — |
| Pinpoint | not implemented | — |
| Private Certificate Authority (ACM PCA) | not implemented | — |
| Redshift | ✅ supported | query (XML) |
| Relational Database Service (RDS) | not implemented | — |
| Resource Access Manager (RAM) | not implemented | — |
| Resource Groups | not implemented | — |
| Resource Groups Tagging API | not implemented | — |
| Route 53 | ✅ supported | rest-xml |
| Route 53 Resolver | not implemented | — |
| S3 | ✅ supported | rest-xml |
| S3 Tables | not implemented | — |
| SageMaker | not implemented | — |
| Secrets Manager | ✅ supported | json |
| Security Token Service (STS) | ✅ supported | query (XML) |
| Serverless Application Repository | not implemented | — |
| Service Discovery | not implemented | — |
| Shield | not implemented | — |
| Simple Email Service (SES) | ✅ supported | query (XML) |
| Simple Notification Service (SNS) | ✅ supported | query (XML) |
| Simple Queue Service (SQS) | ✅ supported | query (XML) |
| Simple Workflow Service (SWF) | not implemented | — |
| SSO Admin | not implemented | — |
| Step Functions | ✅ supported | json |
| Support | not implemented | — |
| Systems Manager (SSM) | ✅ supported | json |
| Textract | not implemented | — |
| Timestream | not implemented | — |
| Transcribe | not implemented | — |
| Transfer | not implemented | — |
| Verified Permissions | not implemented | — |
| Web Application Firewall (WAF) | not implemented | — |
| X-Ray | not implemented | — |

The table lists every service in LocalStack's Community Edition and the current implementation status in openstack.

</details>

---

## Quick Start

### Prerequisites

- Rust stable toolchain (edition 2024; Rust 1.85+)
- Docker (recommended for running the published image and Lambda container execution)

### Docker (recommended)

```bash
docker run --rm -p 4566:4566 ghcr.io/jessekoldewijn/openstack:latest
```

### Docker Compose

```bash
docker compose up
```

The API endpoint is `http://localhost:4566`. All AWS services are accessible on the same port (edge proxy routing).

### Binary

```bash
cargo build --release
./target/release/openstack
```

---

## Usage with AWS CLI

Point any AWS CLI command at the local endpoint:

```bash
aws --endpoint-url http://localhost:4566 s3 mb s3://my-bucket
aws --endpoint-url http://localhost:4566 sqs create-queue --queue-name my-queue
aws --endpoint-url http://localhost:4566 dynamodb list-tables
```

Or use [`awslocal`](https://github.com/localstack/awscli-local) (a thin wrapper that injects the endpoint automatically):

```bash
pip install awscli-local
awslocal s3 mb s3://my-bucket
awslocal sqs create-queue --queue-name my-queue
```

---

## Configuration

openstack is configured entirely through environment variables, fully compatible with LocalStack's variable names:

| Variable | Default | Description |
|---|---|---|
| `GATEWAY_LISTEN` | `0.0.0.0:4566` | Bind address(es) for the HTTP gateway |
| `LOCALSTACK_HOST` | `localhost.localstack.cloud:4566` | Hostname used in generated URLs |
| `SERVICES` | _(all)_ | Comma-separated list of services to enable |
| `PERSISTENCE` | `0` | Enable state persistence to `DATA_DIR` |
| `DATA_DIR` | `/var/lib/localstack` | Directory for persisted state |
| `LS_LOG` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `DEBUG` | `0` | Enable debug mode (exposes `/_localstack/diagnose`; also enables Studio by default) |
| `DNS_ADDRESS` | `0.0.0.0` | DNS server bind address |
| `DNS_PORT` | `53` | DNS server port |
| `DNS_RESOLVE_IP` | `127.0.0.1` | IP that `*.localhost.localstack.cloud` resolves to |
| `LAMBDA_KEEPALIVE_MS` | `600000` | How long to keep warm Lambda containers alive |
| `LAMBDA_REMOVE_CONTAINERS` | `1` | Remove containers after invocation |
| `SNAPSHOT_SAVE_STRATEGY` | `ON_SHUTDOWN` | When to flush state to disk |
| `SNAPSHOT_LOAD_STRATEGY` | `ON_STARTUP` | When to load persisted state |
| `ALLOW_NONSTANDARD_REGIONS` | `0` | Allow arbitrary region names |
| `EAGER_SERVICE_LOADING` | `0` | Start all services at boot instead of lazily |
| `STUDIO` | `0` | Enable Studio UI/transaction capture (`1`/`true`) |

---

## Studio UI

Studio is embedded in the gateway and available at:

- `/_localstack/studio` (SPA)
- `/_localstack/studio/assets/*` (static assets)

Enable it explicitly:

```bash
openstack start --studio
# or
STUDIO=1 openstack start
```

Behavior by mode:

- **Studio disabled (default/headless benchmark mode):**
  - transaction log allocation is skipped
  - transaction recording endpoints are silent no-ops
- **Studio enabled:**
  - live transaction recording (including guided/raw interactions)
  - operation catalog + guided flow explorer + storage/transactions tabs

Notes:

- S3 supports both **path-style** and **virtual-hosted-style** request forms.
- Studio-origin requests are marked internally to avoid duplicate transaction rows.

## Internal API

The following management endpoints are available (LocalStack-compatible):

| Endpoint | Description |
|---|---|
| `GET /_localstack/health` | Service states, edition, version |
| `HEAD /_localstack/health` | Liveness probe (200 OK) |
| `GET /_localstack/info` | Version, uptime, session ID |
| `GET /_localstack/init` | Init script execution status |
| `GET /_localstack/plugins` | Registered service providers |
| `GET /_localstack/diagnose` | Config + diagnostics (DEBUG=1 only) |
| `GET /_localstack/config` | Runtime config read/update |
| `GET /_localstack/studio-api/runtime-config` | Studio runtime endpoint/credentials/polling config |
| `GET /_localstack/studio-api/operations[/{service}]` | Per-service operation catalog |
| `GET /_localstack/studio-api/storage[/{service}]` | Live storage snapshots |
| `GET /_localstack/studio-api/transactions[/{service}]` | Transaction history |
| `POST /_localstack/studio-api/transactions/record` | Record transaction entry |
| `DELETE /_localstack/studio-api/transactions[/{service}]` | Clear all or service-scoped transactions |

---

## Workspace Structure

```
crates/
├── openstack/          Binary entry point
├── config/             Environment variable parsing
├── gateway/            Axum/Hyper HTTP server + handler chain
├── aws-protocol/       AWS wire protocol parsers/serializers
├── service-framework/  Provider trait, lifecycle, plugin manager
├── state/              AccountRegionBundle, persistence, snapshots
├── internal-api/       /_localstack/* management endpoints
├── dns/                Embedded hickory-dns server
├── studio-ui/          Studio data/model crate + protocol adapters + tests
├── services/           One crate per AWS service (24 crates, 26 APIs)
└── tests/integration/  Integration test harness
```

---

## Automated SemVer Versioning

openstack uses an automated SemVer release-PR flow via `release-plz`.

- Release PR workflow: `.github/workflows/release-plz.yml` (pushes to `develop`)
- Develop RC tagging workflow: `.github/workflows/develop-rc-tag.yml` (pushes to `develop`)
- Release workflow: `.github/workflows/release.yml` (pushes to `main`)
- Docker channel policy (`.github/workflows/docker.yml`):
  - `main` → stable (`stable`, `latest`, + semver tags on `v*.*.*`)
  - `develop` → RC (`rc`, `rc-<short-sha>`)
  - `pull_request` → RC preview tags (published for same-repo PRs):
    - immutable: `v<base-rc>.pr-<number>` (e.g. `v1.0.0-rc-2.pr-33`)
    - mutable PR pointer: `pr-<number>` (always updated to latest image for that PR)
    - PR label: `@version:<immutable-tag>` (updated on each PR push via `pr-version-label.yml`)
    - stale preview versions are cleaned in the same GHCR cleanup cycle
- Config: `.release-plz.toml`
- Output: automated release PR(s), version/changelog updates, and SemVer tag/release automation
- Build metadata propagation:
  - binary `openstack --version` includes build tag/sha when provided by CI
  - internal API exposes `version_display` and structured `build` metadata

Conventional commit guidance (used for SemVer bump decisions):

- `feat: ...` → **minor**
- `fix: ...` / `perf: ...` → **patch**
- `feat!: ...` or `BREAKING CHANGE:` footer → **major**

Example:

```text
feat(gateway): add virtual-hosted-style S3 rewrite
fix(studio): avoid duplicate transaction rows
feat!: remove deprecated config field
```

See also: [`docs/semver-release.md`](docs/semver-release.md) for full human/agent release guidance.

## Development

```bash
# Build
cargo build

# Run tests
cargo test --workspace

# Lint
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check

# Run locally
GATEWAY_LISTEN=127.0.0.1:4566 LS_LOG=debug cargo run
```

### Cross-compilation

```bash
# Linux x86_64
cross build --release --target x86_64-unknown-linux-gnu

# Linux arm64
cross build --release --target aarch64-unknown-linux-gnu
```

---

## License

MIT — see [LICENSE](LICENSE).
