#!/usr/bin/env bash
# bench_gate.sh
#
# Benchmark gate script for openstack.
# Reads a raw JSON report from bench_services.sh and evaluates:
#   1. Per-operation p95 latency: absolute ceiling on openstack only
#   2. Openstack loaded RSS: absolute memory ceiling
#   3. Error rate: zero tolerance for openstack errors (with optional ignore list)
#
# Per-operation latency overrides (--op-p95-max) let body-transfer operations be
# gated against a size-appropriate threshold instead of the global metadata ceiling.
# When a per-op threshold is set, it becomes the mandatory gate for that operation
# and the global --p95-max no longer applies to it.
#
# LocalStack and Moto data is included in the output as comparison context
# (to demonstrate how much faster openstack is) but never used for gating.
#
# Outputs a structured JSON evaluation report to stdout or --output file.
# All markdown rendering is handled downstream (CI action).
#
# Exit codes:
#   0 - All gate checks pass
#   1 - One or more gate checks failed
#   2 - Invalid input (missing/malformed report or bad arguments)
#
# Usage:
#   ./bench_gate.sh --report report.json
#   ./bench_gate.sh --report report.json --p95-max 5 --memory-max 10
#   ./bench_gate.sh --report report.json --ignore-errors iam/create_user
#   ./bench_gate.sh --report report.json --op-p95-max "s3/put_object_1mb=50,s3/get_object_1mb=50"
#   ./bench_gate.sh --report report.json --output benchmark-gate.json

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# Argument parsing
# ─────────────────────────────────────────────────────────────────────────────

REPORT=""
P95_MAX="5"
MEMORY_MAX="10"
IGNORE_ERRORS=""
IGNORE_LATENCY=""
OP_P95_MAX=""
OUTPUT=""

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --report <path>              Path to JSON benchmark report from bench_services.sh (required)
  --p95-max <ms>               Global p95 ceiling for openstack in milliseconds (default: 5)
  --memory-max <mb>            Absolute loaded RSS ceiling for openstack in MB (default: 10)
  --ignore-errors <list>       Comma-separated service/operation pairs to skip error check
                               e.g. "iam/create_user,s3/put_object"
  --ignore-latency <list>      Comma-separated service/operation pairs to skip p95 latency check
                               entirely (no latency gate applied). Prefer --op-p95-max for a
                               size-appropriate threshold instead of skipping entirely.
                               e.g. "s3/put_object_10mb,s3/get_object_10mb"
  --op-p95-max <list>          Comma-separated per-operation p95 overrides in the form
                               "service/operation=<ms>". When set, this threshold replaces the
                               global --p95-max for that operation; the global gate no longer
                               applies. Use this for body-transfer operations whose latency
                               scales with payload size.
                               e.g. "s3/put_object_1mb=50,s3/get_object_1mb=50,s3/put_object_10mb=200"
  --output <path>              Write gate JSON to file (default: stdout)
  -h, --help                   Show this help
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --report)          REPORT="$2";          shift 2 ;;
    --p95-max)         P95_MAX="$2";         shift 2 ;;
    --memory-max)      MEMORY_MAX="$2";      shift 2 ;;
    --ignore-errors)   IGNORE_ERRORS="$2";   shift 2 ;;
    --ignore-latency)  IGNORE_LATENCY="$2";  shift 2 ;;
    --op-p95-max)      OP_P95_MAX="$2";      shift 2 ;;
    --output)          OUTPUT="$2";          shift 2 ;;
    -h|--help) usage ;;
    *) echo "ERROR: Unknown option: $1" >&2; exit 2 ;;
  esac
done

# ─────────────────────────────────────────────────────────────────────────────
# Input validation
# ─────────────────────────────────────────────────────────────────────────────

if [[ -z "$REPORT" ]]; then
  echo "ERROR: --report is required" >&2
  exit 2
fi

if [[ ! -f "$REPORT" ]]; then
  echo "ERROR: Report file not found: $REPORT" >&2
  exit 2
fi

if ! command -v jq &>/dev/null; then
  echo "ERROR: Required tool 'jq' not found in PATH." >&2
  exit 2
fi

if ! jq empty "$REPORT" 2>/dev/null; then
  echo "ERROR: Report file is not valid JSON: $REPORT" >&2
  exit 2
fi

if ! jq -e '.results' "$REPORT" >/dev/null 2>&1; then
  echo "ERROR: Report JSON missing 'results' field" >&2
  exit 2
fi

if ! jq -e '.memory' "$REPORT" >/dev/null 2>&1; then
  echo "ERROR: Report JSON missing 'memory' field" >&2
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

# Detect active targets
HAS_LS=false
HAS_MOTO=false
if jq -e '.targets | index("ls")' "$REPORT" >/dev/null 2>&1; then
  HAS_LS=true
fi
if jq -e '.targets | index("moto")' "$REPORT" >/dev/null 2>&1; then
  HAS_MOTO=true
fi

TARGETS_JSON=$(jq -c '.targets // ["os","ls"]' "$REPORT")

# ─────────────────────────────────────────────────────────────────────────────
# Build ignore-errors set (bash associative array keyed by "service/operation")
# ─────────────────────────────────────────────────────────────────────────────

declare -A IGNORE_SET
if [[ -n "$IGNORE_ERRORS" ]]; then
  IFS=',' read -ra _ignore_list <<< "$IGNORE_ERRORS"
  for _entry in "${_ignore_list[@]}"; do
    _entry="${_entry// /}"  # trim spaces
    IGNORE_SET["$_entry"]=1
  done
fi

# Build JSON array of ignored entries for output
IGNORED_JSON=$(
  if [[ -n "$IGNORE_ERRORS" ]]; then
    IFS=',' read -ra _items <<< "$IGNORE_ERRORS"
    printf '%s\n' "${_items[@]}" | jq -R . | jq -s .
  else
    echo "[]"
  fi
)

# ─────────────────────────────────────────────────────────────────────────────
# Build ignore-latency set (backward-compat: skip latency gate entirely)
# ─────────────────────────────────────────────────────────────────────────────

declare -A IGNORE_LATENCY_SET
if [[ -n "$IGNORE_LATENCY" ]]; then
  IFS=',' read -ra _ignore_lat_list <<< "$IGNORE_LATENCY"
  for _entry in "${_ignore_lat_list[@]}"; do
    _entry="${_entry// /}"  # trim spaces
    IGNORE_LATENCY_SET["$_entry"]=1
  done
fi

# Build JSON array of latency-ignored entries for output
IGNORED_LATENCY_JSON=$(
  if [[ -n "$IGNORE_LATENCY" ]]; then
    IFS=',' read -ra _items <<< "$IGNORE_LATENCY"
    printf '%s\n' "${_items[@]}" | jq -R . | jq -s .
  else
    echo "[]"
  fi
)

# ─────────────────────────────────────────────────────────────────────────────
# Build per-operation p95 override map (--op-p95-max "svc/op=ms,svc/op=ms")
# When set for an operation, replaces the global --p95-max for that operation.
# ─────────────────────────────────────────────────────────────────────────────

declare -A OP_P95_MAX_SET   # key: "service/operation", value: threshold_ms
OP_P95_MAX_JSON="{}"        # JSON object for report output

if [[ -n "$OP_P95_MAX" ]]; then
  IFS=',' read -ra _op_list <<< "$OP_P95_MAX"
  _op_json_parts=()
  for _entry in "${_op_list[@]}"; do
    _entry="${_entry// /}"   # trim spaces
    _op_key="${_entry%%=*}"
    _op_val="${_entry##*=}"
    if [[ -z "$_op_key" || -z "$_op_val" ]]; then
      echo "ERROR: --op-p95-max entry malformed (expected service/op=ms): '$_entry'" >&2
      exit 2
    fi
    if ! awk "BEGIN {exit !($_op_val > 0)}" 2>/dev/null; then
      echo "ERROR: --op-p95-max threshold must be a positive number: '$_op_val'" >&2
      exit 2
    fi
    OP_P95_MAX_SET["$_op_key"]="$_op_val"
    _op_json_parts+=("$(jq -n --arg k "$_op_key" --argjson v "$_op_val" '{($k):$v}')")
  done
  if [[ ${#_op_json_parts[@]} -gt 0 ]]; then
    OP_P95_MAX_JSON=$(printf '%s\n' "${_op_json_parts[@]}" | jq -s 'add // {}')
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# Memory evaluation
# ─────────────────────────────────────────────────────────────────────────────

OS_IDLE_MB=$(jq -r  '.memory.openstack.idle_mb   // 0' "$REPORT")
OS_LOADED_MB=$(jq -r '.memory.openstack.loaded_mb // 0' "$REPORT")
LS_IDLE_MB=0; LS_LOADED_MB=0
MOTO_IDLE_MB=0; MOTO_LOADED_MB=0

$HAS_LS   && LS_IDLE_MB=$(jq -r '.memory.localstack.idle_mb   // 0' "$REPORT") \
          && LS_LOADED_MB=$(jq -r '.memory.localstack.loaded_mb // 0' "$REPORT")
$HAS_MOTO && MOTO_IDLE_MB=$(jq -r '.memory.moto.idle_mb   // 0' "$REPORT") \
          && MOTO_LOADED_MB=$(jq -r '.memory.moto.loaded_mb // 0' "$REPORT")

MEMORY_GATE_PASS=true
MEMORY_FAILURE_MSG=""
if [[ "$OS_LOADED_MB" != "null" ]] && awk "BEGIN {exit !($OS_LOADED_MB > $MEMORY_MAX)}"; then
  MEMORY_GATE_PASS=false
  MEMORY_FAILURE_MSG="openstack loaded RSS ${OS_LOADED_MB}MB exceeds ${MEMORY_MAX}MB ceiling"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Per-operation evaluation
# ─────────────────────────────────────────────────────────────────────────────

GATE_FAILED=false
$MEMORY_GATE_PASS || GATE_FAILED=true

# Accumulate JSON fragments using temp arrays
LATENCY_FAILURES=()
ERROR_FAILURES=()
MEMORY_FAILURES=()
$MEMORY_GATE_PASS || MEMORY_FAILURES+=("$MEMORY_FAILURE_MSG")

# Get all unique services (preserving order, skipped entries included)
SERVICES_ORDER=$(jq -r '[.results[].service] | unique | .[]' "$REPORT")

# Build services JSON object — we'll accumulate a jq-compatible string
SERVICES_JSON_PARTS=()

# Global speedup accumulators across all services/operations (for overall section)
declare -a ALL_LS_P50_SU=()
declare -a ALL_LS_P95_SU=()
declare -a ALL_LS_P99_SU=()
declare -a ALL_LS_RPS_SU=()
declare -a ALL_MOTO_P50_SU=()
declare -a ALL_MOTO_P95_SU=()
declare -a ALL_MOTO_P99_SU=()
declare -a ALL_MOTO_RPS_SU=()

# Helper: compute min/max/avg stats from a bash array (passed by name-ref).
# Prints a JSON object {min, max, avg} or "null" if the array is empty.
compute_stats() {
  local -n _arr=$1
  if [[ ${#_arr[@]} -eq 0 ]]; then
    echo "null"
    return
  fi
  local min max sum count
  min="${_arr[0]}"; max="${_arr[0]}"; sum=0; count=0
  for v in "${_arr[@]}"; do
    if awk "BEGIN {exit !($v < $min)}"; then min="$v"; fi
    if awk "BEGIN {exit !($v > $max)}"; then max="$v"; fi
    sum=$(awk "BEGIN {printf \"%.4f\", $sum + $v}")
    count=$(( count + 1 ))
  done
  local avg
  avg=$(awk "BEGIN {printf \"%.2f\", $sum / $count}")
  min=$(awk "BEGIN {printf \"%.2f\", $min}")
  max=$(awk "BEGIN {printf \"%.2f\", $max}")
  jq -n --argjson mn "$min" --argjson mx "$max" --argjson av "$avg" \
    '{min:$mn, max:$mx, avg:$av}'
}

# Helper: given two numeric strings a and b (both > 0), compute ratio a/b
# formatted to 2 decimal places. Prints "null" if either is 0 or non-numeric.
ratio() {
  local a="$1" b="$2"
  if [[ "$a" == "null" || "$b" == "null" ]] \
     || ! awk "BEGIN {exit !($a > 0 && $b > 0)}"; then
    echo "null"
  else
    awk "BEGIN {printf \"%.4f\", $a / $b}"
  fi
}

while IFS= read -r SERVICE; do
  # Collect all benchmarkable operations for this service (exclude meta-entries)
  OPS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation != \"SKIPPED\" and .operation != \"SEED_FAILED\")] | .[].operation" "$REPORT")

  if [[ -z "$OPS" ]]; then
    # Only skipped entries for this service — handled in skipped section
    continue
  fi

  OPS_JSON_PARTS=()

  # Per-service speedup accumulators — one array per metric per competitor
  declare -a LS_P50_SU=()
  declare -a LS_P95_SU=()
  declare -a LS_P99_SU=()
  declare -a LS_RPS_SU=()
  declare -a MOTO_P50_SU=()
  declare -a MOTO_P95_SU=()
  declare -a MOTO_P99_SU=()
  declare -a MOTO_RPS_SU=()

  while IFS= read -r OPERATION; do
    KEY="$SERVICE/$OPERATION"

    # Read openstack metrics
    OS_P50=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].openstack.p50_ms // 0" "$REPORT")
    OS_P95=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].openstack.p95_ms // 0" "$REPORT")
    OS_P99=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].openstack.p99_ms // 0" "$REPORT")
    OS_RPS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].openstack.throughput_rps // 0" "$REPORT")
    OS_ERRORS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].openstack.errors // 0" "$REPORT")

    # Gate checks for this operation
    OP_FAILURES_JSON="[]"
    OP_GATE_PASS=true

    # 1. p95 latency gate — three possible modes per operation:
    #    a) --op-p95-max set: use per-op threshold (mandatory, overrides global)
    #    b) --ignore-latency set: skip latency gate entirely (backward compat)
    #    c) default: apply global --p95-max
    LATENCY_IGNORED=false
    LATENCY_THRESHOLD="$P95_MAX"
    LATENCY_THRESHOLD_SOURCE="global"

    if [[ -n "${OP_P95_MAX_SET[$KEY]+x}" ]]; then
      # Per-op threshold takes precedence — global gate replaced for this op
      LATENCY_THRESHOLD="${OP_P95_MAX_SET[$KEY]}"
      LATENCY_THRESHOLD_SOURCE="per_op"
    elif [[ -n "${IGNORE_LATENCY_SET[$KEY]+x}" ]]; then
      LATENCY_IGNORED=true
    fi

    if [[ "$LATENCY_IGNORED" == "false" ]]; then
      if [[ "$OS_P95" != "null" ]] && awk "BEGIN {exit !($OS_P95 > $LATENCY_THRESHOLD)}"; then
        OP_GATE_PASS=false
        GATE_FAILED=true
        _msg="${KEY}: p95=${OS_P95}ms exceeds ${LATENCY_THRESHOLD}ms threshold (${LATENCY_THRESHOLD_SOURCE})"
        LATENCY_FAILURES+=("$_msg")
        OP_FAILURES_JSON=$(echo "$OP_FAILURES_JSON" | jq -c --arg m "$_msg" '. + [$m]')
      fi
    fi

    # 2. Error check (unless ignored)
    ERROR_IGNORED=false
    if [[ -n "${IGNORE_SET[$KEY]+x}" ]]; then
      ERROR_IGNORED=true
    elif [[ "$OS_ERRORS" != "0" && "$OS_ERRORS" != "null" ]]; then
      OP_GATE_PASS=false
      GATE_FAILED=true
      _msg="${KEY}: openstack errors=${OS_ERRORS}"
      ERROR_FAILURES+=("$_msg")
      OP_FAILURES_JSON=$(echo "$OP_FAILURES_JSON" | jq -c --arg m "$_msg" '. + [$m]')
    fi

    # Build target JSON sub-objects
    OS_JSON=$(jq -n \
      --argjson p50 "$OS_P50" \
      --argjson p95 "$OS_P95" \
      --argjson p99 "$OS_P99" \
      --argjson rps "$OS_RPS" \
      --argjson err "$OS_ERRORS" \
      '{p50_ms:$p50, p95_ms:$p95, p99_ms:$p99, rps:$rps, errors:$err}')

    LS_JSON="null"
    LS_OP_SPEEDUP_JSON="null"
    if $HAS_LS; then
      LS_P50=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].localstack.p50_ms // null" "$REPORT")
      LS_P95=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].localstack.p95_ms // null" "$REPORT")
      LS_P99=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].localstack.p99_ms // null" "$REPORT")
      LS_RPS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].localstack.throughput_rps // null" "$REPORT")
      LS_ERRORS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].localstack.errors // null" "$REPORT")
      if [[ "$LS_P50" != "null" ]]; then
        LS_JSON=$(jq -n \
          --argjson p50 "$LS_P50" \
          --argjson p95 "${LS_P95:-null}" \
          --argjson p99 "${LS_P99:-null}" \
          --argjson rps "${LS_RPS:-null}" \
          --argjson err "${LS_ERRORS:-null}" \
          '{p50_ms:$p50, p95_ms:$p95, p99_ms:$p99, rps:$rps, errors:$err}')

        # Per-operation speedup ratios:
        #   latency: competitor / openstack  (higher = openstack is faster)
        #   rps:     openstack / competitor  (higher = openstack has higher throughput)
        _su_p50=$(ratio "$LS_P50"  "$OS_P50")
        _su_p95=$(ratio "$LS_P95"  "$OS_P95")
        _su_p99=$(ratio "$LS_P99"  "$OS_P99")
        _su_rps=$(ratio "$OS_RPS"  "$LS_RPS")

        # Accumulate valid ratios into per-service and global arrays
        [[ "$_su_p50" != "null" ]] && LS_P50_SU+=("$_su_p50")  && ALL_LS_P50_SU+=("$_su_p50")
        [[ "$_su_p95" != "null" ]] && LS_P95_SU+=("$_su_p95")  && ALL_LS_P95_SU+=("$_su_p95")
        [[ "$_su_p99" != "null" ]] && LS_P99_SU+=("$_su_p99")  && ALL_LS_P99_SU+=("$_su_p99")
        [[ "$_su_rps" != "null" ]] && LS_RPS_SU+=("$_su_rps")  && ALL_LS_RPS_SU+=("$_su_rps")

        LS_OP_SPEEDUP_JSON=$(jq -n \
          --argjson p50 "${_su_p50:-null}" \
          --argjson p95 "${_su_p95:-null}" \
          --argjson p99 "${_su_p99:-null}" \
          --argjson rps "${_su_rps:-null}" \
          '{p50:$p50, p95:$p95, p99:$p99, rps:$rps}')
      fi
    fi

    MOTO_JSON="null"
    MOTO_OP_SPEEDUP_JSON="null"
    if $HAS_MOTO; then
      MOTO_P50=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].moto.p50_ms // null" "$REPORT")
      MOTO_P95=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].moto.p95_ms // null" "$REPORT")
      MOTO_P99=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].moto.p99_ms // null" "$REPORT")
      MOTO_RPS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].moto.throughput_rps // null" "$REPORT")
      MOTO_ERRORS=$(jq -r "[.results[] | select(.service == \"$SERVICE\" and .operation == \"$OPERATION\")][0].moto.errors // null" "$REPORT")
      if [[ "$MOTO_P50" != "null" ]]; then
        MOTO_JSON=$(jq -n \
          --argjson p50 "$MOTO_P50" \
          --argjson p95 "${MOTO_P95:-null}" \
          --argjson p99 "${MOTO_P99:-null}" \
          --argjson rps "${MOTO_RPS:-null}" \
          --argjson err "${MOTO_ERRORS:-null}" \
          '{p50_ms:$p50, p95_ms:$p95, p99_ms:$p99, rps:$rps, errors:$err}')

        _su_p50=$(ratio "$MOTO_P50" "$OS_P50")
        _su_p95=$(ratio "$MOTO_P95" "$OS_P95")
        _su_p99=$(ratio "$MOTO_P99" "$OS_P99")
        _su_rps=$(ratio "$OS_RPS"   "$MOTO_RPS")

        [[ "$_su_p50" != "null" ]] && MOTO_P50_SU+=("$_su_p50") && ALL_MOTO_P50_SU+=("$_su_p50")
        [[ "$_su_p95" != "null" ]] && MOTO_P95_SU+=("$_su_p95") && ALL_MOTO_P95_SU+=("$_su_p95")
        [[ "$_su_p99" != "null" ]] && MOTO_P99_SU+=("$_su_p99") && ALL_MOTO_P99_SU+=("$_su_p99")
        [[ "$_su_rps" != "null" ]] && MOTO_RPS_SU+=("$_su_rps") && ALL_MOTO_RPS_SU+=("$_su_rps")

        MOTO_OP_SPEEDUP_JSON=$(jq -n \
          --argjson p50 "${_su_p50:-null}" \
          --argjson p95 "${_su_p95:-null}" \
          --argjson p99 "${_su_p99:-null}" \
          --argjson rps "${_su_rps:-null}" \
          '{p50:$p50, p95:$p95, p99:$p99, rps:$rps}')
      fi
    fi

    # Assemble operation JSON
    OP_JSON=$(jq -n \
      --argjson os         "$OS_JSON" \
      --argjson ls         "$LS_JSON" \
      --argjson moto       "$MOTO_JSON" \
      --argjson ls_su      "$LS_OP_SPEEDUP_JSON" \
      --argjson moto_su    "$MOTO_OP_SPEEDUP_JSON" \
      --argjson pass       "$([ "$OP_GATE_PASS"      == "true" ] && echo true || echo false)" \
      --argjson errig      "$([ "$ERROR_IGNORED"     == "true" ] && echo true || echo false)" \
      --argjson latig      "$([ "$LATENCY_IGNORED"   == "true" ] && echo true || echo false)" \
      --argjson thresh     "$LATENCY_THRESHOLD" \
      --arg     thresh_src "$LATENCY_THRESHOLD_SOURCE" \
      --argjson failures   "$OP_FAILURES_JSON" \
      '{openstack:$os, localstack:$ls, moto:$moto,
        speedup_vs_localstack:$ls_su, speedup_vs_moto:$moto_su,
        gate_pass:$pass, error_ignored:$errig, latency_ignored:$latig,
        p95_threshold_ms:$thresh, p95_threshold_source:$thresh_src,
        gate_failures:$failures}')

    OPS_JSON_PARTS+=("$(jq -n --arg k "$OPERATION" --argjson v "$OP_JSON" '{($k):$v}')")
  done <<< "$OPS"

  # Merge all operations into a single object
  OPS_MERGED=$(printf '%s\n' "${OPS_JSON_PARTS[@]}" | jq -s 'add // {}')

  # Collect seed failures for this service from the report
  SVC_SEED_FAILURES_JSON=$(jq -c \
    "[.results[] | select(.service == \"$SERVICE\" and .operation == \"SEED_FAILED\") | {target:.target, reason:.seed_reason}]" \
    "$REPORT")

  # Compute per-service speedup stats for each metric
  _build_speedup_obj() {
    # args: p50_arr_name p95_arr_name p99_arr_name rps_arr_name
    local p50_json p95_json p99_json rps_json
    p50_json=$(compute_stats "$1")
    p95_json=$(compute_stats "$2")
    p99_json=$(compute_stats "$3")
    rps_json=$(compute_stats "$4")
    if [[ "$p50_json" == "null" && "$p95_json" == "null" \
          && "$p99_json" == "null" && "$rps_json" == "null" ]]; then
      echo "null"
    else
      jq -n \
        --argjson p50 "$p50_json" \
        --argjson p95 "$p95_json" \
        --argjson p99 "$p99_json" \
        --argjson rps "$rps_json" \
        '{p50:$p50, p95:$p95, p99:$p99, rps:$rps}'
    fi
  }

  LS_SPEEDUP_JSON="null"
  MOTO_SPEEDUP_JSON="null"
  $HAS_LS   && LS_SPEEDUP_JSON=$(  _build_speedup_obj LS_P50_SU   LS_P95_SU   LS_P99_SU   LS_RPS_SU)
  $HAS_MOTO && MOTO_SPEEDUP_JSON=$(_build_speedup_obj MOTO_P50_SU MOTO_P95_SU MOTO_P99_SU MOTO_RPS_SU)

  SVC_JSON=$(jq -n \
    --argjson ops     "$OPS_MERGED" \
    --argjson ls_su   "$LS_SPEEDUP_JSON" \
    --argjson moto_su "$MOTO_SPEEDUP_JSON" \
    --argjson sf      "$SVC_SEED_FAILURES_JSON" \
    '{operations:$ops, speedup_vs_localstack:$ls_su, speedup_vs_moto:$moto_su, seed_failures:$sf}')

  SERVICES_JSON_PARTS+=("$(jq -n --arg k "$SERVICE" --argjson v "$SVC_JSON" '{($k):$v}')")

  unset LS_P50_SU LS_P95_SU LS_P99_SU LS_RPS_SU
  unset MOTO_P50_SU MOTO_P95_SU MOTO_P99_SU MOTO_RPS_SU
  declare -a LS_P50_SU=() LS_P95_SU=() LS_P99_SU=() LS_RPS_SU=()
  declare -a MOTO_P50_SU=() MOTO_P95_SU=() MOTO_P99_SU=() MOTO_RPS_SU=()
done <<< "$SERVICES_ORDER"

# Merge all services into a single object
if [[ ${#SERVICES_JSON_PARTS[@]} -gt 0 ]]; then
  SERVICES_MERGED=$(printf '%s\n' "${SERVICES_JSON_PARTS[@]}" | jq -s 'add // {}')
else
  SERVICES_MERGED="{}"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Overall speedup (across all services and operations)
# ─────────────────────────────────────────────────────────────────────────────

OVERALL_LS_JSON="null"
OVERALL_MOTO_JSON="null"

if $HAS_LS; then
  OVERALL_LS_JSON=$(_build_speedup_obj ALL_LS_P50_SU ALL_LS_P95_SU ALL_LS_P99_SU ALL_LS_RPS_SU)
fi
if $HAS_MOTO; then
  OVERALL_MOTO_JSON=$(_build_speedup_obj ALL_MOTO_P50_SU ALL_MOTO_P95_SU ALL_MOTO_P99_SU ALL_MOTO_RPS_SU)
fi

OVERALL_JSON=$(jq -n \
  --argjson ls   "$OVERALL_LS_JSON" \
  --argjson moto "$OVERALL_MOTO_JSON" \
  '{speedup_vs_localstack:$ls, speedup_vs_moto:$moto}')

# ─────────────────────────────────────────────────────────────────────────────
# Build skipped array
# ─────────────────────────────────────────────────────────────────────────────

SKIPPED_JSON=$(jq -c '[.results[] | select(.operation == "SKIPPED") | {service:.service, reason:.skip_reason}]' "$REPORT")

# Build global seed_failures array (all SEED_FAILED entries across all services)
SEED_FAILURES_JSON=$(jq -c '[.results[] | select(.operation == "SEED_FAILED") | {service:.service, target:.target, reason:.seed_reason}]' "$REPORT")

# ─────────────────────────────────────────────────────────────────────────────
# Build failures summary JSON
# ─────────────────────────────────────────────────────────────────────────────

_arr_to_json() {
  local -n _ref=$1
  if [[ ${#_ref[@]} -gt 0 ]]; then
    printf '%s\n' "${_ref[@]}" | jq -R . | jq -s .
  else
    echo '[]'
  fi
}

LATENCY_FAILURES_JSON=$(_arr_to_json LATENCY_FAILURES)
ERROR_FAILURES_JSON=$(_arr_to_json ERROR_FAILURES)
MEMORY_FAILURES_JSON=$(_arr_to_json MEMORY_FAILURES)

TOTAL_FAILURES=$(( ${#LATENCY_FAILURES[@]} + ${#ERROR_FAILURES[@]} + ${#MEMORY_FAILURES[@]} ))

# ─────────────────────────────────────────────────────────────────────────────
# Assemble final gate JSON
# ─────────────────────────────────────────────────────────────────────────────

VERDICT="PASS"
$GATE_FAILED && VERDICT="FAIL"

GATE_JSON=$(jq -n \
  --arg verdict "$VERDICT" \
  --argjson p95_max "$P95_MAX" \
  --argjson mem_max "$MEMORY_MAX" \
  --argjson ignored "$IGNORED_JSON" \
  --argjson ignored_lat "$IGNORED_LATENCY_JSON" \
  --argjson op_p95 "$OP_P95_MAX_JSON" \
  --arg profile "$PROFILE" \
  --arg mode "$MODE" \
  --arg timestamp "$TIMESTAMP" \
  --argjson targets "$TARGETS_JSON" \
  --argjson requests "$REQ_COUNT" \
  --argjson concurrency "$CONCURRENCY" \
  --argjson os_idle "$OS_IDLE_MB" \
  --argjson os_loaded "$OS_LOADED_MB" \
  --argjson ls_idle "$LS_IDLE_MB" \
  --argjson ls_loaded "$LS_LOADED_MB" \
  --argjson moto_idle "$MOTO_IDLE_MB" \
  --argjson moto_loaded "$MOTO_LOADED_MB" \
  --argjson mem_pass "$([ "$MEMORY_GATE_PASS" == "true" ] && echo true || echo false)" \
  --argjson services "$SERVICES_MERGED" \
  --argjson overall "$OVERALL_JSON" \
  --argjson skipped "$SKIPPED_JSON" \
  --argjson seed_fail "$SEED_FAILURES_JSON" \
  --argjson lat_fail "$LATENCY_FAILURES_JSON" \
  --argjson err_fail "$ERROR_FAILURES_JSON" \
  --argjson mem_fail "$MEMORY_FAILURES_JSON" \
  --argjson total_fail "$TOTAL_FAILURES" \
  '{
    verdict: $verdict,
    thresholds: {
      p95_max_ms: $p95_max,
      memory_max_mb: $mem_max,
      ignored_errors: $ignored,
      ignored_latency: $ignored_lat,
      per_op_p95_thresholds: $op_p95
    },
    metadata: {
      profile: $profile,
      mode: $mode,
      timestamp: $timestamp,
      targets: $targets,
      requests: $requests,
      concurrency: $concurrency
    },
    memory: {
      openstack:  {idle_mb: $os_idle,   loaded_mb: $os_loaded},
      localstack: {idle_mb: $ls_idle,   loaded_mb: $ls_loaded},
      moto:       {idle_mb: $moto_idle, loaded_mb: $moto_loaded},
      gate_pass:  $mem_pass
    },
    services: $services,
    overall: $overall,
    skipped: $skipped,
    seed_failures: $seed_fail,
    failures: {
      latency: $lat_fail,
      errors:  $err_fail,
      memory:  $mem_fail,
      total:   $total_fail
    }
  }')

# ─────────────────────────────────────────────────────────────────────────────
# Output
# ─────────────────────────────────────────────────────────────────────────────

if [[ -n "$OUTPUT" ]]; then
  echo "$GATE_JSON" > "$OUTPUT"
  echo "[gate] Gate JSON written to: $OUTPUT" >&2
else
  echo "$GATE_JSON"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Exit code
# ─────────────────────────────────────────────────────────────────────────────

if $GATE_FAILED; then
  echo "[gate] FAIL — ${TOTAL_FAILURES} gate check(s) did not pass" >&2
  exit 1
else
  echo "[gate] PASS — all gate checks passed" >&2
  exit 0
fi
