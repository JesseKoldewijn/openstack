#!/usr/bin/env bash
# compatibility_tests.sh
#
# Runs awslocal CLI commands against a running openstack server to verify
# real-world usage patterns.
#
# Prerequisites:
#   pip install awscli-local awscli
#   A running openstack server on localhost:4566
#     OR set OPENSTACK_URL to point at the server.
#
# Usage:
#   ./compatibility_tests.sh
#   OPENSTACK_URL=http://localhost:4566 ./compatibility_tests.sh

set -euo pipefail

ENDPOINT="${OPENSTACK_URL:-http://localhost:4566}"
PASS=0
FAIL=0
ERRORS=()

# Configure awslocal / aws cli to hit our endpoint
export AWS_DEFAULT_REGION="us-east-1"
export AWS_ACCESS_KEY_ID="test"
export AWS_SECRET_ACCESS_KEY="test"
export LOCALSTACK_ENDPOINT="${ENDPOINT}"

# Helper: run a test, record pass/fail
run_test() {
    local name="$1"; shift
    echo -n "  [TEST] ${name} ... "
    if "$@" > /dev/null 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL"
        FAIL=$((FAIL + 1))
        ERRORS+=("${name}")
    fi
}

awsl() {
    awslocal --endpoint-url "${ENDPOINT}" "$@"
}

echo "=== openstack compatibility tests ==="
echo "Endpoint: ${ENDPOINT}"
echo

# ── S3 ────────────────────────────────────────────────────────────────────────
echo "--- S3 ---"
run_test "s3 create-bucket"          awsl s3 mb s3://compat-bucket
run_test "s3 put-object"             awsl s3 cp /dev/stdin s3://compat-bucket/hello.txt <<< "hello world"
run_test "s3 get-object (verify)"    bash -c 'content=$(awslocal --endpoint-url '"${ENDPOINT}"' s3 cp s3://compat-bucket/hello.txt -); [ "$content" = "hello world" ]'
run_test "s3 ls bucket"              awsl s3 ls s3://compat-bucket/
run_test "s3 delete-object"          awsl s3 rm s3://compat-bucket/hello.txt
run_test "s3 delete-bucket"          awsl s3 rb s3://compat-bucket

# ── SQS ──────────────────────────────────────────────────────────────────────
echo "--- SQS ---"
run_test "sqs create-queue"          awsl sqs create-queue --queue-name compat-queue
QUEUE_URL=$(awsl sqs get-queue-url --queue-name compat-queue --query QueueUrl --output text 2>/dev/null || true)
run_test "sqs send-message"          awsl sqs send-message --queue-url "${QUEUE_URL}" --message-body "test-msg"
run_test "sqs receive-message"       awsl sqs receive-message --queue-url "${QUEUE_URL}"
run_test "sqs delete-queue"          awsl sqs delete-queue --queue-url "${QUEUE_URL}"

# ── SNS ──────────────────────────────────────────────────────────────────────
echo "--- SNS ---"
run_test "sns create-topic"          awsl sns create-topic --name compat-topic
TOPIC_ARN=$(awsl sns list-topics --query 'Topics[?contains(TopicArn,`compat-topic`)].TopicArn' --output text 2>/dev/null || true)
run_test "sns list-topics"           awsl sns list-topics
run_test "sns delete-topic"          awsl sns delete-topic --topic-arn "${TOPIC_ARN}"

# ── DynamoDB ──────────────────────────────────────────────────────────────────
echo "--- DynamoDB ---"
run_test "dynamodb create-table" \
    awsl dynamodb create-table \
        --table-name compat-table \
        --key-schema AttributeName=pk,KeyType=HASH \
        --attribute-definitions AttributeName=pk,AttributeType=S \
        --billing-mode PAY_PER_REQUEST
run_test "dynamodb put-item" \
    awsl dynamodb put-item \
        --table-name compat-table \
        --item '{"pk":{"S":"k1"},"val":{"S":"v1"}}'
run_test "dynamodb get-item" \
    awsl dynamodb get-item \
        --table-name compat-table \
        --key '{"pk":{"S":"k1"}}'
run_test "dynamodb delete-table" \
    awsl dynamodb delete-table --table-name compat-table

# ── IAM ──────────────────────────────────────────────────────────────────────
echo "--- IAM ---"
run_test "iam create-user"           awsl iam create-user --user-name compat-user
run_test "iam list-users"            awsl iam list-users
run_test "iam delete-user"           awsl iam delete-user --user-name compat-user

# ── STS ──────────────────────────────────────────────────────────────────────
echo "--- STS ---"
run_test "sts get-caller-identity"   awsl sts get-caller-identity

# ── KMS ──────────────────────────────────────────────────────────────────────
echo "--- KMS ---"
KEY_ID=$(awsl kms create-key --description "compat key" --query KeyMetadata.KeyId --output text 2>/dev/null || true)
run_test "kms create-key"            [ -n "${KEY_ID}" ]
run_test "kms describe-key"          awsl kms describe-key --key-id "${KEY_ID}"
run_test "kms list-keys"             awsl kms list-keys

# ── SecretsManager ───────────────────────────────────────────────────────────
echo "--- SecretsManager ---"
run_test "secretsmanager create-secret" \
    awsl secretsmanager create-secret \
        --name compat/secret \
        --secret-string "mysecret"
run_test "secretsmanager get-secret-value" \
    awsl secretsmanager get-secret-value --secret-id compat/secret
run_test "secretsmanager delete-secret" \
    awsl secretsmanager delete-secret --secret-id compat/secret --force-delete-without-recovery

# ── SSM ──────────────────────────────────────────────────────────────────────
echo "--- SSM ---"
run_test "ssm put-parameter" \
    awsl ssm put-parameter \
        --name /compat/param \
        --value "paramval" \
        --type String
run_test "ssm get-parameter"         awsl ssm get-parameter --name /compat/param
run_test "ssm delete-parameter"      awsl ssm delete-parameter --name /compat/param

# ── Summary ──────────────────────────────────────────────────────────────────
echo
echo "=== Results ==="
echo "  PASS: ${PASS}"
echo "  FAIL: ${FAIL}"
if [ ${#ERRORS[@]} -gt 0 ]; then
    echo "  Failed tests:"
    for e in "${ERRORS[@]}"; do
        echo "    - ${e}"
    done
    exit 1
fi
echo "All compatibility tests passed."
