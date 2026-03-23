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
# Helper: format a per-operation speedup ratio as a human-readable multiplier.
# $1 = ratio (float or "null")
#   For latency: ratio = competitor / openstack  (>1 = competitor slower = openstack faster)
#   For RPS:     ratio = openstack / competitor   (>1 = openstack faster)
# Outputs: "(-4.5x)" if competitor is slower, "(+5.0x)" if competitor is faster (value inverted).
# Suppressed when ratio is within ±5% of 1.0 (i.e. 0.95–1.05).
# ─────────────────────────────────────────────────────────────────────────────
fmt_ratio() {
  local v="$1"
  if [[ "$v" == "null" ]] || ! awk "BEGIN {exit !($v > 0)}" 2>/dev/null; then
    echo ""
    return
  fi
  awk -v r="$v" 'BEGIN {
    if      (r >= 1.05) printf "(-%.1fx)", r
    else if (r <= 0.95) printf "(+%.1fx)", 1/r
  }'
}

# ─────────────────────────────────────────────────────────────────────────────
# Helper: format a service speedup summary as up to two lines (faster / slower).
# $1 = service name (key in .services)
# $2 = competitor key: "localstack" or "moto"  (matches speedup_vs_<key> fields)
# Reads per-op p50 ratios from the gate JSON to split into faster/slower buckets.
# Outputs 0, 1 or 2 lines of the form:
#   "2.1×–8.3× faster on N ops (avg 4.7×)"
#   "1.3×–5.0× slower on M ops (avg 2.1×)"
# ─────────────────────────────────────────────────────────────────────────────
fmt_summary_line() {
  local svc="$1" comp="$2"
  local mul en_dash
  mul=$(printf '\xc3\x97')      # ×
  en_dash=$(printf '\xe2\x80\x93')  # –

  # Collect all per-op p50 ratios for this service/competitor
  local ratios
  if [[ "$svc" == "*" ]]; then
    # Overall: collect across all services
    ratios=$(jq -r \
      ".services | to_entries[] | .value.operations | to_entries[] | .value.speedup_vs_${comp}.p50 // empty | select(. > 0)" \
      "$GATE_JSON" 2>/dev/null)
  else
    ratios=$(jq -r \
      ".services.\"$svc\".operations | to_entries[] | .value.speedup_vs_${comp}.p50 // empty | select(. > 0)" \
      "$GATE_JSON" 2>/dev/null)
  fi

  if [[ -z "$ratios" ]]; then echo ""; return; fi

  # Split into two awk passes: ratios > 1 (openstack faster) and < 1 (openstack slower)
  local faster_line slower_line
  faster_line=$(echo "$ratios" | awk -v mul="$mul" -v dash="$en_dash" '
    BEGIN { mn=9999; mx=0; sum=0; n=0 }
    $1+0 > 1 { v=$1+0; if(v<mn) mn=v; if(v>mx) mx=v; sum+=v; n++ }
    END {
      if (n==0) exit
      avg=sum/n
      printf "%.1f%s%s%.1f%s faster on %d op%s (avg **%.1f%s**)\n", mn, mul, dash, mx, mul, n, (n==1?"":"s"), avg, mul
    }')

  slower_line=$(echo "$ratios" | awk -v mul="$mul" -v dash="$en_dash" '
    BEGIN { mn=9999; mx=0; sum=0; n=0 }
    $1+0 < 1 { v=1/($1+0); if(v<mn) mn=v; if(v>mx) mx=v; sum+=v; n++ }
    END {
      if (n==0) exit
      avg=sum/n
      printf "%.1f%s%s%.1f%s slower on %d op%s (avg **%.1f%s**)\n", mn, mul, dash, mx, mul, n, (n==1?"":"s"), avg, mul
    }')

  # Emit whichever lines are non-empty
  [[ -n "$faster_line" ]] && echo "$faster_line"
  [[ -n "$slower_line" ]] && echo "$slower_line"
}

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
# Helper: render a benchmark table for a given set of operations.
# $1 = service name (key in gate JSON)
# $2 = newline-separated list of operation names to render
# Outputs table rows (no header — caller emits header).
# ─────────────────────────────────────────────────────────────────────────────
render_ops_table_rows() {
  local svc="$1" ops="$2"

  # Pre-compute which targets have a seed failure for this service.
  # A seed failure means missing data for that target is expected and should be
  # omitted silently (the warning blockquote covers it).  If a target is active
  # (HAS_LS / HAS_MOTO) but has NO seed failure, any operation that is missing
  # data for that target gets an explicit "—" row so the gap is visible.
  local LS_SEED_FAILED=false MOTO_SEED_FAILED=false
  if $HAS_LS; then
    jq -e ".services.\"$svc\".seed_failures // [] | map(select(.target == \"localstack\")) | length > 0" \
      "$GATE_JSON" >/dev/null 2>&1 && LS_SEED_FAILED=true
  fi
  if $HAS_MOTO; then
    jq -e ".services.\"$svc\".seed_failures // [] | map(select(.target == \"moto\")) | length > 0" \
      "$GATE_JSON" >/dev/null 2>&1 && MOTO_SEED_FAILED=true
  fi

  while IFS= read -r OP; do
    OP_DATA=$(jq -c ".services.\"$svc\".operations.\"$OP\"" "$GATE_JSON")
    GATE_PASS=$(echo "$OP_DATA" | jq -r '.gate_pass')
    ERROR_IGNORED=$(echo "$OP_DATA" | jq -r '.error_ignored')

    OS_P50=$(echo "$OP_DATA" | jq -r '.openstack.p50_ms')
    OS_P95=$(echo "$OP_DATA" | jq -r '.openstack.p95_ms')
    OS_P99=$(echo "$OP_DATA" | jq -r '.openstack.p99_ms')
    OS_RPS=$(echo "$OP_DATA" | jq -r '.openstack.rps')
    OS_ERR=$(echo "$OP_DATA" | jq -r '.openstack.errors')

    STATUS_ICON=""
    [[ "$GATE_PASS" == "false" ]] && STATUS_ICON=" ❌"

    ERR_DISPLAY="$OS_ERR"
    [[ "$ERROR_IGNORED" == "true" && "$OS_ERR" != "0" ]] && ERR_DISPLAY="${OS_ERR} *(ignored)*"

    echo "| **${OP}**${STATUS_ICON} | **openstack** | **${OS_P50}** | **${OS_P95}** | **${OS_P99}** | **${OS_RPS}** | **${ERR_DISPLAY}** |"

    if $HAS_LS; then
      LS_P50=$(echo "$OP_DATA" | jq -r '.localstack.p50_ms // "—"')
      LS_P95=$(echo "$OP_DATA" | jq -r '.localstack.p95_ms // "—"')
      LS_P99=$(echo "$OP_DATA" | jq -r '.localstack.p99_ms // "—"')
      LS_RPS=$(echo "$OP_DATA" | jq -r '.localstack.rps // "—"')
      LS_ERR=$(echo "$OP_DATA" | jq -r '.localstack.errors // "—"')
      if [[ "$LS_P50" != "null" && "$LS_P50" != "—" ]]; then
        SU_LS=$(echo "$OP_DATA" | jq -c '.speedup_vs_localstack // {}')
        SU_LS_P50=$(fmt_ratio "$(echo "$SU_LS" | jq -r '.p50 // "null"')")
        SU_LS_P95=$(fmt_ratio "$(echo "$SU_LS" | jq -r '.p95 // "null"')")
        SU_LS_P99=$(fmt_ratio "$(echo "$SU_LS" | jq -r '.p99 // "null"')")
        SU_LS_RPS=$(fmt_ratio "$(echo "$SU_LS" | jq -r '.rps // "null"')")
        [[ -n "$SU_LS_P50" ]] && LS_P50="${LS_P50} ${SU_LS_P50}"
        [[ -n "$SU_LS_P95" ]] && LS_P95="${LS_P95} ${SU_LS_P95}"
        [[ -n "$SU_LS_P99" ]] && LS_P99="${LS_P99} ${SU_LS_P99}"
        [[ -n "$SU_LS_RPS" ]] && LS_RPS="${LS_RPS} ${SU_LS_RPS}"
        echo "| | LocalStack | ${LS_P50} | ${LS_P95} | ${LS_P99} | ${LS_RPS} | ${LS_ERR} |"
      elif ! $LS_SEED_FAILED; then
        # LocalStack is active, no data, and no seed failure explains it — show explicit gap
        echo "| | LocalStack | — | — | — | — | — |"
      fi
    fi

    if $HAS_MOTO; then
      MOTO_P50=$(echo "$OP_DATA" | jq -r '.moto.p50_ms // "—"')
      MOTO_P95=$(echo "$OP_DATA" | jq -r '.moto.p95_ms // "—"')
      MOTO_P99=$(echo "$OP_DATA" | jq -r '.moto.p99_ms // "—"')
      MOTO_RPS=$(echo "$OP_DATA" | jq -r '.moto.rps // "—"')
      MOTO_ERR=$(echo "$OP_DATA" | jq -r '.moto.errors // "—"')
      if [[ "$MOTO_P50" != "null" && "$MOTO_P50" != "—" ]]; then
        SU_MOTO=$(echo "$OP_DATA" | jq -c '.speedup_vs_moto // {}')
        SU_MOTO_P50=$(fmt_ratio "$(echo "$SU_MOTO" | jq -r '.p50 // "null"')")
        SU_MOTO_P95=$(fmt_ratio "$(echo "$SU_MOTO" | jq -r '.p95 // "null"')")
        SU_MOTO_P99=$(fmt_ratio "$(echo "$SU_MOTO" | jq -r '.p99 // "null"')")
        SU_MOTO_RPS=$(fmt_ratio "$(echo "$SU_MOTO" | jq -r '.rps // "null"')")
        [[ -n "$SU_MOTO_P50" ]] && MOTO_P50="${MOTO_P50} ${SU_MOTO_P50}"
        [[ -n "$SU_MOTO_P95" ]] && MOTO_P95="${MOTO_P95} ${SU_MOTO_P95}"
        [[ -n "$SU_MOTO_P99" ]] && MOTO_P99="${MOTO_P99} ${SU_MOTO_P99}"
        [[ -n "$SU_MOTO_RPS" ]] && MOTO_RPS="${MOTO_RPS} ${SU_MOTO_RPS}"
        echo "| | moto | ${MOTO_P50} | ${MOTO_P95} | ${MOTO_P99} | ${MOTO_RPS} | ${MOTO_ERR} |"
      elif ! $MOTO_SEED_FAILED; then
        # moto is active, no data, and no seed failure explains it — show explicit gap
        echo "| | moto | — | — | — | — | — |"
      fi
    fi
  done <<< "$ops"
}

# ─────────────────────────────────────────────────────────────────────────────
# Helper: emit a summary blockquote for a named sub-section of a service.
# $1 = service name   $2 = competitor key (localstack|moto)   $3 = ops (newline-sep)
# Re-computes faster/slower split from only the provided operation set.
# ─────────────────────────────────────────────────────────────────────────────
fmt_subset_summary() {
  local svc="$1" comp="$2" ops="$3"
  local mul en_dash
  mul=$(printf '\xc3\x97')
  en_dash=$(printf '\xe2\x80\x93')

  # Collect p50 ratios only for ops in the subset
  local ratios=""
  while IFS= read -r op; do
    local r
    r=$(jq -r ".services.\"$svc\".operations.\"$op\".speedup_vs_${comp}.p50 // empty" "$GATE_JSON" 2>/dev/null || true)
    [[ -n "$r" && "$r" != "null" ]] && ratios+="$r"$'\n'
  done <<< "$ops"

  [[ -z "$ratios" ]] && return

  echo "$ratios" | awk -v mul="$mul" -v dash="$en_dash" '
    BEGIN { fmn=9999; fmx=0; fsum=0; fn=0; smn=9999; smx=0; ssum=0; sn=0 }
    NF>0 {
      v=$1+0
      if (v > 1) { if(v<fmn) fmn=v; if(v>fmx) fmx=v; fsum+=v; fn++ }
      if (v > 0 && v < 1) { iv=1/v; if(iv<smn) smn=iv; if(iv>smx) smx=iv; ssum+=iv; sn++ }
    }
    END {
      if (fn>0) printf "%.1f%s%s%.1f%s faster on %d op%s (avg **%.1f%s**)\n", fmn,mul,dash,fmx,mul,fn,(fn==1?"":"s"),fsum/fn,mul
      if (sn>0) printf "%.1f%s%s%.1f%s slower on %d op%s (avg **%.1f%s**)\n", smn,mul,dash,smx,mul,sn,(sn==1?"":"s"),ssum/sn,mul
    }'
}

# ─────────────────────────────────────────────────────────────────────────────
# Per-service sections
# ─────────────────────────────────────────────────────────────────────────────

SERVICES=$(jq -r '.services | keys[]' "$GATE_JSON")

while IFS= read -r SVC; do

  # ── Special handling: S3 split into per-filesize sub-tables ────────────────
  if [[ "$SVC" == "s3" ]]; then
    echo ""
    echo "### S3"

    # S3 seed failure warnings (service-level, not per-tier)
    SVC_SEED_FAILURES=$(jq -r ".services.\"$SVC\".seed_failures // [] | .[] | \"\(.target): \(.reason)\"" "$GATE_JSON" 2>/dev/null || true)
    if [[ -n "$SVC_SEED_FAILURES" ]]; then
      echo ""
      while IFS= read -r sf; do
        target_name="${sf%%:*}"
        reason="${sf#*: }"
        [[ "$target_name" == "localstack" ]] && target_name="LocalStack"
        echo "> ⚠️ **${target_name}:** seed failed (${reason}) — excluded from this service's benchmarks"
      done <<< "$SVC_SEED_FAILURES"
    fi

    ALL_S3_OPS=$(jq -r ".services.\"s3\".operations | keys[]" "$GATE_JSON")

    for tier in 1mb 10mb 50mb 100mb; do
      # Filter ops for this tier (suffix match)
      TIER_OPS=$(echo "$ALL_S3_OPS" | grep "_${tier}$" || true)
      [[ -z "$TIER_OPS" ]] && continue

      # Display label: 1mb→1MB, 10mb→10MB etc.
      TIER_LABEL=$(echo "$tier" | tr '[:lower:]' '[:upper:]')
      echo ""
      echo "#### S3 — ${TIER_LABEL}"
      echo ""
      echo "| Operation | Platform | p50 (ms) | p95 (ms) | p99 (ms) | RPS | Errors |"
      echo "|-----------|----------|----------|----------|----------|-----|--------|"
      render_ops_table_rows "s3" "$TIER_OPS"

      # Per-tier summary
      TIER_SPEEDUP_LINES=()
      while IFS= read -r line; do
        [[ -n "$line" ]] && TIER_SPEEDUP_LINES+=("**openstack vs LocalStack:** ${line}")
      done < <($HAS_LS && fmt_subset_summary "s3" "localstack" "$TIER_OPS" || true)
      while IFS= read -r line; do
        [[ -n "$line" ]] && TIER_SPEEDUP_LINES+=("**openstack vs moto:** ${line}")
      done < <($HAS_MOTO && fmt_subset_summary "s3" "moto" "$TIER_OPS" || true)
      if [[ ${#TIER_SPEEDUP_LINES[@]} -gt 0 ]]; then
        echo ""
        for line in "${TIER_SPEEDUP_LINES[@]}"; do echo "> ${line}"; done
      fi
    done
    continue
  fi

  # ── Generic service rendering ───────────────────────────────────────────────
  echo ""
  echo "### $(echo "$SVC" | tr '[:lower:]' '[:upper:]')"
  echo ""

  echo "| Operation | Platform | p50 (ms) | p95 (ms) | p99 (ms) | RPS | Errors |"
  echo "|-----------|----------|----------|----------|----------|-----|--------|"

  OPERATIONS=$(jq -r ".services.\"$SVC\".operations | keys[]" "$GATE_JSON")
  render_ops_table_rows "$SVC" "$OPERATIONS"

  # Seed failure warnings — one blockquote per failed target for this service
  SVC_SEED_FAILURES=$(jq -r ".services.\"$SVC\".seed_failures // [] | .[] | \"\(.target): \(.reason)\"" "$GATE_JSON" 2>/dev/null || true)
  if [[ -n "$SVC_SEED_FAILURES" ]]; then
    echo ""
    while IFS= read -r sf; do
      # Capitalise target name for display (localstack → LocalStack)
      target_name="${sf%%:*}"
      reason="${sf#*: }"
      [[ "$target_name" == "localstack" ]] && target_name="LocalStack"
      [[ "$target_name" == "moto" ]]       && target_name="moto"
      [[ "$target_name" == "openstack" ]]  && target_name="openstack"
      echo "> ⚠️ **${target_name}:** seed failed (${reason}) — excluded from this service's benchmarks"
    done <<< "$SVC_SEED_FAILURES"
  fi

  # Per-service speedup summary (p50 latency split into faster/slower buckets)
  SPEEDUP_LINES=()
  while IFS= read -r line; do
    [[ -n "$line" ]] && SPEEDUP_LINES+=("**openstack vs LocalStack:** ${line}")
  done < <($HAS_LS && fmt_summary_line "$SVC" "localstack" || true)
  while IFS= read -r line; do
    [[ -n "$line" ]] && SPEEDUP_LINES+=("**openstack vs moto:** ${line}")
  done < <($HAS_MOTO && fmt_summary_line "$SVC" "moto" || true)

  if [[ ${#SPEEDUP_LINES[@]} -gt 0 ]]; then
    echo ""
    for line in "${SPEEDUP_LINES[@]}"; do
      echo "> ${line}"
    done
  fi
done <<< "$SERVICES"

# ─────────────────────────────────────────────────────────────────────────────
# Overall performance summary
# ─────────────────────────────────────────────────────────────────────────────

if $HAS_LS || $HAS_MOTO; then
  OVERALL_LS_LINES=()
  OVERALL_MOTO_LINES=()
  while IFS= read -r line; do [[ -n "$line" ]] && OVERALL_LS_LINES+=("$line"); done \
    < <($HAS_LS && fmt_summary_line "*" "localstack" || true)
  while IFS= read -r line; do [[ -n "$line" ]] && OVERALL_MOTO_LINES+=("$line"); done \
    < <($HAS_MOTO && fmt_summary_line "*" "moto" || true)

  if [[ ${#OVERALL_LS_LINES[@]} -gt 0 || ${#OVERALL_MOTO_LINES[@]} -gt 0 ]]; then
    echo ""
    echo "### Overall Performance"
    echo ""
    for line in "${OVERALL_LS_LINES[@]}";   do echo "> **openstack vs LocalStack:** ${line}"; done
    for line in "${OVERALL_MOTO_LINES[@]}"; do echo "> **openstack vs moto:** ${line}"; done
  fi
fi

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
