#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CSS_PATH="$ROOT_DIR/crates/studio-ui/src/styles/input.css"

# Current prebuilt CSS input should stay small while we bootstrap Studio UI.
MAX_BYTES=16384

if [[ ! -f "$CSS_PATH" ]]; then
  echo "Studio CSS file missing: $CSS_PATH"
  exit 1
fi

SIZE_BYTES=$(wc -c < "$CSS_PATH")
echo "Studio CSS bytes: $SIZE_BYTES"

if [[ "$SIZE_BYTES" -gt "$MAX_BYTES" ]]; then
  echo "Studio CSS budget exceeded: $SIZE_BYTES > $MAX_BYTES"
  exit 1
fi

echo "Studio CSS budget OK"
