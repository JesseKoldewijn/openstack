# openstack

[![CI](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/ci.yml?branch=main&label=CI&logo=github)](https://github.com/JesseKoldewijn/openstack/actions/workflows/ci.yml)
[![Docker](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/docker.yml?branch=main&label=Docker&logo=docker&logoColor=white)](https://github.com/JesseKoldewijn/openstack/actions/workflows/docker.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Docker Image](https://img.shields.io/badge/ghcr.io-JesseKoldewijn%2Fopenstack-blue?logo=github)](https://github.com/JesseKoldewijn/openstack/pkgs/container/openstack)

A Rust reimplementation of [LocalStack](https://localstack.cloud) Community Edition — a 100% API-compatible, drop-in replacement for the Python original.

---

## Supported Services

| Service | Protocol |
|---|---|
| S3 | rest-xml |
| SQS | query (XML) |
| SNS | query (XML) |
| DynamoDB | json |
| Lambda | json + Docker |
| IAM | query (XML) |
| STS | query (XML) |
| KMS | json |
| Secrets Manager | json |
| SSM Parameter Store | json |
| ACM | json |
| Kinesis | json |
| Firehose | json |
| CloudFormation | query (XML) |
| CloudWatch (metrics + logs) | json |
| EventBridge | json |
| Step Functions | json |
| API Gateway | rest-json |
| EC2 (metadata) | ec2 query |
| Route 53 | rest-xml |
| SES | query (XML) |
| ECR | json |
| OpenSearch | rest-json |
| Redshift (metadata) | query (XML) |

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
| `DEBUG` | `0` | Enable debug mode (exposes `/_localstack/diagnose`) |
| `DNS_ADDRESS` | `0.0.0.0` | DNS server bind address |
| `DNS_PORT` | `53` | DNS server port |
| `DNS_RESOLVE_IP` | `127.0.0.1` | IP that `*.localhost.localstack.cloud` resolves to |
| `LAMBDA_KEEPALIVE_MS` | `600000` | How long to keep warm Lambda containers alive |
| `LAMBDA_REMOVE_CONTAINERS` | `1` | Remove containers after invocation |
| `SNAPSHOT_SAVE_STRATEGY` | `ON_SHUTDOWN` | When to flush state to disk |
| `SNAPSHOT_LOAD_STRATEGY` | `ON_STARTUP` | When to load persisted state |
| `ALLOW_NONSTANDARD_REGIONS` | `0` | Allow arbitrary region names |
| `EAGER_SERVICE_LOADING` | `0` | Start all services at boot instead of lazily |

---

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
├── services/           One crate per AWS service (24 total)
└── tests/integration/  Integration test harness
```

---

## Development

```bash
# Build
cargo build

# Run tests
cargo test --workspace

# Lint
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check

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
