#!/usr/bin/env bash
# bench_gate.sh
#
# Benchmark gate script for openstack.
# Reads a JSON report from bench_services.sh and evaluates:
#   1. Per-operation p95 latency ratio (openstack vs LocalStack)
#   2. Memory budget (openstack/LocalStack RSS ratio)
#   3. Error rate (zero tolerance for openstack errors)
#
# Produces a markdown summary suitable for CI comments.
#
# Exit codes:
#   0 - All checks pass
#   1 - One or more checks failed
#   2 - Invalid input (missing/malformed report)
#
# Usage:
#   ./bench_gate.sh --report report.json
#   ./bench_gate.sh --report report.json --p95-threshold 2.0 --memory-budget 0.10
#   ./bench_gate.sh --report report.json --output-markdown summary.md

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# 6.1 Argument parsing
# ─────────────────────────────────────────────────────────────────────────────

REPORT=""
P95_THRESHOLD="1.5"
MEMORY_BUDGET="0.20"
OUTPUT_MARKDOWN=""

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --report <path>              Path to JSON benchmark report (required)
  --p95-threshold <ratio>      Max openstack/LocalStack p95 ratio (default: 1.5)
  --memory-budget <ratio>      Max openstack/LocalStack RSS ratio (default: 0.20)
  --output-markdown <path>     Write markdown summary to file (default: stdout)
  -h, --help                   Show this help
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --report) REPORT="$2"; shift 2 ;;
    --p95-threshold) P95_THRESHOLD="$2"; shift 2 ;;
    --memory-budget) MEMORY_BUDGET="$2"; shift 2 ;;
    --output-markdown) OUTPUT_MARKDOWN="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "ERROR: Unknown option: $1" >&2; exit 2 ;;
  esac
done

# ─────────────────────────────────────────────────────────────────────────────
# Input validation (exit 2 on invalid input)
# ─────────────────────────────────────────────────────────────────────────────

if [[ -z "$REPORT" ]]; then
  echo "ERROR: --report is required" >&2
  exit 2
fi

if [[ ! -f "$REPORT" ]]; then
  echo "ERROR: Report file not found: $REPORT" >&2
  exit 2
fi

# Validate JSON structure
if ! jq empty "$REPORT" 2>/dev/null; then
  echo "ERROR: Report file is not valid JSON: $REPORT" >&2
  exit 2
fi

# Check required fields exist
if ! jq -e '.results' "$REPORT" >/dev/null 2>&1; then
  echo "ERROR: Report JSON missing 'results' field" >&2
  exit 2
fi

if ! jq -e '.memory' "$REPORT" >/dev/null 2>&1; then
  echo "ERROR: Report JSON missing 'memory' field" >&2
  exit 2
fi

# Check for required tools
if ! command -v jq &>/dev/null; then
  echo "ERROR: Required tool 'jq' not found in PATH." >&2
  exit 2
fi

# ─────────────────────────────────────────────────────────────────────────────
# Read report metadata
# ─────────────────────────────────────────────────────────────────────────────

PROFILE=$(jq -r '.profile // "unknown"' "$REPORT")
MODE=$(jq -r '.mode // "unknown"' "$REPORT")
TIMESTAMP=$(jq -r '.timestamp // "unknown"' "$REPORT")
REQ_COUNT=$(jq -r '.config.requests // "?"' "$REPORT")
CONCURRENCY=$(jq -r '.config.concurrency // "?"' "$REPORT")

# ─────────────────────────────────────────────────────────────────────────────
# 6.2 Per-operation p95 latency ratio evaluation
# ─────────────────────────────────────────────────────────────────────────────

GATE_FAILED=false
LATENCY_FAILURES=()
ERROR_FAILURES=()

# Process each non-skipped result that has both openstack and localstack data
RESULT_COUNT=$(jq '[.results[] | select(.operation != "SKIPPED" and .openstack != null and .localstack != null)] | length' "$REPORT")

for i in $(seq 0 $(( RESULT_COUNT - 1 ))); do
  SERVICE=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].service" "$REPORT")
  OPERATION=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].operation" "$REPORT")
  OS_P95=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.p95_ms" "$REPORT")
  LS_P95=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].localstack.p95_ms" "$REPORT")
  OS_ERRORS=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.errors" "$REPORT")

  # 6.2: Check p95 latency ratio
  if [[ "$LS_P95" != "0" && "$LS_P95" != "null" ]]; then
    RATIO=$(awk "BEGIN {printf \"%.2f\", $OS_P95 / $LS_P95}")
    EXCEEDS=$(awk "BEGIN {print ($RATIO > $P95_THRESHOLD) ? \"yes\" : \"no\"}")
    if [[ "$EXCEEDS" == "yes" ]]; then
      GATE_FAILED=true
      LATENCY_FAILURES+=("$SERVICE/$OPERATION: openstack=${OS_P95}ms vs LocalStack=${LS_P95}ms (ratio=${RATIO}x, threshold=${P95_THRESHOLD}x)")
    fi
  fi

  # 6.4: Check error rate
  if [[ "$OS_ERRORS" != "0" && "$OS_ERRORS" != "null" ]]; then
    GATE_FAILED=true
    ERROR_FAILURES+=("$SERVICE/$OPERATION: openstack errors=$OS_ERRORS")
  fi
done

# ─────────────────────────────────────────────────────────────────────────────
# 6.3 Memory budget evaluation
# ─────────────────────────────────────────────────────────────────────────────

MEMORY_FAILED=false
MEMORY_FAILURE_MSG=""

OS_LOADED_MB=$(jq -r '.memory.openstack.loaded_mb // 0' "$REPORT")
LS_LOADED_MB=$(jq -r '.memory.localstack.loaded_mb // 0' "$REPORT")
OS_IDLE_MB=$(jq -r '.memory.openstack.idle_mb // 0' "$REPORT")
LS_IDLE_MB=$(jq -r '.memory.localstack.idle_mb // 0' "$REPORT")

if [[ "$LS_LOADED_MB" != "0" && "$LS_LOADED_MB" != "null" && "$OS_LOADED_MB" != "null" ]]; then
  MEM_RATIO=$(awk "BEGIN {printf \"%.4f\", $OS_LOADED_MB / $LS_LOADED_MB}")
  MEM_EXCEEDS=$(awk "BEGIN {print ($MEM_RATIO > $MEMORY_BUDGET) ? \"yes\" : \"no\"}")
  if [[ "$MEM_EXCEEDS" == "yes" ]]; then
    GATE_FAILED=true
    MEMORY_FAILED=true
    MEMORY_FAILURE_MSG="openstack=${OS_LOADED_MB}MB vs LocalStack=${LS_LOADED_MB}MB (ratio=${MEM_RATIO}, budget=${MEMORY_BUDGET})"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 6.5 Markdown summary generation
# ─────────────────────────────────────────────────────────────────────────────

generate_markdown() {
  local verdict="PASS"
  local verdict_emoji=""
  local failure_count=0

  if $GATE_FAILED; then
    verdict="FAIL"
    failure_count=$(( ${#LATENCY_FAILURES[@]} + ${#ERROR_FAILURES[@]} + ($MEMORY_FAILED && echo 1 || echo 0) ))
  fi

  cat <<EOF
## Benchmark Gate: $verdict

**Profile:** $PROFILE | **Mode:** $MODE | **Requests:** $REQ_COUNT | **Concurrency:** $CONCURRENCY
**Timestamp:** $TIMESTAMP
**Thresholds:** p95 ratio <= ${P95_THRESHOLD}x | Memory ratio <= ${MEMORY_BUDGET}

### Memory Comparison

| Target | Idle RSS (MB) | Loaded RSS (MB) |
|--------|---------------|-----------------|
| openstack | $OS_IDLE_MB | $OS_LOADED_MB |
| LocalStack | $LS_IDLE_MB | $LS_LOADED_MB |
EOF

  if [[ "$LS_LOADED_MB" != "0" && "$LS_LOADED_MB" != "null" && "$OS_LOADED_MB" != "null" ]]; then
    echo "| **Ratio (OS/LS)** | $(awk "BEGIN {printf \"%.4f\", $OS_IDLE_MB / ($LS_IDLE_MB == 0 ? 1 : $LS_IDLE_MB)}") | $MEM_RATIO |"
  fi

  if $MEMORY_FAILED; then
    echo ""
    echo "> **FAIL:** Memory budget exceeded: $MEMORY_FAILURE_MSG"
  fi

  echo ""
  echo "### Per-Operation Metrics"
  echo ""
  echo "| Service | Operation | OS p50 | OS p95 | OS p99 | LS p50 | LS p95 | LS p99 | p95 Ratio | OS RPS | LS RPS | Status |"
  echo "|---------|-----------|--------|--------|--------|--------|--------|--------|-----------|--------|--------|--------|"

  # Iterate through all non-skipped results
  for i in $(seq 0 $(( RESULT_COUNT - 1 ))); do
    local svc op os_p50 os_p95 os_p99 ls_p50 ls_p95 ls_p99 os_rps ls_rps os_errs ratio status
    svc=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].service" "$REPORT")
    op=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].operation" "$REPORT")
    os_p50=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.p50_ms" "$REPORT")
    os_p95=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.p95_ms" "$REPORT")
    os_p99=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.p99_ms" "$REPORT")
    ls_p50=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].localstack.p50_ms" "$REPORT")
    ls_p95=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].localstack.p95_ms" "$REPORT")
    ls_p99=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].localstack.p99_ms" "$REPORT")
    os_rps=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.throughput_rps" "$REPORT")
    ls_rps=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].localstack.throughput_rps" "$REPORT")
    os_errs=$(jq -r "[.results[] | select(.operation != \"SKIPPED\" and .openstack != null and .localstack != null)][$i].openstack.errors" "$REPORT")

    # Calculate p95 ratio
    if [[ "$ls_p95" != "0" && "$ls_p95" != "null" ]]; then
      ratio=$(awk "BEGIN {printf \"%.2f\", $os_p95 / $ls_p95}")
    else
      ratio="N/A"
    fi

    # Determine status
    status="PASS"
    if [[ "$os_errs" != "0" && "$os_errs" != "null" ]]; then
      status="FAIL (errors)"
    elif [[ "$ratio" != "N/A" ]]; then
      local exceeds
      exceeds=$(awk "BEGIN {print ($ratio > $P95_THRESHOLD) ? \"yes\" : \"no\"}")
      if [[ "$exceeds" == "yes" ]]; then
        status="FAIL (p95)"
      fi
    fi

    echo "| $svc | $op | ${os_p50}ms | ${os_p95}ms | ${os_p99}ms | ${ls_p50}ms | ${ls_p95}ms | ${ls_p99}ms | ${ratio}x | $os_rps | $ls_rps | $status |"
  done

  # Show skipped services
  SKIP_COUNT=$(jq '[.results[] | select(.operation == "SKIPPED")] | length' "$REPORT")
  if [[ "$SKIP_COUNT" -gt 0 ]]; then
    echo ""
    echo "### Skipped Services"
    echo ""
    for i in $(seq 0 $(( SKIP_COUNT - 1 ))); do
      local skip_svc skip_reason
      skip_svc=$(jq -r "[.results[] | select(.operation == \"SKIPPED\")][$i].service" "$REPORT")
      skip_reason=$(jq -r "[.results[] | select(.operation == \"SKIPPED\")][$i].skip_reason" "$REPORT")
      echo "- **$skip_svc**: $skip_reason"
    done
  fi

  # Show failures summary
  if $GATE_FAILED; then
    echo ""
    echo "### Failures"
    echo ""

    if [[ ${#LATENCY_FAILURES[@]} -gt 0 ]]; then
      echo "**Latency threshold exceeded (p95 ratio > ${P95_THRESHOLD}x):**"
      for f in "${LATENCY_FAILURES[@]}"; do
        echo "- $f"
      done
      echo ""
    fi

    if [[ ${#ERROR_FAILURES[@]} -gt 0 ]]; then
      echo "**Non-zero error rates:**"
      for f in "${ERROR_FAILURES[@]}"; do
        echo "- $f"
      done
      echo ""
    fi

    if $MEMORY_FAILED; then
      echo "**Memory budget exceeded:**"
      echo "- $MEMORY_FAILURE_MSG"
      echo ""
    fi

    local total_failures=$(( ${#LATENCY_FAILURES[@]} + ${#ERROR_FAILURES[@]} ))
    $MEMORY_FAILED && total_failures=$(( total_failures + 1 ))
    echo "**Total failures: $total_failures**"
  fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Output markdown summary
# ─────────────────────────────────────────────────────────────────────────────

MARKDOWN=$(generate_markdown)

if [[ -n "$OUTPUT_MARKDOWN" ]]; then
  echo "$MARKDOWN" > "$OUTPUT_MARKDOWN"
  echo "[gate] Markdown summary written to: $OUTPUT_MARKDOWN" >&2
else
  echo "$MARKDOWN"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 6.6 Exit codes
# ─────────────────────────────────────────────────────────────────────────────

if $GATE_FAILED; then
  echo "[gate] FAIL — gate checks did not pass" >&2
  exit 1
else
  echo "[gate] PASS — all checks passed" >&2
  exit 0
fi
