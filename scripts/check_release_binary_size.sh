#!/usr/bin/env bash
set -euo pipefail

BINARY_PATH="${1:-target/release/openstack}"
MAX_SIZE_MB="${2:-55}"

if [[ ! -f "$BINARY_PATH" ]]; then
  echo "Release binary not found at $BINARY_PATH"
  exit 1
fi

size_bytes=$(stat -c%s "$BINARY_PATH")
max_bytes=$((MAX_SIZE_MB * 1024 * 1024))

size_mb=$(python3 -c "print(round($size_bytes / (1024*1024), 2))")
echo "openstack release binary size: ${size_mb} MB"
echo "size budget: ${MAX_SIZE_MB} MB"

if (( size_bytes > max_bytes )); then
  echo "Binary size budget exceeded"
  exit 1
fi

echo "Binary size within budget"
