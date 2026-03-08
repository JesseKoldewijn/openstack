#!/usr/bin/env bash
# run_terraform_test.sh
#
# Runs the Terraform compatibility test against a running openstack server.
#
# Prerequisites:
#   - terraform CLI installed
#   - openstack server running on OPENSTACK_URL (default: http://localhost:4566)
#
# Usage:
#   ./run_terraform_test.sh
#   OPENSTACK_URL=http://localhost:4566 ./run_terraform_test.sh

set -euo pipefail

ENDPOINT="${OPENSTACK_URL:-http://localhost:4566}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== Terraform compatibility test ==="
echo "Endpoint: ${ENDPOINT}"
echo

cd "${SCRIPT_DIR}"

# Init
echo "--- terraform init ---"
terraform init -input=false

# Plan
echo "--- terraform plan ---"
terraform plan \
    -var="openstack_endpoint=${ENDPOINT}" \
    -input=false \
    -out=tfplan

# Apply
echo "--- terraform apply ---"
terraform apply \
    -input=false \
    -auto-approve \
    tfplan

# Show outputs
echo "--- terraform output ---"
terraform output

# Destroy
echo "--- terraform destroy ---"
terraform destroy \
    -var="openstack_endpoint=${ENDPOINT}" \
    -input=false \
    -auto-approve

echo
echo "=== Terraform test passed ==="
