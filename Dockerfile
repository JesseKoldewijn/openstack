# syntax=docker/dockerfile:1

# ─────────────────────────────────────────────
# Stage 1: builder
# ─────────────────────────────────────────────
FROM rust:bookworm AS builder

# Install system dependencies required by some crates
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation separately from source compilation.
# Copy only manifests first.
COPY Cargo.toml Cargo.lock ./
COPY crates/openstack/Cargo.toml          crates/openstack/Cargo.toml
COPY crates/config/Cargo.toml             crates/config/Cargo.toml
COPY crates/gateway/Cargo.toml            crates/gateway/Cargo.toml
COPY crates/aws-protocol/Cargo.toml       crates/aws-protocol/Cargo.toml
COPY crates/service-framework/Cargo.toml  crates/service-framework/Cargo.toml
COPY crates/state/Cargo.toml              crates/state/Cargo.toml
COPY crates/internal-api/Cargo.toml       crates/internal-api/Cargo.toml
COPY crates/dns/Cargo.toml                crates/dns/Cargo.toml
COPY crates/services/s3/Cargo.toml            crates/services/s3/Cargo.toml
COPY crates/services/sqs/Cargo.toml           crates/services/sqs/Cargo.toml
COPY crates/services/sns/Cargo.toml           crates/services/sns/Cargo.toml
COPY crates/services/dynamodb/Cargo.toml      crates/services/dynamodb/Cargo.toml
COPY crates/services/lambda/Cargo.toml        crates/services/lambda/Cargo.toml
COPY crates/services/iam/Cargo.toml           crates/services/iam/Cargo.toml
COPY crates/services/sts/Cargo.toml           crates/services/sts/Cargo.toml
COPY crates/services/kms/Cargo.toml           crates/services/kms/Cargo.toml
COPY crates/services/cloudformation/Cargo.toml crates/services/cloudformation/Cargo.toml
COPY crates/services/cloudwatch/Cargo.toml    crates/services/cloudwatch/Cargo.toml
COPY crates/services/kinesis/Cargo.toml       crates/services/kinesis/Cargo.toml
COPY crates/services/firehose/Cargo.toml      crates/services/firehose/Cargo.toml
COPY crates/services/eventbridge/Cargo.toml   crates/services/eventbridge/Cargo.toml
COPY crates/services/stepfunctions/Cargo.toml crates/services/stepfunctions/Cargo.toml
COPY crates/services/apigateway/Cargo.toml    crates/services/apigateway/Cargo.toml
COPY crates/services/ec2/Cargo.toml           crates/services/ec2/Cargo.toml
COPY crates/services/route53/Cargo.toml       crates/services/route53/Cargo.toml
COPY crates/services/ses/Cargo.toml           crates/services/ses/Cargo.toml
COPY crates/services/ssm/Cargo.toml           crates/services/ssm/Cargo.toml
COPY crates/services/secretsmanager/Cargo.toml crates/services/secretsmanager/Cargo.toml
COPY crates/services/acm/Cargo.toml           crates/services/acm/Cargo.toml
COPY crates/services/ecr/Cargo.toml           crates/services/ecr/Cargo.toml
COPY crates/services/opensearch/Cargo.toml    crates/services/opensearch/Cargo.toml
COPY crates/services/redshift/Cargo.toml      crates/services/redshift/Cargo.toml
COPY crates/tests/integration/Cargo.toml     crates/tests/integration/Cargo.toml

# Create stub lib.rs / main.rs files so Cargo can resolve the dependency graph
RUN find crates -name "Cargo.toml" | while read f; do \
      dir=$(dirname "$f"); \
      mkdir -p "$dir/src"; \
      if grep -q '\[\[bin\]\]' "$f" || grep -q 'name = "openstack"' "$f"; then \
        echo 'fn main() {}' > "$dir/src/main.rs"; \
      else \
        touch "$dir/src/lib.rs"; \
      fi; \
    done

# Fetch + compile dependencies only (layer-cached)
RUN cargo fetch
RUN cargo build --release --bin openstack

# Now copy the real source and rebuild
COPY . .

RUN find crates -type f -path "*/src/*" -exec touch {} + \
    && cargo build --release --bin openstack

# ─────────────────────────────────────────────
# Stage 2: minimal runtime image
# ─────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Runtime libraries
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Prepare runtime directories for non-root execution.
RUN mkdir -p /var/lib/localstack /etc/localstack/init \
    && chown -R 1000:1000 /var/lib/localstack /etc/localstack

# Non-root user
RUN useradd -ms /bin/bash openstack
USER openstack
WORKDIR /home/openstack

COPY --from=builder /build/target/release/openstack /usr/local/bin/openstack

# LocalStack-compatible port (main edge API)
EXPOSE 4566
# DNS port
EXPOSE 53/udp
# External service ports range
EXPOSE 4510-4560

# Sensible defaults — all can be overridden at runtime via environment variables.
ENV PERSISTENCE=0 \
    GATEWAY_LISTEN=0.0.0.0:4566 \
    DATA_DIR=/var/lib/localstack \
    DNS_ADDRESS=0.0.0.0 \
    DNS_PORT=53 \
    DNS_RESOLVE_IP=127.0.0.1 \
    LOCALSTACK_HOST=localhost.localstack.cloud:4566 \
    LS_LOG=info

# Health check matching LocalStack's endpoint
HEALTHCHECK --interval=5s --timeout=3s --start-period=10s --retries=5 \
    CMD curl -f http://localhost:4566/_localstack/health || exit 1

ENTRYPOINT ["/usr/local/bin/openstack"]
