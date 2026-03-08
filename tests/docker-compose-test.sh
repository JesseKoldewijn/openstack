#!/usr/bin/env bash
# docker-compose-test.sh
#
# Integration test: starts openstack via docker-compose, runs a suite of
# AWS API smoke tests against the running container, then tears down.
#
# Prerequisites:
#   - docker and docker compose (v2) installed
#   - curl installed
#   - aws CLI installed (configured with any credentials — endpoint is overridden)
#
# Usage:
#   ./tests/docker-compose-test.sh
#   IMAGE=my-registry/openstack:latest ./tests/docker-compose-test.sh

set -euo pipefail

REPO_ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
COMPOSE_FILE="${REPO_ROOT}/docker-compose.yml"
ENDPOINT="http://127.0.0.1:4566"
HEALTH_URL="${ENDPOINT}/_localstack/health"
AWS_REGION="us-east-1"
AWS_ACCOUNT="000000000000"

# Allow overriding the image (e.g. in CI after a docker build)
export IMAGE="${IMAGE:-openstack:latest}"

PASS=0
FAIL=0
ERRORS=()

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

aws_local() {
    aws --endpoint-url "${ENDPOINT}" \
        --region "${AWS_REGION}" \
        --no-sign-request \
        --output json \
        "$@" 2>&1
}

check() {
    local description="$1"; shift
    if "$@" > /dev/null 2>&1; then
        printf "  PASS  %s\n" "${description}"
        PASS=$((PASS + 1))
    else
        printf "  FAIL  %s\n" "${description}"
        FAIL=$((FAIL + 1))
        ERRORS+=("${description}")
    fi
}

wait_healthy() {
    echo "Waiting for openstack to become healthy..."
    for i in $(seq 1 60); do
        if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
            echo "  Ready after ${i} attempts"
            return 0
        fi
        sleep 2
    done
    echo "ERROR: container did not become healthy within 120 s"
    return 1
}

# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------

cleanup() {
    echo
    echo "--- Tearing down docker-compose stack ---"
    docker compose -f "${COMPOSE_FILE}" down --volumes --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

echo "=== openstack docker-compose integration test ==="
echo "Compose file: ${COMPOSE_FILE}"
echo "Image:        ${IMAGE}"
echo

echo "--- Starting stack ---"
docker compose -f "${COMPOSE_FILE}" up -d --build 2>&1 | tail -5

wait_healthy

echo
echo "--- Health / info endpoints ---"
check "GET /_localstack/health returns 200" \
    curl -sf "${HEALTH_URL}"

check "HEAD /_localstack/health returns 200" \
    curl -sfI "${HEALTH_URL}"

check "GET /_localstack/info returns 200" \
    curl -sf "${ENDPOINT}/_localstack/info"

# ---------------------------------------------------------------------------
# S3 smoke test
# ---------------------------------------------------------------------------

echo
echo "--- S3 ---"
BUCKET="dc-test-bucket-$$"

check "CreateBucket" \
    aws_local s3api create-bucket --bucket "${BUCKET}"

check "HeadBucket" \
    aws_local s3api head-bucket --bucket "${BUCKET}"

check "PutObject" \
    bash -c "echo 'hello world' | aws_local s3 cp - s3://${BUCKET}/test.txt"

check "GetObject" \
    aws_local s3 cp "s3://${BUCKET}/test.txt" -

check "DeleteObject" \
    aws_local s3api delete-object --bucket "${BUCKET}" --key test.txt

check "DeleteBucket" \
    aws_local s3api delete-bucket --bucket "${BUCKET}"

# ---------------------------------------------------------------------------
# SQS smoke test
# ---------------------------------------------------------------------------

echo
echo "--- SQS ---"
QUEUE_NAME="dc-test-queue-$$"

QUEUE_URL=$(aws_local sqs create-queue --queue-name "${QUEUE_NAME}" \
    --query QueueUrl --output text 2>/dev/null || echo "")

check "CreateQueue" test -n "${QUEUE_URL}"

check "SendMessage" \
    aws_local sqs send-message --queue-url "${QUEUE_URL}" --message-body "hello"

check "ReceiveMessage" \
    bash -c "aws_local sqs receive-message --queue-url '${QUEUE_URL}' | grep -q 'hello'"

check "DeleteQueue" \
    aws_local sqs delete-queue --queue-url "${QUEUE_URL}"

# ---------------------------------------------------------------------------
# SNS smoke test
# ---------------------------------------------------------------------------

echo
echo "--- SNS ---"
TOPIC_NAME="dc-test-topic-$$"

TOPIC_ARN=$(aws_local sns create-topic --name "${TOPIC_NAME}" \
    --query TopicArn --output text 2>/dev/null || echo "")

check "CreateTopic" test -n "${TOPIC_ARN}"

check "Publish" \
    aws_local sns publish --topic-arn "${TOPIC_ARN}" --message "hello"

check "DeleteTopic" \
    aws_local sns delete-topic --topic-arn "${TOPIC_ARN}"

# ---------------------------------------------------------------------------
# DynamoDB smoke test
# ---------------------------------------------------------------------------

echo
echo "--- DynamoDB ---"
TABLE_NAME="dc-test-table-$$"

check "CreateTable" \
    aws_local dynamodb create-table \
        --table-name "${TABLE_NAME}" \
        --attribute-definitions AttributeName=id,AttributeType=S \
        --key-schema AttributeName=id,KeyType=HASH \
        --billing-mode PAY_PER_REQUEST

check "PutItem" \
    aws_local dynamodb put-item \
        --table-name "${TABLE_NAME}" \
        --item '{"id":{"S":"1"},"value":{"S":"hello"}}'

check "GetItem" \
    bash -c "aws_local dynamodb get-item \
        --table-name '${TABLE_NAME}' \
        --key '{\"id\":{\"S\":\"1\"}}' | grep -q hello"

check "DeleteTable" \
    aws_local dynamodb delete-table --table-name "${TABLE_NAME}"

# ---------------------------------------------------------------------------
# STS smoke test
# ---------------------------------------------------------------------------

echo
echo "--- STS ---"
check "GetCallerIdentity" \
    bash -c "aws_local sts get-caller-identity | grep -q '${AWS_ACCOUNT}'"

# ---------------------------------------------------------------------------
# KMS smoke test
# ---------------------------------------------------------------------------

echo
echo "--- KMS ---"
KEY_ID=$(aws_local kms create-key --description "dc-test-$$" \
    --query KeyMetadata.KeyId --output text 2>/dev/null || echo "")

check "CreateKey" test -n "${KEY_ID}"

CIPHERTEXT=$(aws_local kms encrypt \
    --key-id "${KEY_ID}" \
    --plaintext "aGVsbG8=" \
    --query CiphertextBlob --output text 2>/dev/null || echo "")

check "Encrypt" test -n "${CIPHERTEXT}"

check "Decrypt" \
    aws_local kms decrypt --ciphertext-blob "${CIPHERTEXT}" --query Plaintext

check "ScheduleKeyDeletion" \
    aws_local kms schedule-key-deletion --key-id "${KEY_ID}" --pending-window-in-days 7

# ---------------------------------------------------------------------------
# Secrets Manager smoke test
# ---------------------------------------------------------------------------

echo
echo "--- Secrets Manager ---"
SECRET_NAME="dc-test-secret-$$"

check "CreateSecret" \
    aws_local secretsmanager create-secret \
        --name "${SECRET_NAME}" \
        --secret-string '{"user":"admin","pass":"secret"}'

check "GetSecretValue" \
    bash -c "aws_local secretsmanager get-secret-value \
        --secret-id '${SECRET_NAME}' | grep -q admin"

check "DeleteSecret (force)" \
    aws_local secretsmanager delete-secret \
        --secret-id "${SECRET_NAME}" \
        --force-delete-without-recovery

# ---------------------------------------------------------------------------
# SSM smoke test
# ---------------------------------------------------------------------------

echo
echo "--- SSM ---"
PARAM_NAME="/dc/test/param-$$"

check "PutParameter" \
    aws_local ssm put-parameter \
        --name "${PARAM_NAME}" \
        --value "hello-from-dc-test" \
        --type String

check "GetParameter" \
    bash -c "aws_local ssm get-parameter --name '${PARAM_NAME}' | grep -q hello"

check "DeleteParameter" \
    aws_local ssm delete-parameter --name "${PARAM_NAME}"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

TOTAL=$((PASS + FAIL))
echo
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [ "${FAIL}" -gt 0 ]; then
    echo "Failed tests:"
    for e in "${ERRORS[@]}"; do
        printf "  - %s\n" "${e}"
    done
    exit 1
else
    echo "All tests passed."
    exit 0
fi
