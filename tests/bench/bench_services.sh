#!/usr/bin/env bash
# bench_services.sh
#
# Comprehensive shell-based benchmark script for openstack.
# Benchmarks all 24 supported AWS services against both openstack and LocalStack,
# producing a structured JSON report with raw per-operation metrics.
#
# Prerequisites:
#   - oha (preferred) or hey (fallback) HTTP benchmarking tool
#   - docker
#   - jq
#   - curl
#
# Usage:
#   ./bench_services.sh --profile default --output report.json
#   ./bench_services.sh --profile stress --output report.json
#   ./bench_services.sh --binary --profile default --output report.json
#   ./bench_services.sh --services s3,dynamodb --requests 500 --concurrency 4

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# 1.1 Argument parsing
# ─────────────────────────────────────────────────────────────────────────────

PROFILE="default"
SERVICES_FILTER=""
BINARY_MODE=false
OUTPUT=""
REQUESTS=""
CONCURRENCY=""
OPENSTACK_IMAGE="${PARITY_OPENSTACK_IMAGE:-ghcr.io/jessekoldewijn/openstack:latest}"
LOCALSTACK_IMAGE="${PARITY_LOCALSTACK_IMAGE:-localstack/localstack:3.7.2}"
MOTO_IMAGE="${PARITY_MOTO_IMAGE:-motoserver/moto:latest}"
CPU_LIMIT="${PARITY_DOCKER_CPU_LIMIT:-2}"
MEMORY_LIMIT="${PARITY_DOCKER_MEMORY_LIMIT:-4g}"
BINARY_PATH="${OPENSTACK_BINARY:-}"
TARGETS="os,ls,moto"
OS_PORT=14566
LS_PORT=14567
MOTO_PORT=5555

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --profile <default|stress>        Benchmark profile (default: default)
  --services <s3,dynamodb,...>       Comma-separated service filter (overrides profile)
  --binary                          Run openstack as bare binary (not in Docker)
  --output <path>                   Write JSON report to path (default: stdout)
  --requests <n>                    Override request count per operation
  --concurrency <n>                 Override concurrency level
  --openstack-image <image>         Docker image for openstack (default: env or ghcr.io/jessekoldewijn/openstack:latest)
  --localstack-image <image>        Docker image for LocalStack (default: env or localstack/localstack:3.7.2)
  --moto-image <image>              Docker image for moto (default: env or motoserver/moto:latest)
  --targets <os,ls,moto>            Comma-separated targets to benchmark (default: os,ls,moto; os is required)
  --binary-path <path>              Path to openstack binary (for --binary mode)
  -h, --help                        Show this help
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile) PROFILE="$2"; shift 2 ;;
    --services) SERVICES_FILTER="$2"; shift 2 ;;
    --binary) BINARY_MODE=true; shift ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --requests) REQUESTS="$2"; shift 2 ;;
    --concurrency) CONCURRENCY="$2"; shift 2 ;;
    --openstack-image) OPENSTACK_IMAGE="$2"; shift 2 ;;
    --localstack-image) LOCALSTACK_IMAGE="$2"; shift 2 ;;
    --moto-image) MOTO_IMAGE="$2"; shift 2 ;;
    --targets) TARGETS="$2"; shift 2 ;;
    --binary-path) BINARY_PATH="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "Unknown option: $1"; usage ;;
  esac
done

# Validate --targets: os must always be present
if [[ "$TARGETS" != *os* ]]; then
  echo "ERROR: 'os' must be included in --targets (got: $TARGETS)" >&2
  exit 1
fi

# Helper to check if a target is active
target_active() {
  [[ ",$TARGETS," == *",$1,"* ]]
}

# ─────────────────────────────────────────────────────────────────────────────
# 1.5 Helper functions
# ─────────────────────────────────────────────────────────────────────────────

log() {
  echo "[bench] $*" >&2
}

log_section() {
  echo "" >&2
  echo "══════════════════════════════════════════════════════════════" >&2
  echo "  $*" >&2
  echo "══════════════════════════════════════════════════════════════" >&2
}

get_docker_mem_kb() {
  local container_id="$1"
  # Use cgroup v2: memory.current minus page-cache (inactive_file + active_file)
  # to measure true process RSS without OS file-cache inflation.
  local mem_bytes cache_bytes
  mem_bytes=$(docker exec "$container_id" cat /sys/fs/cgroup/memory.current 2>/dev/null || echo "")
  if [[ -n "$mem_bytes" && "$mem_bytes" =~ ^[0-9]+$ ]]; then
    # Try to subtract file cache recorded in memory.stat
    cache_bytes=$(docker exec "$container_id" \
      awk '/^inactive_file /{c+=$2} /^active_file /{c+=$2} END{print c+0}' \
      /sys/fs/cgroup/memory.stat 2>/dev/null || echo "0")
    echo $(( (mem_bytes - cache_bytes) / 1024 ))
    return
  fi
  # Fallback: docker stats (includes page cache — less accurate)
  docker stats --no-stream --format '{{.MemUsage}}' "$container_id" 2>/dev/null \
    | awk -F'/' '{gsub(/[^0-9.]/, "", $1); if ($1 ~ /GiB/) printf "%.0f", $1*1048576; else if ($1 ~ /MiB/) printf "%.0f", $1*1024; else printf "%.0f", $1}' 2>/dev/null \
    || echo "0"
}

get_process_mem_kb() {
  local pid="$1"
  if [[ "$(uname)" == "Darwin" ]]; then
    ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo "0"
  else
    awk '/^VmRSS:/{print $2}' "/proc/$pid/status" 2>/dev/null || echo "0"
  fi
}

REPORT_FILE=""
init_report() {
  REPORT_FILE=$(mktemp /tmp/bench_report.XXXXXX.json)
  local mode="docker"
  $BINARY_MODE && mode="binary"
  jq -n \
    --arg profile "$PROFILE" \
    --arg mode "$mode" \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg requests "$REQ_COUNT" \
    --arg concurrency "$CONC" \
    --arg os_image "$OPENSTACK_IMAGE" \
    --arg ls_image "$LOCALSTACK_IMAGE" \
    --arg moto_image "$MOTO_IMAGE" \
    --arg targets "$TARGETS" \
    --arg cpu "$CPU_LIMIT" \
    --arg mem "$MEMORY_LIMIT" \
    '{
      profile: $profile,
      mode: $mode,
      timestamp: $ts,
      targets: ($targets | split(",")),
      config: {
        requests: ($requests | tonumber),
        concurrency: ($concurrency | tonumber),
        openstack_image: $os_image,
        localstack_image: $ls_image,
        moto_image: $moto_image,
        cpu_limit: $cpu,
        memory_limit: $mem
      },
      memory: {
        openstack: { idle_mb: null, loaded_mb: null },
        localstack: { idle_mb: null, loaded_mb: null },
        moto: { idle_mb: null, loaded_mb: null }
      },
      results: []
    }' > "$REPORT_FILE"
}

update_results() {
  local tmp
  tmp=$(mktemp)
  jq "$@" "$REPORT_FILE" > "$tmp" && mv "$tmp" "$REPORT_FILE"
}

# ─────────────────────────────────────────────────────────────────────────────
# 1.2 HTTP bench tool detection
# ─────────────────────────────────────────────────────────────────────────────

BENCH_TOOL=""
detect_bench_tool() {
  if command -v oha &>/dev/null; then
    BENCH_TOOL="oha"
    log "Using oha for HTTP benchmarking"
  elif command -v hey &>/dev/null; then
    BENCH_TOOL="hey"
    log "Using hey for HTTP benchmarking (fallback)"
  else
    echo "ERROR: Neither 'oha' nor 'hey' found in PATH." >&2
    echo "" >&2
    echo "Install one of:" >&2
    echo "  oha (recommended): cargo install oha" >&2
    echo "                     brew install oha" >&2
    echo "  hey (fallback):    go install github.com/rakyll/hey@latest" >&2
    echo "                     brew install hey" >&2
    exit 1
  fi
}

# Check for required tools
for tool in docker jq curl; do
  if ! command -v "$tool" &>/dev/null; then
    echo "ERROR: Required tool '$tool' not found in PATH." >&2
    exit 1
  fi
done

detect_bench_tool

# ─────────────────────────────────────────────────────────────────────────────
# 1.3 bench() helper — wraps oha/hey, extracts metrics, appends to report
# ─────────────────────────────────────────────────────────────────────────────

# bench <service> <operation> <target: os|ls> <method> <url> [extra_args...]
# extra_args are passed directly to oha/hey (e.g., -H "Content-Type: ..." -d '...')
bench() {
  local service="$1" operation="$2" target="$3" method="$4" url="$5"
  shift 5
  local extra_args=("$@")

  local p50=0 p95=0 p99=0 throughput=0 errors=0 total="$REQ_COUNT"
  local raw_output
  raw_output=$(mktemp)

  if [[ "$BENCH_TOOL" == "oha" ]]; then
    # oha --output-format json gives structured output
    oha -n "$REQ_COUNT" -c "$CONC" -m "$method" --output-format json --no-tui \
      "${extra_args[@]}" \
      "$url" > "$raw_output" 2>/dev/null || true

    p50=$(jq -r '.latencyPercentiles.p50 // 0' "$raw_output" 2>/dev/null | awk '{printf "%.2f", $1*1000}')
    p95=$(jq -r '.latencyPercentiles.p95 // 0' "$raw_output" 2>/dev/null | awk '{printf "%.2f", $1*1000}')
    p99=$(jq -r '.latencyPercentiles.p99 // 0' "$raw_output" 2>/dev/null | awk '{printf "%.2f", $1*1000}')
    throughput=$(jq -r '.summary.requestsPerSec // 0' "$raw_output" 2>/dev/null | awk '{printf "%.1f", $1}')
    local status_codes
    status_codes=$(jq -r '.statusCodeDistribution // {}' "$raw_output" 2>/dev/null)
    # Count non-2xx responses as errors
    errors=$(echo "$status_codes" | jq -r 'to_entries | map(select(.key | test("^[2]") | not)) | map(.value) | add // 0' 2>/dev/null || echo "0")
  else
    # hey output needs text parsing
    hey -n "$REQ_COUNT" -c "$CONC" -m "$method" \
      "${extra_args[@]}" \
      "$url" > "$raw_output" 2>/dev/null || true

    p50=$(awk '/50% in/{print $3*1000}' "$raw_output" 2>/dev/null || echo "0")
    p95=$(awk '/95% in/{print $3*1000}' "$raw_output" 2>/dev/null || echo "0")
    p99=$(awk '/99% in/{print $3*1000}' "$raw_output" 2>/dev/null || echo "0")
    throughput=$(awk '/Requests\/sec:/{print $2}' "$raw_output" 2>/dev/null || echo "0")
    errors=$(awk '/\[5[0-9][0-9]\]/{sum+=$2} /\[4[0-9][0-9]\]/{sum+=$2} END{print sum+0}' "$raw_output" 2>/dev/null || echo "0")
  fi

  rm -f "$raw_output"

  # Append to report
  local target_field="openstack"
  [[ "$target" == "ls" ]] && target_field="localstack"
  [[ "$target" == "moto" ]] && target_field="moto"

  update_results \
    --arg svc "$service" \
    --arg op "$operation" \
    --arg tf "$target_field" \
    --argjson p50 "${p50:-0}" \
    --argjson p95 "${p95:-0}" \
    --argjson p99 "${p99:-0}" \
    --argjson tp "${throughput:-0}" \
    --argjson errs "${errors:-0}" \
    --argjson total "$total" \
    '
    # Find or create the result entry for this service+operation
    (.results |= (
      if any(.[]; .service == $svc and .operation == $op) then
        map(if .service == $svc and .operation == $op then
          . + {($tf): {p50_ms: $p50, p95_ms: $p95, p99_ms: $p99, throughput_rps: $tp, errors: $errs, total: $total}}
        else . end)
      else
        . + [{service: $svc, operation: $op, ($tf): {p50_ms: $p50, p95_ms: $p95, p99_ms: $p99, throughput_rps: $tp, errors: $errs, total: $total}}]
      end
    ))
    '
}

# bench_dynamic <service> <operation> <target> <method> <url> <body_template> [extra_header_args...]
# Like bench() but sends REQ_COUNT requests with a unique body per request.
# body_template must contain the literal "{i}" which is replaced with the
# 1-based iteration index, e.g.:
#   "Action=CreateUser&UserName=user-{i}&Version=2010-05-08"
# Requests are dispatched in batches of $CONC concurrent curl processes.
# Produces the same p50/p95/p99/throughput/errors metrics as bench().
bench_dynamic() {
  local service="$1" operation="$2" target="$3" method="$4" url="$5" body_template="$6"
  shift 6
  local extra_args=("$@")   # extra -H flags, etc.

  local tmpdir
  tmpdir=$(mktemp -d)

  local start_ts end_ts wall_secs throughput
  start_ts=$(date +%s%3N)   # ms since epoch

  local i
  for (( i=1; i<=REQ_COUNT; i++ )); do
    local body="${body_template//\{i\}/$i}"
    # Write result file: "time_total_ms http_code"
    (
      local raw
      raw=$(curl -s -w "\n%{http_code}" -X "$method" "${extra_args[@]}" -d "$body" "$url" 2>/dev/null) || true
      local code="${raw##*$'\n'}"
      local time_ms
      # Capture time_total via a second curl just for timing — no, instead
      # use curl -o /dev/null -w '%{time_total}' which avoids body parsing.
      time_ms=$(curl -s -o /dev/null -w '%{time_total}' -X "$method" \
        "${extra_args[@]}" -d "$body" "$url" 2>/dev/null) || time_ms=0
      # Convert fractional seconds to ms
      time_ms=$(awk "BEGIN{printf \"%.2f\", ${time_ms:-0}*1000}")
      printf '%s %s\n' "$time_ms" "${code:-000}" > "$tmpdir/$i"
    ) &

    # Wait for batch to complete every $CONC jobs
    if (( i % CONC == 0 )); then
      wait
    fi
  done
  wait   # final stragglers

  end_ts=$(date +%s%3N)
  wall_secs=$(awk "BEGIN{printf \"%.3f\", ($end_ts - $start_ts)/1000}")

  # Collect results
  local times=() errors=0
  for (( i=1; i<=REQ_COUNT; i++ )); do
    if [[ -f "$tmpdir/$i" ]]; then
      local t c
      read -r t c < "$tmpdir/$i"
      times+=("$t")
      if [[ ! "$c" =~ ^2 ]]; then
        (( errors++ )) || true
      fi
    fi
  done
  rm -rf "$tmpdir"

  local total="${#times[@]}"

  # Compute percentiles: sort numerically, pick by index
  local p50=0 p95=0 p99=0
  if [[ $total -gt 0 ]]; then
    local sorted_times
    sorted_times=$(printf '%s\n' "${times[@]}" | sort -n)
    p50=$(echo "$sorted_times" | awk -v n="$total" 'NR==int(n*0.50+0.5){print; exit}')
    p95=$(echo "$sorted_times" | awk -v n="$total" 'NR==int(n*0.95+0.5){print; exit}')
    p99=$(echo "$sorted_times" | awk -v n="$total" 'NR==int(n*0.99+0.5){print; exit}')
    p50=$(awk "BEGIN{printf \"%.2f\", ${p50:-0}}")
    p95=$(awk "BEGIN{printf \"%.2f\", ${p95:-0}}")
    p99=$(awk "BEGIN{printf \"%.2f\", ${p99:-0}}")
  fi

  throughput=$(awk "BEGIN{printf \"%.1f\", ($total)/(${wall_secs:-1})}")

  # Append to report (same schema as bench())
  local target_field="openstack"
  [[ "$target" == "ls" ]]   && target_field="localstack"
  [[ "$target" == "moto" ]] && target_field="moto"

  update_results \
    --arg svc "$service" \
    --arg op "$operation" \
    --arg tf "$target_field" \
    --argjson p50 "${p50:-0}" \
    --argjson p95 "${p95:-0}" \
    --argjson p99 "${p99:-0}" \
    --argjson tp "${throughput:-0}" \
    --argjson errs "${errors:-0}" \
    --argjson total "$total" \
    '
    (.results |= (
      if any(.[]; .service == $svc and .operation == $op) then
        map(if .service == $svc and .operation == $op then
          . + {($tf): {p50_ms: $p50, p95_ms: $p95, p99_ms: $p99, throughput_rps: $tp, errors: $errs, total: $total}}
        else . end)
      else
        . + [{service: $svc, operation: $op, ($tf): {p50_ms: $p50, p95_ms: $p95, p99_ms: $p99, throughput_rps: $tp, errors: $errs, total: $total}}]
      end
    ))
    '
}

# bench_targets <service> <operation> <method> <os_url> <ls_url> <moto_url> [extra_args...]
# Respects SEED_OS/SEED_LS/SEED_MOTO flags: skips targets whose seed failed.
# Defaults to 1 (enabled) so services without seed_all_targets still work.
bench_targets() {
  local service="$1" operation="$2" method="$3" os_url="$4" ls_url="$5" moto_url="$6"
  shift 6
  local extra_args=("$@")

  if [[ "${SEED_OS:-1}" -eq 1 ]]; then
    log "  $service/$operation (openstack)..."
    bench "$service" "$operation" "os" "$method" "$os_url" "${extra_args[@]}"
  fi
  if target_active ls && [[ "${SEED_LS:-1}" -eq 1 ]]; then
    log "  $service/$operation (localstack)..."
    bench "$service" "$operation" "ls" "$method" "$ls_url" "${extra_args[@]}"
  fi
  if target_active moto && [[ "${SEED_MOTO:-1}" -eq 1 ]]; then
    log "  $service/$operation (moto)..."
    bench "$service" "$operation" "moto" "$method" "$moto_url" "${extra_args[@]}" "${MOTO_EXTRA[@]}"
  fi
}

# bench_dynamic_targets <service> <operation> <method> <os_url> <ls_url> <moto_url> <body_template> [extra_header_args...]
# Like bench_targets but uses bench_dynamic (unique body per request via {i} substitution).
# Respects SEED_OS/SEED_LS/SEED_MOTO flags.
bench_dynamic_targets() {
  local service="$1" operation="$2" method="$3" os_url="$4" ls_url="$5" moto_url="$6" body_template="$7"
  shift 7
  local extra_args=("$@")

  if [[ "${SEED_OS:-1}" -eq 1 ]]; then
    log "  $service/$operation (openstack)..."
    bench_dynamic "$service" "$operation" "os" "$method" "$os_url" "$body_template" "${extra_args[@]}"
  fi
  if target_active ls && [[ "${SEED_LS:-1}" -eq 1 ]]; then
    log "  $service/$operation (localstack)..."
    bench_dynamic "$service" "$operation" "ls" "$method" "$ls_url" "$body_template" "${extra_args[@]}"
  fi
  if target_active moto && [[ "${SEED_MOTO:-1}" -eq 1 ]]; then
    log "  $service/$operation (moto)..."
    bench_dynamic "$service" "$operation" "moto" "$method" "$moto_url" "$body_template" "${extra_args[@]}" "${MOTO_EXTRA[@]}"
  fi
}

# Record a skip entry for a service
skip_service() {
  local service="$1" reason="$2"
  log "  SKIP: $service — $reason"
  update_results \
    --arg svc "$service" \
    --arg reason "$reason" \
    '.results += [{service: $svc, operation: "SKIPPED", skip_reason: $reason}]'
}

# Record a seed failure for a specific target within a service.
# Creates a SEED_FAILED result entry so downstream tools can surface warnings.
# $1 = service   $2 = target (os|ls|moto)   $3 = reason (human-readable)
record_seed_failure() {
  local service="$1" target="$2" reason="$3"
  local target_field="openstack"
  [[ "$target" == "ls"   ]] && target_field="localstack"
  [[ "$target" == "moto" ]] && target_field="moto"
  log "  SEED_FAIL: $service/$target_field — $reason"
  update_results \
    --arg svc "$service" \
    --arg tf "$target_field" \
    --arg reason "$reason" \
    '.results += [{service: $svc, operation: "SEED_FAILED", target: $tf, seed_reason: $reason}]'
}

# Per-target seed state flags (reset to 1 so bench_targets is unaffected when
# a service section uses the old-style seed or has no seed at all).
SEED_OS=1; SEED_LS=1; SEED_MOTO=1

# Optional moto-only extra curl/oha args (e.g. -H "Host: s3.amazonaws.com").
# Set per service section; bench_targets/seed_all_targets append these to moto
# calls only.  Reset to empty after each service block that uses it.
MOTO_EXTRA=()

# seed_request <target: os|ls|moto> <method> <url> [extra_args...]
# Returns 0 on success (2xx), 1 on failure.  Logs diagnostic info on failure.
seed_request() {
  local target="$1" method="$2" url="$3"
  shift 3
  local response http_code body
  response=$(curl -s -w "\n%{http_code}" -X "$method" "$@" "$url" 2>/dev/null) || true
  http_code="${response##*$'\n'}"
  body="${response%$'\n'*}"

  if [[ "$http_code" =~ ^2 ]]; then
    return 0
  fi

  log "  WARN: seed failed for target=$target (HTTP ${http_code:-000})"
  if [[ -n "$body" ]]; then
    log "  Response: ${body:0:300}"
  fi
  return 1
}

# seed_all_targets <service> <method> <os_url> <ls_url> <moto_url> [extra_args...]
# Seeds each active target independently.  Sets SEED_OS/SEED_LS/SEED_MOTO to
# 1 (succeeded) or 0 (failed/inactive).  Returns 0 if openstack seed succeeded.
# Records SEED_FAILED entries in the report for any competitor whose seed fails.
seed_all_targets() {
  local service="$1" method="$2" os_url="$3" ls_url="$4" moto_url="$5"
  shift 5
  SEED_OS=0; SEED_LS=0; SEED_MOTO=0

  seed_request "os" "$method" "$os_url" "$@" && SEED_OS=1 || true

  if target_active ls; then
    seed_request "ls" "$method" "$ls_url" "$@" && SEED_LS=1 \
      || record_seed_failure "$service" "ls" "seed request failed"
  fi

  if target_active moto; then
    seed_request "moto" "$method" "$moto_url" "$@" "${MOTO_EXTRA[@]}" && SEED_MOTO=1 \
      || record_seed_failure "$service" "moto" "seed request failed"
  fi

  # Service proceeds as long as openstack seed succeeded
  [[ $SEED_OS -eq 1 ]]
}

# ─────────────────────────────────────────────────────────────────────────────
# 1.4 Profile resolution
# ─────────────────────────────────────────────────────────────────────────────

CORE_SERVICES="dynamodb,firehose,iam,kinesis,s3,secretsmanager,sns,sts"

resolve_profile() {
  case "$PROFILE" in
    default)
      PROFILE_SERVICES="$CORE_SERVICES"
      PROFILE_REQUESTS=100
      PROFILE_CONCURRENCY=6
      ;;
    stress)
      PROFILE_SERVICES="$CORE_SERVICES"
      PROFILE_REQUESTS=1000
      PROFILE_CONCURRENCY=20
      ;;
    *)
      echo "ERROR: Unknown profile '$PROFILE'. Use default or stress." >&2
      exit 1
      ;;
  esac

  # Apply overrides
  [[ -n "$SERVICES_FILTER" ]] && PROFILE_SERVICES="$SERVICES_FILTER"
  REQ_COUNT="${REQUESTS:-$PROFILE_REQUESTS}"
  CONC="${CONCURRENCY:-$PROFILE_CONCURRENCY}"
}

resolve_profile

# Convert comma-separated services to array
IFS=',' read -ra ACTIVE_SERVICES <<< "$PROFILE_SERVICES"

is_active() {
  local svc="$1"
  for s in "${ACTIVE_SERVICES[@]}"; do
    [[ "$s" == "$svc" ]] && return 0
  done
  return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# Print configuration
# ─────────────────────────────────────────────────────────────────────────────

log_section "Benchmark Configuration"
log "Profile:     $PROFILE"
log "Mode:        $(if $BINARY_MODE; then echo binary; else echo docker; fi)"
log "Services:    ${ACTIVE_SERVICES[*]}"
log "Requests:    $REQ_COUNT"
log "Concurrency: $CONC"
log "Bench tool:  $BENCH_TOOL"

OS_BASE="http://127.0.0.1:$OS_PORT"
LS_BASE="http://127.0.0.1:$LS_PORT"
MOTO_BASE="http://127.0.0.1:$MOTO_PORT"

# ─────────────────────────────────────────────────────────────────────────────
# 2.5 Cleanup trap
# ─────────────────────────────────────────────────────────────────────────────

OS_CONTAINER=""
LS_CONTAINER=""
MOTO_CONTAINER=""
OS_PID=""

cleanup() {
  log "Cleaning up..."
  if [[ -n "$OS_CONTAINER" ]]; then docker rm -f "$OS_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$LS_CONTAINER" ]]; then docker rm -f "$LS_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$MOTO_CONTAINER" ]]; then docker rm -f "$MOTO_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$OS_PID" ]]; then kill "$OS_PID" &>/dev/null || true; fi
  if [[ -n "$OS_PID" ]]; then wait "$OS_PID" &>/dev/null || true; fi
}
trap cleanup EXIT

# ─────────────────────────────────────────────────────────────────────────────
# Wait for health endpoint
# ─────────────────────────────────────────────────────────────────────────────

wait_healthy() {
  local url="$1" label="$2" timeout_s="${3:-60}"
  local start=$SECONDS
  log "Waiting for $label to be healthy ($url)..."
  while (( SECONDS - start < timeout_s )); do
    if curl -sf "$url" > /dev/null 2>&1; then
      log "$label is healthy ($(( SECONDS - start ))s)"
      return 0
    fi
    sleep 0.5
  done
  log "ERROR: $label did not become healthy within ${timeout_s}s"
  return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# 2.1 Docker mode startup
# ─────────────────────────────────────────────────────────────────────────────

start_docker_mode() {
  log_section "Starting targets (Docker mode)"

  log "Starting openstack container..."
  OS_CONTAINER=$(docker run -d \
    --name "bench-openstack-$$" \
    --cpus="$CPU_LIMIT" \
    --memory="$MEMORY_LIMIT" \
    -p "$OS_PORT:4566" \
    -e GATEWAY_LISTEN="0.0.0.0:4566" \
    -e PERSISTENCE=0 \
    -e LS_LOG=error \
    "$OPENSTACK_IMAGE")

  if target_active ls; then
    log "Starting LocalStack container..."
    LS_CONTAINER=$(docker run -d \
      --name "bench-localstack-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      -p "$LS_PORT:4566" \
      -e PERSISTENCE=0 \
      "$LOCALSTACK_IMAGE")
  fi

  if target_active moto; then
    log "Starting moto container..."
    MOTO_CONTAINER=$(docker run -d \
      --name "bench-moto-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      -p "$MOTO_PORT:5000" \
      "$MOTO_IMAGE")
  fi

  wait_healthy "$OS_BASE/_localstack/health" "openstack" 60
  if target_active ls; then
    wait_healthy "$LS_BASE/_localstack/health" "LocalStack" 120
  fi
  if target_active moto; then
    wait_healthy "$MOTO_BASE/moto-api/" "moto" 30
  fi
}

# ─────────────────────────────────────────────────────────────────────────────
# 2.2 Binary mode startup
# ─────────────────────────────────────────────────────────────────────────────

start_binary_mode() {
  log_section "Starting targets (Binary mode)"

  # Resolve binary path
  if [[ -z "$BINARY_PATH" ]]; then
    local repo_root
    repo_root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || echo ".")
    BINARY_PATH="$repo_root/target/release/openstack"
  fi

  if [[ ! -x "$BINARY_PATH" ]]; then
    echo "ERROR: openstack binary not found at $BINARY_PATH" >&2
    echo "Build it first: cargo build --release --bin openstack" >&2
    exit 1
  fi

  # Use a writable temp directory for the data dir so the binary can be run
  # without root privileges (the default /var/lib/localstack requires root).
  local os_data_dir
  os_data_dir=$(mktemp -d -t openstack-bench-XXXXXX)
  log "Starting openstack binary ($BINARY_PATH) with data dir $os_data_dir..."
  GATEWAY_LISTEN="127.0.0.1:$OS_PORT" \
  LOCALSTACK_DATA_DIR="$os_data_dir" \
  PERSISTENCE=0 \
  LS_LOG=error \
    "$BINARY_PATH" &
  OS_PID=$!

  # Clean up the data dir on exit
  # shellcheck disable=SC2064
  trap "rm -rf '$os_data_dir'" EXIT

  if target_active ls; then
    log "Starting LocalStack container..."
    LS_CONTAINER=$(docker run -d \
      --name "bench-localstack-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      -p "$LS_PORT:4566" \
      -e PERSISTENCE=0 \
      "$LOCALSTACK_IMAGE")
  fi

  if target_active moto; then
    log "Starting moto container..."
    MOTO_CONTAINER=$(docker run -d \
      --name "bench-moto-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      -p "$MOTO_PORT:5000" \
      "$MOTO_IMAGE")
  fi

  wait_healthy "$OS_BASE/_localstack/health" "openstack" 30
  if target_active ls; then
    wait_healthy "$LS_BASE/_localstack/health" "LocalStack" 120
  fi
  if target_active moto; then
    wait_healthy "$MOTO_BASE/moto-api/" "moto" 30
  fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Start targets
# ─────────────────────────────────────────────────────────────────────────────

if $BINARY_MODE; then
  start_binary_mode
else
  start_docker_mode
fi

# ─────────────────────────────────────────────────────────────────────────────
# Initialize report
# ─────────────────────────────────────────────────────────────────────────────

init_report

# ─────────────────────────────────────────────────────────────────────────────
# 2.3 Idle memory snapshot
# ─────────────────────────────────────────────────────────────────────────────

log_section "Collecting idle memory snapshots"
sleep 2  # Let processes settle

if $BINARY_MODE; then
  os_idle_kb=$(get_process_mem_kb "$OS_PID")
else
  os_idle_kb=$(get_docker_mem_kb "$OS_CONTAINER")
fi
ls_idle_kb=0
if target_active ls; then
  ls_idle_kb=$(get_docker_mem_kb "$LS_CONTAINER")
fi
moto_idle_kb=0
if target_active moto; then
  moto_idle_kb=$(get_docker_mem_kb "$MOTO_CONTAINER")
fi

os_idle_mb=$(echo "$os_idle_kb" | awk '{printf "%.1f", $1/1024}')
ls_idle_mb=$(echo "$ls_idle_kb" | awk '{printf "%.1f", $1/1024}')
moto_idle_mb=$(echo "$moto_idle_kb" | awk '{printf "%.1f", $1/1024}')

log "openstack idle RSS: ${os_idle_mb} MB"
if target_active ls; then log "LocalStack idle RSS: ${ls_idle_mb} MB"; fi
if target_active moto; then log "moto idle RSS: ${moto_idle_mb} MB"; fi

update_results \
  --argjson os_idle "$os_idle_mb" \
  --argjson ls_idle "$ls_idle_mb" \
  --argjson moto_idle "$moto_idle_mb" \
  '.memory.openstack.idle_mb = $os_idle | .memory.localstack.idle_mb = $ls_idle | .memory.moto.idle_mb = $moto_idle'

# ─────────────────────────────────────────────────────────────────────────────
# Service benchmark sections
# ─────────────────────────────────────────────────────────────────────────────

# Common variables for endpoint URLs

# ─────────────────────────────────────────────────────────────────────────────
# 3.1 S3 (REST-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "s3"; then
  log_section "S3 (REST-XML)"

  # Moto's multi-service standalone server needs a Host header to route
  # path-style S3 URLs (/bucket/key) to its S3 backend.  LocalStack and
  # openstack both infer S3 from path style without it; moto does not.
  # Additionally, moto 5.x requires an Authorization header for S3 GET/HEAD
  # on objects (returns 403 without it).  The signature is not validated, so
  # a static dummy value works fine.
  MOTO_EXTRA=(-H "Host: s3.amazonaws.com" -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=dummy")

  # Pre-flight: verify enough disk for temp payload files (~1.5 GB total).
  # GitHub ubuntu-latest runners have ~14 GB free; abort early rather than
  # silently producing partial results on constrained environments.
  S3_TMPDIR=$(mktemp -d)
  _s3_avail_kb=$(df -k "$S3_TMPDIR" 2>/dev/null | awk 'NR==2{print $4}')
  _s3_avail_gb=$(( ${_s3_avail_kb:-0} / 1024 / 1024 ))
  if [[ $_s3_avail_gb -lt 2 ]]; then
    log "  WARN: Only ${_s3_avail_gb} GB available at $S3_TMPDIR; need ≥2 GB for S3 temp files"
    skip_service "s3" "Insufficient disk space (${_s3_avail_gb} GB available, 2 GB required)"
    rm -rf "$S3_TMPDIR"
    MOTO_EXTRA=()
  else

    # Generate payload files once; dd from /dev/urandom fills with random bytes.
    # Sizes cover realistic single-part S3 object categories:
    #   1MB  — small objects (configs, thumbnails, JSON)
    #   10MB — medium objects (PDFs, packages, images)
    #   50MB — large objects (archives, datasets)
    #   100MB — max realistic single-part upload (videos, model weights)
    log "  Generating S3 payload files in $S3_TMPDIR (${_s3_avail_gb} GB available)..."
    dd if=/dev/urandom of="$S3_TMPDIR/1mb.bin"   bs=1M count=1   status=none
    dd if=/dev/urandom of="$S3_TMPDIR/10mb.bin"  bs=1M count=10  status=none
    dd if=/dev/urandom of="$S3_TMPDIR/50mb.bin"  bs=1M count=50  status=none
    dd if=/dev/urandom of="$S3_TMPDIR/100mb.bin" bs=1M count=100 status=none

    # Seed: create bucket once (each target independently; only openstack is required)
    SEED_OS=1; SEED_LS=1; SEED_MOTO=1
    if seed_all_targets "s3" PUT \
         "$OS_BASE/bench-bucket-$$" \
         "$LS_BASE/bench-bucket-$$" \
         "$MOTO_BASE/bench-bucket-$$"; then

      # Benchmark each size tier independently.
      # Request counts and concurrency are capped per tier to stay within the
      # GitHub runner disk/memory budget (14 GB disk, 7 GB RAM, 4 GB per container).
      #
      # Tier  | default reqs | stress reqs | max concurrency
      # 1MB   |     100      |    1000     | profile default
      # 10MB  |      50      |     200     | profile default
      # 50MB  |      20      |      60     | min(profile, 4)
      # 100MB |      10      |      20     | min(profile, 2)

      for _s3_tier in 1mb 10mb 50mb 100mb; do
        _s3_file="$S3_TMPDIR/${_s3_tier}.bin"
        _s3_key="testobj-${_s3_tier}"

        # Save profile-level values; restore after each tier
        _s3_saved_req=$REQ_COUNT
        _s3_saved_conc=$CONC

        case "$_s3_tier" in
          1mb)
            REQ_COUNT=$( [[ "$PROFILE" == "stress" ]] && echo 1000 || echo 100 )
            # Concurrency unchanged — use profile default
            ;;
          10mb)
            REQ_COUNT=$( [[ "$PROFILE" == "stress" ]] && echo 200  || echo 50  )
            # Concurrency unchanged — use profile default
            ;;
          50mb)
            REQ_COUNT=$( [[ "$PROFILE" == "stress" ]] && echo 60   || echo 20  )
            CONC=$(( CONC < 4 ? CONC : 4 ))
            ;;
          100mb)
            REQ_COUNT=$( [[ "$PROFILE" == "stress" ]] && echo 20   || echo 10  )
            CONC=$(( CONC < 2 ? CONC : 2 ))
            ;;
        esac

        # Seed object for this tier on each active target
        SEED_OS=0; SEED_LS=0; SEED_MOTO=0
        seed_request "os" PUT "$OS_BASE/bench-bucket-$$/$_s3_key" \
          -H "Content-Type: application/octet-stream" --data-binary "@$_s3_file" \
          && SEED_OS=1 || true
        if target_active ls; then
          seed_request "ls" PUT "$LS_BASE/bench-bucket-$$/$_s3_key" \
            -H "Content-Type: application/octet-stream" --data-binary "@$_s3_file" \
            && SEED_LS=1 \
            || record_seed_failure "s3" "ls" "object upload failed for tier ${_s3_tier}"
        fi
        if target_active moto; then
          seed_request "moto" PUT "$MOTO_BASE/bench-bucket-$$/$_s3_key" \
            -H "Content-Type: application/octet-stream" --data-binary "@$_s3_file" \
            "${MOTO_EXTRA[@]}" && SEED_MOTO=1 \
            || record_seed_failure "s3" "moto" "object upload failed for tier ${_s3_tier}"
        fi

        # PutObject — body from file via -D (oha) or -d @file (hey)
        if [[ "$BENCH_TOOL" == "oha" ]]; then
          bench_targets "s3" "put_object_${_s3_tier}" PUT \
            "$OS_BASE/bench-bucket-$$/$_s3_key" \
            "$LS_BASE/bench-bucket-$$/$_s3_key" \
            "$MOTO_BASE/bench-bucket-$$/$_s3_key" \
            -H "Content-Type: application/octet-stream" \
            -D "$_s3_file"
        else
          bench_targets "s3" "put_object_${_s3_tier}" PUT \
            "$OS_BASE/bench-bucket-$$/$_s3_key" \
            "$LS_BASE/bench-bucket-$$/$_s3_key" \
            "$MOTO_BASE/bench-bucket-$$/$_s3_key" \
            -H "Content-Type: application/octet-stream" \
            -d "@$_s3_file"
        fi

        # GetObject
        bench_targets "s3" "get_object_${_s3_tier}" GET \
          "$OS_BASE/bench-bucket-$$/$_s3_key" \
          "$LS_BASE/bench-bucket-$$/$_s3_key" \
          "$MOTO_BASE/bench-bucket-$$/$_s3_key"

        # HeadObject
        bench_targets "s3" "head_object_${_s3_tier}" HEAD \
          "$OS_BASE/bench-bucket-$$/$_s3_key" \
          "$LS_BASE/bench-bucket-$$/$_s3_key" \
          "$MOTO_BASE/bench-bucket-$$/$_s3_key"

        # ListObjectsV2  (size-tagged for report parity with other ops)
        bench_targets "s3" "list_objects_v2_${_s3_tier}" GET \
          "$OS_BASE/bench-bucket-$$?list-type=2" \
          "$LS_BASE/bench-bucket-$$?list-type=2" \
          "$MOTO_BASE/bench-bucket-$$?list-type=2"

        # Restore profile-level values for next tier
        REQ_COUNT=$_s3_saved_req
        CONC=$_s3_saved_conc
      done

    else
      skip_service "s3" "Failed to create seed bucket"
    fi

    rm -rf "$S3_TMPDIR"
  fi  # disk pre-flight

  MOTO_EXTRA=()  # clear after S3 block
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.2 DynamoDB (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "dynamodb"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "DynamoDB (JSON)"

  DDB_HEADERS=(-H "Content-Type: application/x-amz-json-1.0")

  # Seed: create table (each target independently; only openstack is required)
  if seed_all_targets "dynamodb" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.0" \
       -H "X-Amz-Target: DynamoDB_20120810.CreateTable" \
       -d '{"TableName":"bench-table-'"$$"'","KeySchema":[{"AttributeName":"pk","KeyType":"HASH"}],"AttributeDefinitions":[{"AttributeName":"pk","AttributeType":"S"}],"BillingMode":"PAY_PER_REQUEST"}'; then

    sleep 1  # Wait for table to be active

    # PutItem
    bench_targets "dynamodb" "put_item" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${DDB_HEADERS[@]}" \
      -H "X-Amz-Target: DynamoDB_20120810.PutItem" \
      -d '{"TableName":"bench-table-'"$$"'","Item":{"pk":{"S":"key1"},"data":{"S":"benchmark-value"}}}'

    # GetItem
    bench_targets "dynamodb" "get_item" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${DDB_HEADERS[@]}" \
      -H "X-Amz-Target: DynamoDB_20120810.GetItem" \
      -d '{"TableName":"bench-table-'"$$"'","Key":{"pk":{"S":"key1"}}}'

    # Query
    bench_targets "dynamodb" "query" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${DDB_HEADERS[@]}" \
      -H "X-Amz-Target: DynamoDB_20120810.Query" \
      -d '{"TableName":"bench-table-'"$$"'","KeyConditionExpression":"pk = :v","ExpressionAttributeValues":{":v":{"S":"key1"}}}'

    # Scan
    bench_targets "dynamodb" "scan" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${DDB_HEADERS[@]}" \
      -H "X-Amz-Target: DynamoDB_20120810.Scan" \
      -d '{"TableName":"bench-table-'"$$"'"}'
  else
    skip_service "dynamodb" "Failed to create seed table"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.3 SNS (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "sns"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "SNS (Query-XML)"

  # Seed: create topic
  # Use curl -s (not -sf) so non-2xx responses don't silently kill the pipe.
  # Collapse newlines before grep so the ARN is found even if the XML is multiline.
  OS_TOPIC_ARN=$(curl -s -X POST "$OS_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
    | tr -d '\n' | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
  LS_TOPIC_ARN=""
  if target_active ls; then
    LS_TOPIC_ARN=$(curl -s -X POST "$LS_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
      | tr -d '\n' | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
    if [[ -z "$LS_TOPIC_ARN" ]]; then
      log "  WARN: SNS LocalStack seed returned no TopicArn — publish/get_topic_attributes will skip LocalStack"
      record_seed_failure "sns" "ls" "CreateTopic returned no TopicArn"
    fi
  fi
  MOTO_TOPIC_ARN=""
  if target_active moto; then
    MOTO_TOPIC_ARN=$(curl -s -X POST "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
      | tr -d '\n' | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
    if [[ -z "$MOTO_TOPIC_ARN" ]]; then
      log "  WARN: SNS moto seed returned no TopicArn — publish/get_topic_attributes will skip moto"
      record_seed_failure "sns" "moto" "CreateTopic returned no TopicArn"
    fi
  fi

  if [[ -n "$OS_TOPIC_ARN" ]]; then
    # Publish (target-specific ARNs require separate bench calls)
    log "  sns/publish (openstack)..."
    bench "sns" "publish" "os" POST "$OS_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=Publish&TopicArn=$OS_TOPIC_ARN&Message=benchmark-message"
    if target_active ls && [[ -n "$LS_TOPIC_ARN" ]]; then
      log "  sns/publish (localstack)..."
      bench "sns" "publish" "ls" POST "$LS_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=Publish&TopicArn=$LS_TOPIC_ARN&Message=benchmark-message"
    fi
    if target_active moto && [[ -n "$MOTO_TOPIC_ARN" ]]; then
      log "  sns/publish (moto)..."
      bench "sns" "publish" "moto" POST "$MOTO_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=Publish&TopicArn=$MOTO_TOPIC_ARN&Message=benchmark-message"
    fi

    # GetTopicAttributes
    log "  sns/get_topic_attributes (openstack)..."
    bench "sns" "get_topic_attributes" "os" POST "$OS_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=GetTopicAttributes&TopicArn=$OS_TOPIC_ARN"
    if target_active ls && [[ -n "$LS_TOPIC_ARN" ]]; then
      log "  sns/get_topic_attributes (localstack)..."
      bench "sns" "get_topic_attributes" "ls" POST "$LS_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=GetTopicAttributes&TopicArn=$LS_TOPIC_ARN"
    fi
    if target_active moto && [[ -n "$MOTO_TOPIC_ARN" ]]; then
      log "  sns/get_topic_attributes (moto)..."
      bench "sns" "get_topic_attributes" "moto" POST "$MOTO_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=GetTopicAttributes&TopicArn=$MOTO_TOPIC_ARN"
    fi

    # ListTopics
    bench_targets "sns" "list_topics" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ListTopics"
  else
    skip_service "sns" "Failed to create seed topic"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.4 IAM (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "iam"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "IAM (Query-XML)"

  # Seed: create user (each target independently; only openstack is required)
  if seed_all_targets "iam" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -d "Action=CreateUser&UserName=bench-user-$$&Version=2010-05-08"; then

    # CreateUser — unique username per iteration via {i}; 0 errors expected
    bench_dynamic_targets "iam" "create_user" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "Action=CreateUser&UserName=bench-create-user-$$-{i}&Version=2010-05-08" \
      -H "Content-Type: application/x-www-form-urlencoded"

    # GetUser — reads back the users created above using the same {i} index
    bench_dynamic_targets "iam" "get_user" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "Action=GetUser&UserName=bench-create-user-$$-{i}&Version=2010-05-08" \
      -H "Content-Type: application/x-www-form-urlencoded"

    # ListUsers — idempotent; oha is fine here
    bench_targets "iam" "list_users" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ListUsers&Version=2010-05-08"
  else
    skip_service "iam" "Failed to create seed user"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.5 STS (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "sts"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "STS (Query-XML)"

  # No seed needed for STS

  # GetCallerIdentity
  bench_targets "sts" "get_caller_identity" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=GetCallerIdentity&Version=2011-06-15"

  # AssumeRole
  bench_targets "sts" "assume_role" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=AssumeRole&RoleArn=arn:aws:iam::000000000000:role/bench-role&RoleSessionName=bench-session&Version=2011-06-15"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.6 Kinesis (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "kinesis"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Kinesis (JSON)"

  KINESIS_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: create stream (each target independently; only openstack is required)
  if seed_all_targets "kinesis" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: Kinesis_20131202.CreateStream" \
       -d '{"StreamName":"bench-stream-'"$$"'","ShardCount":1}'; then

    sleep 2  # Wait for stream to be active

    # PutRecord
    bench_targets "kinesis" "put_record" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${KINESIS_HEADERS[@]}" \
      -H "X-Amz-Target: Kinesis_20131202.PutRecord" \
      -d '{"StreamName":"bench-stream-'"$$"'","Data":"YmVuY2htYXJrLWRhdGE=","PartitionKey":"pk1"}'

    # DescribeStream
    bench_targets "kinesis" "describe_stream" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${KINESIS_HEADERS[@]}" \
      -H "X-Amz-Target: Kinesis_20131202.DescribeStream" \
      -d '{"StreamName":"bench-stream-'"$$"'"}'

    # ListStreams
    bench_targets "kinesis" "list_streams" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${KINESIS_HEADERS[@]}" \
      -H "X-Amz-Target: Kinesis_20131202.ListStreams" \
      -d '{}'
  else
    skip_service "kinesis" "Failed to create seed stream"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.7 Firehose (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "firehose"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Firehose (JSON)"

  FIREHOSE_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: create delivery stream (each target independently; only openstack is required)
  if seed_all_targets "firehose" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: Firehose_20150804.CreateDeliveryStream" \
       -d '{"DeliveryStreamName":"bench-firehose-'"$$"'","DeliveryStreamType":"DirectPut"}'; then

    sleep 1

    # PutRecord
    bench_targets "firehose" "put_record" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${FIREHOSE_HEADERS[@]}" \
      -H "X-Amz-Target: Firehose_20150804.PutRecord" \
      -d '{"DeliveryStreamName":"bench-firehose-'"$$"'","Record":{"Data":"YmVuY2htYXJrLWRhdGE="}}'

    # PutRecordBatch
    bench_targets "firehose" "put_record_batch" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${FIREHOSE_HEADERS[@]}" \
      -H "X-Amz-Target: Firehose_20150804.PutRecordBatch" \
      -d '{"DeliveryStreamName":"bench-firehose-'"$$"'","Records":[{"Data":"YmVuY2htYXJrLTE="},{"Data":"YmVuY2htYXJrLTI="}]}'

    # ListDeliveryStreams
    bench_targets "firehose" "list_delivery_streams" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${FIREHOSE_HEADERS[@]}" \
      -H "X-Amz-Target: Firehose_20150804.ListDeliveryStreams" \
      -d '{}'
  else
    skip_service "firehose" "Failed to create seed delivery stream"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.8 SecretsManager (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "secretsmanager"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "SecretsManager (JSON)"

  SM_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: create secret (each target independently; only openstack is required)
  # ClientRequestToken is required by LocalStack 3.x (and recommended by AWS);
  # openstack and moto accept requests without it but LocalStack returns HTTP 400.
  if seed_all_targets "secretsmanager" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: secretsmanager.CreateSecret" \
       -d '{"Name":"bench-secret-'"$$"'","SecretString":"benchmark-secret-value","ClientRequestToken":"bench-seed-'"$$"'"}'; then

    # CreateSecret — unique name and token per iteration via {i}; 0 errors expected
    bench_dynamic_targets "secretsmanager" "create_secret" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      '{"Name":"bench-secret-create-'"$$"'-{i}","SecretString":"new-secret-value","ClientRequestToken":"bench-create-'"$$"'-{i}"}' \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: secretsmanager.CreateSecret"

    # GetSecretValue
    bench_targets "secretsmanager" "get_secret_value" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SM_HEADERS[@]}" \
      -H "X-Amz-Target: secretsmanager.GetSecretValue" \
      -d '{"SecretId":"bench-secret-'"$$"'"}'

    # PutSecretValue
    bench_targets "secretsmanager" "put_secret_value" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SM_HEADERS[@]}" \
      -H "X-Amz-Target: secretsmanager.PutSecretValue" \
      -d '{"SecretId":"bench-secret-'"$$"'","SecretString":"updated-benchmark-value"}'

    # ListSecrets
    bench_targets "secretsmanager" "list_secrets" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SM_HEADERS[@]}" \
      -H "X-Amz-Target: secretsmanager.ListSecrets" \
      -d '{}'
  else
    skip_service "secretsmanager" "Failed to create seed secret"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 2.4 Post-load memory snapshot
# ─────────────────────────────────────────────────────────────────────────────

log_section "Collecting post-load memory snapshots"

if $BINARY_MODE; then
  os_loaded_kb=$(get_process_mem_kb "$OS_PID")
else
  os_loaded_kb=$(get_docker_mem_kb "$OS_CONTAINER")
fi
ls_loaded_kb=0
if target_active ls; then
  ls_loaded_kb=$(get_docker_mem_kb "$LS_CONTAINER")
fi
moto_loaded_kb=0
if target_active moto; then
  moto_loaded_kb=$(get_docker_mem_kb "$MOTO_CONTAINER")
fi

os_loaded_mb=$(echo "$os_loaded_kb" | awk '{printf "%.1f", $1/1024}')
ls_loaded_mb=$(echo "$ls_loaded_kb" | awk '{printf "%.1f", $1/1024}')
moto_loaded_mb=$(echo "$moto_loaded_kb" | awk '{printf "%.1f", $1/1024}')

log "openstack loaded RSS: ${os_loaded_mb} MB"
if target_active ls; then log "LocalStack loaded RSS: ${ls_loaded_mb} MB"; fi
if target_active moto; then log "moto loaded RSS: ${moto_loaded_mb} MB"; fi

update_results \
  --argjson os_loaded "$os_loaded_mb" \
  --argjson ls_loaded "$ls_loaded_mb" \
  --argjson moto_loaded "$moto_loaded_mb" \
  '.memory.openstack.loaded_mb = $os_loaded | .memory.localstack.loaded_mb = $ls_loaded | .memory.moto.loaded_mb = $moto_loaded'

# ─────────────────────────────────────────────────────────────────────────────
# 5.1-5.3 Final JSON assembly and output
# ─────────────────────────────────────────────────────────────────────────────

log_section "Benchmark Complete"

RESULT_COUNT=$(jq '.results | length' "$REPORT_FILE")
SKIP_COUNT=$(jq '[.results[] | select(.operation == "SKIPPED")] | length' "$REPORT_FILE")
log "Total result entries: $RESULT_COUNT (skipped: $SKIP_COUNT)"
log "openstack memory: idle=${os_idle_mb}MB loaded=${os_loaded_mb}MB"
if target_active ls; then log "LocalStack memory: idle=${ls_idle_mb}MB loaded=${ls_loaded_mb}MB"; fi
if target_active moto; then log "moto memory: idle=${moto_idle_mb}MB loaded=${moto_loaded_mb}MB"; fi

if [[ -n "$OUTPUT" ]]; then
  cp "$REPORT_FILE" "$OUTPUT"
  log "Report written to: $OUTPUT"
else
  cat "$REPORT_FILE"
fi

rm -f "$REPORT_FILE"
