#!/usr/bin/env bash
# render.sh
#
# Reads a benchmark-gate.json file and renders a markdown report to stdout.
# Used by the benchmark-report composite GitHub Action.
#
# Usage:
#   ./render.sh <path-to-gate.json>

set -euo pipefail

GATE_JSON="${1:-}"
if [[ -z "$GATE_JSON" || ! -f "$GATE_JSON" ]]; then
  echo "Usage: render.sh <gate-json-path>" >&2
  exit 1
fi

if ! command -v jq &>/dev/null; then
  echo "ERROR: jq is required" >&2
  exit 1
fi

# ─────────────────────────────────────────────────────────────────────────────
# Read top-level fields
# ─────────────────────────────────────────────────────────────────────────────

VERDICT=$(jq -r '.verdict' "$GATE_JSON")
PROFILE=$(jq -r '.metadata.profile' "$GATE_JSON")
MODE=$(jq -r '.metadata.mode' "$GATE_JSON")
TIMESTAMP=$(jq -r '.metadata.timestamp' "$GATE_JSON")
REQUESTS=$(jq -r '.metadata.requests' "$GATE_JSON")
CONCURRENCY=$(jq -r '.metadata.concurrency' "$GATE_JSON")
P95_MAX=$(jq -r '.thresholds.p95_max_ms' "$GATE_JSON")
MEM_MAX=$(jq -r '.thresholds.memory_max_mb' "$GATE_JSON")
TOTAL_FAILURES=$(jq -r '.failures.total' "$GATE_JSON")

HAS_LS=false
HAS_MOTO=false
jq -e '.metadata.targets | index("ls")' "$GATE_JSON" >/dev/null 2>&1 && HAS_LS=true
jq -e '.metadata.targets | index("moto")' "$GATE_JSON" >/dev/null 2>&1 && HAS_MOTO=true

# Badge style
if [[ "$VERDICT" == "PASS" ]]; then
  BADGE="✅ PASS"
else
  BADGE="❌ FAIL"
fi

# Targets display
TARGETS_STR="openstack"
$HAS_LS   && TARGETS_STR="$TARGETS_STR, LocalStack"
$HAS_MOTO && TARGETS_STR="$TARGETS_STR, moto"

# ─────────────────────────────────────────────────────────────────────────────
# Header
# ─────────────────────────────────────────────────────────────────────────────

cat <<EOF
<!-- benchmark-gate-report -->
## Benchmark Gate: ${BADGE}

**Profile:** \`${PROFILE}\` | **Mode:** \`${MODE}\` | **Requests:** ${REQUESTS} | **Concurrency:** ${CONCURRENCY}
**Timestamp:** ${TIMESTAMP} | **Targets:** ${TARGETS_STR}
**Thresholds:** openstack p95 ≤ ${P95_MAX}ms | openstack memory ≤ ${MEM_MAX}MB | errors = 0
EOF

# ─────────────────────────────────────────────────────────────────────────────
# Memory section
# ─────────────────────────────────────────────────────────────────────────────

OS_IDLE=$(jq -r '.memory.openstack.idle_mb' "$GATE_JSON")
OS_LOADED=$(jq -r '.memory.openstack.loaded_mb' "$GATE_JSON")
MEM_GATE_PASS=$(jq -r '.memory.gate_pass' "$GATE_JSON")

echo ""
echo "### Memory Footprint"
echo ""
echo "| Target | Idle RSS (MB) | Loaded RSS (MB) |"
echo "|--------|---------------|-----------------|"
echo "| **openstack** | **${OS_IDLE}** | **${OS_LOADED}** |"

MEM_RATIO=""
if $HAS_LS; then
  LS_IDLE=$(jq -r '.memory.localstack.idle_mb' "$GATE_JSON")
  LS_LOADED=$(jq -r '.memory.localstack.loaded_mb' "$GATE_JSON")
  echo "| LocalStack | ${LS_IDLE} | ${LS_LOADED} |"
  if awk "BEGIN {exit !($LS_LOADED > 0)}"; then
    MEM_RATIO=$(awk "BEGIN {printf \"%.0f\", $LS_LOADED / ($OS_LOADED == 0 ? 1 : $OS_LOADED)}")
  fi
fi

if $HAS_MOTO; then
  MOTO_IDLE=$(jq -r '.memory.moto.idle_mb' "$GATE_JSON")
  MOTO_LOADED=$(jq -r '.memory.moto.loaded_mb' "$GATE_JSON")
  echo "| moto | ${MOTO_IDLE} | ${MOTO_LOADED} |"
fi

# Context lines after the full table
if [[ -n "$MEM_RATIO" ]]; then
  echo ""
  echo "> openstack uses **${MEM_RATIO}x less memory** than LocalStack under load"
fi

if [[ "$MEM_GATE_PASS" == "false" ]]; then
  MEM_FAIL=$(jq -r '.failures.memory[0] // ""' "$GATE_JSON")
  echo ""
  echo "> ❌ **Memory gate failed:** ${MEM_FAIL}"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Per-service sections
# ─────────────────────────────────────────────────────────────────────────────

SERVICES=$(jq -r '.services | keys[]' "$GATE_JSON")

while IFS= read -r SVC; do
  echo ""
  echo "### $(echo "$SVC" | tr '[:lower:]' '[:upper:]')"
  echo ""

  # Table header — always: Operation | Platform | p50 | p95 | p99 | RPS | Errors
  echo "| Operation | Platform | p50 (ms) | p95 (ms) | p99 (ms) | RPS | Errors |"
  echo "|-----------|----------|----------|----------|----------|-----|--------|"

  OPERATIONS=$(jq -r ".services.\"$SVC\".operations | keys[]" "$GATE_JSON")

  while IFS= read -r OP; do
    OP_DATA=$(jq -c ".services.\"$SVC\".operations.\"$OP\"" "$GATE_JSON")
    GATE_PASS=$(echo "$OP_DATA" | jq -r '.gate_pass')
    ERROR_IGNORED=$(echo "$OP_DATA" | jq -r '.error_ignored')

    # openstack row (always present)
    OS_P50=$(echo "$OP_DATA" | jq -r '.openstack.p50_ms')
    OS_P95=$(echo "$OP_DATA" | jq -r '.openstack.p95_ms')
    OS_P99=$(echo "$OP_DATA" | jq -r '.openstack.p99_ms')
    OS_RPS=$(echo "$OP_DATA" | jq -r '.openstack.rps')
    OS_ERR=$(echo "$OP_DATA" | jq -r '.openstack.errors')

    # Status indicator for openstack row
    STATUS_ICON=""
    if [[ "$GATE_PASS" == "false" ]]; then
      STATUS_ICON=" ❌"
    fi

    # Format errors — show note if ignored
    ERR_DISPLAY="$OS_ERR"
    if [[ "$ERROR_IGNORED" == "true" && "$OS_ERR" != "0" ]]; then
      ERR_DISPLAY="${OS_ERR} *(ignored)*"
    fi

    echo "| **${OP}**${STATUS_ICON} | **openstack** | **${OS_P50}** | **${OS_P95}** | **${OS_P99}** | **${OS_RPS}** | **${ERR_DISPLAY}** |"

    # LocalStack row
    if $HAS_LS; then
      LS_P50=$(echo "$OP_DATA" | jq -r '.localstack.p50_ms // "—"')
      LS_P95=$(echo "$OP_DATA" | jq -r '.localstack.p95_ms // "—"')
      LS_P99=$(echo "$OP_DATA" | jq -r '.localstack.p99_ms // "—"')
      LS_RPS=$(echo "$OP_DATA" | jq -r '.localstack.rps // "—"')
      LS_ERR=$(echo "$OP_DATA" | jq -r '.localstack.errors // "—"')
      if [[ "$LS_P50" != "null" && "$LS_P50" != "—" ]]; then
        echo "| | LocalStack | ${LS_P50} | ${LS_P95} | ${LS_P99} | ${LS_RPS} | ${LS_ERR} |"
      fi
    fi

    # Moto row
    if $HAS_MOTO; then
      MOTO_P50=$(echo "$OP_DATA" | jq -r '.moto.p50_ms // "—"')
      MOTO_P95=$(echo "$OP_DATA" | jq -r '.moto.p95_ms // "—"')
      MOTO_P99=$(echo "$OP_DATA" | jq -r '.moto.p99_ms // "—"')
      MOTO_RPS=$(echo "$OP_DATA" | jq -r '.moto.rps // "—"')
      MOTO_ERR=$(echo "$OP_DATA" | jq -r '.moto.errors // "—"')
      if [[ "$MOTO_P50" != "null" && "$MOTO_P50" != "—" ]]; then
        echo "| | moto | ${MOTO_P50} | ${MOTO_P95} | ${MOTO_P99} | ${MOTO_RPS} | ${MOTO_ERR} |"
      fi
    fi
  done <<< "$OPERATIONS"

  # Speedup summary lines
  LS_SPEEDUP=$(jq -c ".services.\"$SVC\".speedup_vs_localstack" "$GATE_JSON")
  MOTO_SPEEDUP=$(jq -c ".services.\"$SVC\".speedup_vs_moto" "$GATE_JSON")

  SPEEDUP_LINES=()
  if [[ "$LS_SPEEDUP" != "null" ]]; then
    LS_MIN=$(echo "$LS_SPEEDUP" | jq -r '.min')
    LS_MAX=$(echo "$LS_SPEEDUP" | jq -r '.max')
    LS_AVG=$(echo "$LS_SPEEDUP" | jq -r '.avg')
    SPEEDUP_LINES+=("**vs LocalStack:** ${LS_MIN}x – ${LS_MAX}x faster (avg **${LS_AVG}x**) on p95")
  fi
  if [[ "$MOTO_SPEEDUP" != "null" ]]; then
    MOTO_MIN=$(echo "$MOTO_SPEEDUP" | jq -r '.min')
    MOTO_MAX=$(echo "$MOTO_SPEEDUP" | jq -r '.max')
    MOTO_AVG=$(echo "$MOTO_SPEEDUP" | jq -r '.avg')
    SPEEDUP_LINES+=("**vs moto:** ${MOTO_MIN}x – ${MOTO_MAX}x faster (avg **${MOTO_AVG}x**) on p95")
  fi

  if [[ ${#SPEEDUP_LINES[@]} -gt 0 ]]; then
    echo ""
    for line in "${SPEEDUP_LINES[@]}"; do
      echo "> ${line}"
    done
  fi
done <<< "$SERVICES"

# ─────────────────────────────────────────────────────────────────────────────
# Skipped services
# ─────────────────────────────────────────────────────────────────────────────

SKIP_COUNT=$(jq '.skipped | length' "$GATE_JSON")
if [[ "$SKIP_COUNT" -gt 0 ]]; then
  echo ""
  echo "### Skipped Services"
  echo ""
  jq -r '.skipped[] | "- **\(.service)**: \(.reason)"' "$GATE_JSON"
fi

# ─────────────────────────────────────────────────────────────────────────────
# Failures summary
# ─────────────────────────────────────────────────────────────────────────────

if [[ "$VERDICT" == "FAIL" ]]; then
  echo ""
  echo "### Failures"
  echo ""

  LAT_COUNT=$(jq '.failures.latency | length' "$GATE_JSON")
  if [[ "$LAT_COUNT" -gt 0 ]]; then
    echo "**Latency threshold exceeded (openstack p95 > ${P95_MAX}ms):**"
    jq -r '.failures.latency[] | "- \(.)"' "$GATE_JSON"
    echo ""
  fi

  ERR_COUNT=$(jq '.failures.errors | length' "$GATE_JSON")
  if [[ "$ERR_COUNT" -gt 0 ]]; then
    echo "**Non-zero openstack error rates:**"
    jq -r '.failures.errors[] | "- \(.)"' "$GATE_JSON"
    echo ""
  fi

  MEM_COUNT=$(jq '.failures.memory | length' "$GATE_JSON")
  if [[ "$MEM_COUNT" -gt 0 ]]; then
    echo "**Memory ceiling exceeded:**"
    jq -r '.failures.memory[] | "- \(.)"' "$GATE_JSON"
    echo ""
  fi

  echo "**Total failures: ${TOTAL_FAILURES}**"
fi
