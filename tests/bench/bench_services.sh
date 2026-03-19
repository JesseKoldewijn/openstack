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
#   ./bench_services.sh --profile smoke --output report.json
#   ./bench_services.sh --profile standard
#   ./bench_services.sh --profile deep --output report.json
#   ./bench_services.sh --binary --profile smoke --output report.json
#   ./bench_services.sh --services s3,dynamodb --requests 500 --concurrency 4

set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# 1.1 Argument parsing
# ─────────────────────────────────────────────────────────────────────────────

PROFILE="smoke"
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
  --profile <smoke|standard|deep>   Benchmark profile (default: smoke)
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
  # Use cgroup memory.current for accurate RSS
  docker exec "$container_id" cat /sys/fs/cgroup/memory.current 2>/dev/null \
    | awk '{printf "%.0f", $1/1024}' 2>/dev/null \
    || docker stats --no-stream --format '{{.MemUsage}}' "$container_id" 2>/dev/null \
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
    bench "$service" "$operation" "moto" "$method" "$moto_url" "${extra_args[@]}"
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
    bench_dynamic "$service" "$operation" "moto" "$method" "$moto_url" "$body_template" "${extra_args[@]}"
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

# Per-target seed state flags (reset to 1 so bench_targets is unaffected when
# a service section uses the old-style seed or has no seed at all).
SEED_OS=1; SEED_LS=1; SEED_MOTO=1

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

# seed_all_targets <method> <os_url> <ls_url> <moto_url> [extra_args...]
# Seeds each active target independently.  Sets SEED_OS/SEED_LS/SEED_MOTO to
# 1 (succeeded) or 0 (failed/inactive).  Returns 0 if openstack seed succeeded.
seed_all_targets() {
  local method="$1" os_url="$2" ls_url="$3" moto_url="$4"
  shift 4
  SEED_OS=0; SEED_LS=0; SEED_MOTO=0

  seed_request "os" "$method" "$os_url" "$@" && SEED_OS=1 || true

  if target_active ls; then
    seed_request "ls" "$method" "$ls_url" "$@" && SEED_LS=1 || true
  fi

  if target_active moto; then
    seed_request "moto" "$method" "$moto_url" "$@" && SEED_MOTO=1 || true
  fi

  # Service proceeds as long as openstack seed succeeded
  [[ $SEED_OS -eq 1 ]]
}

# ─────────────────────────────────────────────────────────────────────────────
# 1.4 Profile resolution
# ─────────────────────────────────────────────────────────────────────────────

CORE_SERVICES="dynamodb,firehose,iam,kinesis,s3,secretsmanager,sns,sts"
ALL_SERVICES="acm,apigateway,cloudformation,cloudwatch,dynamodb,ec2,ecr,events,firehose,iam,kinesis,kms,lambda,opensearch,redshift,route53,s3,secretsmanager,ses,sns,sqs,ssm,states,sts"

resolve_profile() {
  case "$PROFILE" in
    smoke)
      PROFILE_SERVICES="$CORE_SERVICES"
      PROFILE_REQUESTS=50
      PROFILE_CONCURRENCY=1
      ;;
    standard)
      PROFILE_SERVICES="$ALL_SERVICES"
      PROFILE_REQUESTS=200
      PROFILE_CONCURRENCY=2
      ;;
    deep)
      PROFILE_SERVICES="$ALL_SERVICES"
      PROFILE_REQUESTS=1000
      PROFILE_CONCURRENCY=4
      ;;
    *)
      echo "ERROR: Unknown profile '$PROFILE'. Use smoke, standard, or deep." >&2
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

  log "Starting openstack binary ($BINARY_PATH)..."
  GATEWAY_LISTEN="127.0.0.1:$OS_PORT" \
  PERSISTENCE=0 \
  LS_LOG=error \
    "$BINARY_PATH" &
  OS_PID=$!

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
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "S3 (REST-XML)"

  # Seed: create bucket (each target independently; only openstack is required)
  if seed_all_targets PUT \
       "$OS_BASE/bench-bucket-$$" \
       "$LS_BASE/bench-bucket-$$" \
       "$MOTO_BASE/bench-bucket-$$"; then

    # PutObject
    bench_targets "s3" "put_object" PUT \
      "$OS_BASE/bench-bucket-$$/testkey" \
      "$LS_BASE/bench-bucket-$$/testkey" \
      "$MOTO_BASE/bench-bucket-$$/testkey" \
      -H "Content-Type: application/octet-stream" \
      -d "benchmark-payload-data-1234567890"

    # GetObject
    bench_targets "s3" "get_object" GET \
      "$OS_BASE/bench-bucket-$$/testkey" \
      "$LS_BASE/bench-bucket-$$/testkey" \
      "$MOTO_BASE/bench-bucket-$$/testkey"

    # HeadObject
    bench_targets "s3" "head_object" HEAD \
      "$OS_BASE/bench-bucket-$$/testkey" \
      "$LS_BASE/bench-bucket-$$/testkey" \
      "$MOTO_BASE/bench-bucket-$$/testkey"

    # ListObjectsV2
    bench_targets "s3" "list_objects_v2" GET \
      "$OS_BASE/bench-bucket-$$?list-type=2" \
      "$LS_BASE/bench-bucket-$$?list-type=2" \
      "$MOTO_BASE/bench-bucket-$$?list-type=2"
  else
    skip_service "s3" "Failed to create seed bucket"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.2 DynamoDB (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "dynamodb"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "DynamoDB (JSON)"

  DDB_HEADERS=(-H "Content-Type: application/x-amz-json-1.0")

  # Seed: create table (each target independently; only openstack is required)
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
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
  OS_TOPIC_ARN=$(curl -sf -X POST "$OS_BASE" \
    -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
    | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
  LS_TOPIC_ARN=""
  if target_active ls; then
    LS_TOPIC_ARN=$(curl -sf -X POST "$LS_BASE" \
      -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
      | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
  fi
  MOTO_TOPIC_ARN=""
  if target_active moto; then
    MOTO_TOPIC_ARN=$(curl -sf -X POST "$MOTO_BASE" \
      -d "Action=CreateTopic&Name=bench-topic-$$" 2>/dev/null \
      | grep -oP '(?<=<TopicArn>)[^<]+' || echo "")
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
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
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
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
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
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
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
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: secretsmanager.CreateSecret" \
       -d '{"Name":"bench-secret-'"$$"'","SecretString":"benchmark-secret-value"}'; then

    # CreateSecret (will fail with duplicate but exercises write path)
    bench_targets "secretsmanager" "create_secret" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SM_HEADERS[@]}" \
      -H "X-Amz-Target: secretsmanager.CreateSecret" \
      -d '{"Name":"bench-secret-create-'"$$"'","SecretString":"new-secret-value"}'

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
# 4.1 SQS (Query-XML/JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "sqs"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "SQS (Query-XML)"

  # Seed: create queue
  OS_QUEUE_URL=$(curl -sf -X POST "$OS_BASE" \
    -d "Action=CreateQueue&QueueName=bench-queue-$$" 2>/dev/null \
    | grep -oP '(?<=<QueueUrl>)[^<]+' || echo "")
  LS_QUEUE_URL=""
  if target_active ls; then
    LS_QUEUE_URL=$(curl -sf -X POST "$LS_BASE" \
      -d "Action=CreateQueue&QueueName=bench-queue-$$" 2>/dev/null \
      | grep -oP '(?<=<QueueUrl>)[^<]+' || echo "")
  fi
  MOTO_QUEUE_URL=""
  if target_active moto; then
    MOTO_QUEUE_URL=$(curl -sf -X POST "$MOTO_BASE" \
      -d "Action=CreateQueue&QueueName=bench-queue-$$" 2>/dev/null \
      | grep -oP '(?<=<QueueUrl>)[^<]+' || echo "")
  fi

  if [[ -n "$OS_QUEUE_URL" ]]; then
    # SendMessage (target-specific queue URLs require separate bench calls)
    log "  sqs/send_message (openstack)..."
    bench "sqs" "send_message" "os" POST "$OS_QUEUE_URL" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=SendMessage&MessageBody=benchmark-message"
    if target_active ls && [[ -n "$LS_QUEUE_URL" ]]; then
      log "  sqs/send_message (localstack)..."
      bench "sqs" "send_message" "ls" POST "$LS_QUEUE_URL" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=SendMessage&MessageBody=benchmark-message"
    fi
    if target_active moto && [[ -n "$MOTO_QUEUE_URL" ]]; then
      log "  sqs/send_message (moto)..."
      bench "sqs" "send_message" "moto" POST "$MOTO_QUEUE_URL" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=SendMessage&MessageBody=benchmark-message"
    fi

    # ReceiveMessage
    log "  sqs/receive_message (openstack)..."
    bench "sqs" "receive_message" "os" POST "$OS_QUEUE_URL" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ReceiveMessage&MaxNumberOfMessages=1"
    if target_active ls && [[ -n "$LS_QUEUE_URL" ]]; then
      log "  sqs/receive_message (localstack)..."
      bench "sqs" "receive_message" "ls" POST "$LS_QUEUE_URL" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=ReceiveMessage&MaxNumberOfMessages=1"
    fi
    if target_active moto && [[ -n "$MOTO_QUEUE_URL" ]]; then
      log "  sqs/receive_message (moto)..."
      bench "sqs" "receive_message" "moto" POST "$MOTO_QUEUE_URL" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=ReceiveMessage&MaxNumberOfMessages=1"
    fi

    # ListQueues
    bench_targets "sqs" "list_queues" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ListQueues"
  else
    skip_service "sqs" "Failed to create seed queue"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.2 KMS (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "kms"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "KMS (JSON)"

  KMS_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: create key
  OS_KEY_ID=$(curl -sf -X POST "$OS_BASE" \
    -H "Content-Type: application/x-amz-json-1.1" \
    -H "X-Amz-Target: TrentService.CreateKey" \
    -d '{"Description":"bench-key"}' 2>/dev/null \
    | jq -r '.KeyMetadata.KeyId // empty' || echo "")
  LS_KEY_ID=""
  if target_active ls; then
    LS_KEY_ID=$(curl -sf -X POST "$LS_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: TrentService.CreateKey" \
      -d '{"Description":"bench-key"}' 2>/dev/null \
      | jq -r '.KeyMetadata.KeyId // empty' || echo "")
  fi
  MOTO_KEY_ID=""
  if target_active moto; then
    MOTO_KEY_ID=$(curl -sf -X POST "$MOTO_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: TrentService.CreateKey" \
      -d '{"Description":"bench-key"}' 2>/dev/null \
      | jq -r '.KeyMetadata.KeyId // empty' || echo "")
  fi

  if [[ -n "$OS_KEY_ID" ]]; then
    # Encrypt (target-specific key IDs require separate bench calls)
    log "  kms/encrypt (openstack)..."
    bench "kms" "encrypt" "os" POST "$OS_BASE" \
      "${KMS_HEADERS[@]}" \
      -H "X-Amz-Target: TrentService.Encrypt" \
      -d '{"KeyId":"'"$OS_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}'
    if target_active ls && [[ -n "$LS_KEY_ID" ]]; then
      log "  kms/encrypt (localstack)..."
      bench "kms" "encrypt" "ls" POST "$LS_BASE" \
        "${KMS_HEADERS[@]}" \
        -H "X-Amz-Target: TrentService.Encrypt" \
        -d '{"KeyId":"'"$LS_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}'
    fi
    if target_active moto && [[ -n "$MOTO_KEY_ID" ]]; then
      log "  kms/encrypt (moto)..."
      bench "kms" "encrypt" "moto" POST "$MOTO_BASE" \
        "${KMS_HEADERS[@]}" \
        -H "X-Amz-Target: TrentService.Encrypt" \
        -d '{"KeyId":"'"$MOTO_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}'
    fi

    # Decrypt — we need a ciphertext blob first; for benchmark purposes, test the endpoint
    # We'll encrypt once and use the result for decrypt bench
    OS_CIPHER=$(curl -sf -X POST "$OS_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: TrentService.Encrypt" \
      -d '{"KeyId":"'"$OS_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}' \
      | jq -r '.CiphertextBlob // empty' || echo "")
    LS_CIPHER=""
    if target_active ls && [[ -n "$LS_KEY_ID" ]]; then
      LS_CIPHER=$(curl -sf -X POST "$LS_BASE" \
        -H "Content-Type: application/x-amz-json-1.1" \
        -H "X-Amz-Target: TrentService.Encrypt" \
        -d '{"KeyId":"'"$LS_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}' \
        | jq -r '.CiphertextBlob // empty' || echo "")
    fi
    MOTO_CIPHER=""
    if target_active moto && [[ -n "$MOTO_KEY_ID" ]]; then
      MOTO_CIPHER=$(curl -sf -X POST "$MOTO_BASE" \
        -H "Content-Type: application/x-amz-json-1.1" \
        -H "X-Amz-Target: TrentService.Encrypt" \
        -d '{"KeyId":"'"$MOTO_KEY_ID"'","Plaintext":"YmVuY2htYXJr"}' \
        | jq -r '.CiphertextBlob // empty' || echo "")
    fi

    if [[ -n "$OS_CIPHER" ]]; then
      log "  kms/decrypt (openstack)..."
      bench "kms" "decrypt" "os" POST "$OS_BASE" \
        "${KMS_HEADERS[@]}" \
        -H "X-Amz-Target: TrentService.Decrypt" \
        -d '{"CiphertextBlob":"'"$OS_CIPHER"'"}'
    fi
    if target_active ls && [[ -n "$LS_CIPHER" ]]; then
      log "  kms/decrypt (localstack)..."
      bench "kms" "decrypt" "ls" POST "$LS_BASE" \
        "${KMS_HEADERS[@]}" \
        -H "X-Amz-Target: TrentService.Decrypt" \
        -d '{"CiphertextBlob":"'"$LS_CIPHER"'"}'
    fi
    if target_active moto && [[ -n "$MOTO_CIPHER" ]]; then
      log "  kms/decrypt (moto)..."
      bench "kms" "decrypt" "moto" POST "$MOTO_BASE" \
        "${KMS_HEADERS[@]}" \
        -H "X-Amz-Target: TrentService.Decrypt" \
        -d '{"CiphertextBlob":"'"$MOTO_CIPHER"'"}'
    fi

    # ListKeys
    bench_targets "kms" "list_keys" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${KMS_HEADERS[@]}" \
      -H "X-Amz-Target: TrentService.ListKeys" \
      -d '{}'
  else
    skip_service "kms" "Failed to create seed key"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.3 SSM (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ssm"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "SSM (JSON)"

  SSM_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: put parameter (each target independently; only openstack is required)
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: AmazonSSM.PutParameter" \
       -d '{"Name":"/bench/param-'"$$"'","Value":"benchmark-value","Type":"String"}'; then

    # PutParameter (overwrite)
    bench_targets "ssm" "put_parameter" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SSM_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonSSM.PutParameter" \
      -d '{"Name":"/bench/param-'"$$"'","Value":"updated-value","Type":"String","Overwrite":true}'

    # GetParameter
    bench_targets "ssm" "get_parameter" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SSM_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonSSM.GetParameter" \
      -d '{"Name":"/bench/param-'"$$"'"}'

    # DescribeParameters
    bench_targets "ssm" "describe_parameters" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SSM_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonSSM.DescribeParameters" \
      -d '{}'
  else
    skip_service "ssm" "Failed to create seed parameter"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.4 ACM (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "acm"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "ACM (JSON)"

  ACM_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # No seed needed — RequestCertificate is the write, ListCertificates is the read

  # RequestCertificate
  bench_targets "acm" "request_certificate" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${ACM_HEADERS[@]}" \
    -H "X-Amz-Target: CertificateManager.RequestCertificate" \
    -d '{"DomainName":"bench-'"$$"'.example.com","ValidationMethod":"DNS"}'

  # ListCertificates
  bench_targets "acm" "list_certificates" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${ACM_HEADERS[@]}" \
    -H "X-Amz-Target: CertificateManager.ListCertificates" \
    -d '{}'
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.5 CloudWatch (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "cloudwatch"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "CloudWatch (Query-XML)"

  # PutMetricData
  bench_targets "cloudwatch" "put_metric_data" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=PutMetricData&Namespace=BenchNS&MetricData.member.1.MetricName=BenchMetric&MetricData.member.1.Value=42&MetricData.member.1.Unit=Count&Version=2010-08-01"

  # GetMetricStatistics
  bench_targets "cloudwatch" "get_metric_statistics" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=GetMetricStatistics&Namespace=BenchNS&MetricName=BenchMetric&StartTime=2020-01-01T00:00:00Z&EndTime=2030-01-01T00:00:00Z&Period=3600&Statistics.member.1=Average&Version=2010-08-01"

  # ListMetrics
  bench_targets "cloudwatch" "list_metrics" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=ListMetrics&Version=2010-08-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.6 EventBridge (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "events"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "EventBridge (JSON)"

  EB_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: put rule (each target independently; only openstack is required)
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: AWSEvents.PutRule" \
       -d '{"Name":"bench-rule-'"$$"'","ScheduleExpression":"rate(1 hour)","State":"ENABLED"}'; then

    # PutRule (overwrite)
    bench_targets "events" "put_rule" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EB_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.PutRule" \
      -d '{"Name":"bench-rule-'"$$"'","ScheduleExpression":"rate(1 hour)","State":"ENABLED"}'

    # ListRules
    bench_targets "events" "list_rules" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EB_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.ListRules" \
      -d '{}'

    # ListTargetsByRule
    bench_targets "events" "list_targets_by_rule" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EB_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.ListTargetsByRule" \
      -d '{"Rule":"bench-rule-'"$$"'"}'
  else
    skip_service "events" "Failed to create seed rule"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.7 Step Functions (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "states"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Step Functions (JSON)"

  SF_HEADERS=(-H "Content-Type: application/x-amz-json-1.0")

  # Seed: create state machine
  OS_SM_ARN=$(curl -sf -X POST "$OS_BASE" \
    -H "Content-Type: application/x-amz-json-1.0" \
    -H "X-Amz-Target: AWSStepFunctions.CreateStateMachine" \
    -d '{"name":"bench-sm-'"$$"'","definition":"{\"StartAt\":\"Pass\",\"States\":{\"Pass\":{\"Type\":\"Pass\",\"End\":true}}}","roleArn":"arn:aws:iam::000000000000:role/bench-role"}' \
    | jq -r '.stateMachineArn // empty' || echo "")
  LS_SM_ARN=""
  if target_active ls; then
    LS_SM_ARN=$(curl -sf -X POST "$LS_BASE" \
      -H "Content-Type: application/x-amz-json-1.0" \
      -H "X-Amz-Target: AWSStepFunctions.CreateStateMachine" \
      -d '{"name":"bench-sm-'"$$"'","definition":"{\"StartAt\":\"Pass\",\"States\":{\"Pass\":{\"Type\":\"Pass\",\"End\":true}}}","roleArn":"arn:aws:iam::000000000000:role/bench-role"}' \
      | jq -r '.stateMachineArn // empty' || echo "")
  fi
  MOTO_SM_ARN=""
  if target_active moto; then
    MOTO_SM_ARN=$(curl -sf -X POST "$MOTO_BASE" \
      -H "Content-Type: application/x-amz-json-1.0" \
      -H "X-Amz-Target: AWSStepFunctions.CreateStateMachine" \
      -d '{"name":"bench-sm-'"$$"'","definition":"{\"StartAt\":\"Pass\",\"States\":{\"Pass\":{\"Type\":\"Pass\",\"End\":true}}}","roleArn":"arn:aws:iam::000000000000:role/bench-role"}' \
      | jq -r '.stateMachineArn // empty' || echo "")
  fi

  if [[ -n "$OS_SM_ARN" ]]; then
    # StartExecution (target-specific ARNs require separate bench calls)
    log "  states/start_execution (openstack)..."
    bench "states" "start_execution" "os" POST "$OS_BASE" \
      "${SF_HEADERS[@]}" \
      -H "X-Amz-Target: AWSStepFunctions.StartExecution" \
      -d '{"stateMachineArn":"'"$OS_SM_ARN"'","input":"{}"}'
    if target_active ls && [[ -n "$LS_SM_ARN" ]]; then
      log "  states/start_execution (localstack)..."
      bench "states" "start_execution" "ls" POST "$LS_BASE" \
        "${SF_HEADERS[@]}" \
        -H "X-Amz-Target: AWSStepFunctions.StartExecution" \
        -d '{"stateMachineArn":"'"$LS_SM_ARN"'","input":"{}"}'
    fi
    if target_active moto && [[ -n "$MOTO_SM_ARN" ]]; then
      log "  states/start_execution (moto)..."
      bench "states" "start_execution" "moto" POST "$MOTO_BASE" \
        "${SF_HEADERS[@]}" \
        -H "X-Amz-Target: AWSStepFunctions.StartExecution" \
        -d '{"stateMachineArn":"'"$MOTO_SM_ARN"'","input":"{}"}'
    fi

    # ListStateMachines
    bench_targets "states" "list_state_machines" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${SF_HEADERS[@]}" \
      -H "X-Amz-Target: AWSStepFunctions.ListStateMachines" \
      -d '{}'
  else
    skip_service "states" "Failed to create seed state machine"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.8 API Gateway (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "apigateway"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "API Gateway (REST-JSON)"

  # Seed: create REST API
  OS_API_ID=$(curl -sf -X POST "$OS_BASE/restapis" \
    -H "Content-Type: application/json" \
    -d '{"name":"bench-api-'"$$"'"}' \
    | jq -r '.id // empty' || echo "")
  LS_API_ID=""
  if target_active ls; then
    # shellcheck disable=SC2034 # seeded for side effect; variable retained for debugging
    LS_API_ID=$(curl -sf -X POST "$LS_BASE/restapis" \
      -H "Content-Type: application/json" \
      -d '{"name":"bench-api-'"$$"'"}' \
      | jq -r '.id // empty' || echo "")
  fi
  MOTO_API_ID=""
  if target_active moto; then
    # shellcheck disable=SC2034 # seeded for side effect; variable retained for debugging
    MOTO_API_ID=$(curl -sf -X POST "$MOTO_BASE/restapis" \
      -H "Content-Type: application/json" \
      -d '{"name":"bench-api-'"$$"'"}' \
      | jq -r '.id // empty' || echo "")
  fi

  if [[ -n "$OS_API_ID" ]]; then
    # CreateRestApi
    bench_targets "apigateway" "create_rest_api" POST \
      "$OS_BASE/restapis" \
      "$LS_BASE/restapis" \
      "$MOTO_BASE/restapis" \
      -H "Content-Type: application/json" \
      -d '{"name":"bench-api-create-'"$$"'"}'

    # GetRestApis
    bench_targets "apigateway" "get_rest_apis" GET \
      "$OS_BASE/restapis" \
      "$LS_BASE/restapis" \
      "$MOTO_BASE/restapis"
  else
    skip_service "apigateway" "Failed to create seed REST API"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.9 EC2 (EC2-Query)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ec2"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "EC2 (EC2-Query)"

  # No seed needed

  # DescribeInstances
  bench_targets "ec2" "describe_instances" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=DescribeInstances&Version=2016-11-15"

  # DescribeVpcs
  bench_targets "ec2" "describe_vpcs" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=DescribeVpcs&Version=2016-11-15"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.10 Route53 (REST-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "route53"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Route53 (REST-XML)"

  # Seed: create hosted zone
  OS_HZ_ID=$(curl -sf -X POST "$OS_BASE/2013-04-01/hostedzone" \
    -H "Content-Type: application/xml" \
    -d '<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><Name>bench-'"$$"'.example.com</Name><CallerReference>bench-'"$$"'</CallerReference></CreateHostedZoneRequest>' \
    | grep -oP '(?<=<Id>)[^<]+' || echo "")
  if target_active ls; then
    # shellcheck disable=SC2034 # seeded for side effect; variable retained for debugging
    LS_HZ_ID=$(curl -sf -X POST "$LS_BASE/2013-04-01/hostedzone" \
      -H "Content-Type: application/xml" \
      -d '<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><Name>bench-'"$$"'.example.com</Name><CallerReference>bench-'"$$"'</CallerReference></CreateHostedZoneRequest>' \
      | grep -oP '(?<=<Id>)[^<]+' || echo "")
  fi
  if target_active moto; then
    # shellcheck disable=SC2034 # seeded for side effect; variable retained for debugging
    MOTO_HZ_ID=$(curl -sf -X POST "$MOTO_BASE/2013-04-01/hostedzone" \
      -H "Content-Type: application/xml" \
      -d '<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><Name>bench-'"$$"'.example.com</Name><CallerReference>bench-'"$$"'</CallerReference></CreateHostedZoneRequest>' \
      | grep -oP '(?<=<Id>)[^<]+' || echo "")
  fi

  if [[ -n "$OS_HZ_ID" ]]; then
    # CreateHostedZone (will create more zones)
    bench_targets "route53" "create_hosted_zone" POST \
      "$OS_BASE/2013-04-01/hostedzone" \
      "$LS_BASE/2013-04-01/hostedzone" \
      "$MOTO_BASE/2013-04-01/hostedzone" \
      -H "Content-Type: application/xml" \
      -d '<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/"><Name>bench-create-'"$$"'.example.com</Name><CallerReference>bench-create-'"$$"'</CallerReference></CreateHostedZoneRequest>'

    # ListHostedZones
    bench_targets "route53" "list_hosted_zones" GET \
      "$OS_BASE/2013-04-01/hostedzone" \
      "$LS_BASE/2013-04-01/hostedzone" \
      "$MOTO_BASE/2013-04-01/hostedzone"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.11 SES (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ses"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "SES (Query-XML)"

  # No seed needed

  # VerifyEmailIdentity
  bench_targets "ses" "verify_email_identity" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=VerifyEmailIdentity&EmailAddress=bench@example.com&Version=2010-12-01"

  # ListIdentities
  bench_targets "ses" "list_identities" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=ListIdentities&Version=2010-12-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.12 ECR (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ecr"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "ECR (JSON)"

  ECR_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  # Seed: create repository (each target independently; only openstack is required)
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository" \
       -d '{"repositoryName":"bench-repo-'"$$"'"}'; then

    # CreateRepository
    bench_targets "ecr" "create_repository" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${ECR_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository" \
      -d '{"repositoryName":"bench-repo-create-'"$$"'"}'

    # DescribeRepositories
    bench_targets "ecr" "describe_repositories" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${ECR_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.DescribeRepositories" \
      -d '{}'
  else
    skip_service "ecr" "Failed to create seed repository"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.13 OpenSearch (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "opensearch"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "OpenSearch (REST-JSON)"

  # CreateDomain
  bench_targets "opensearch" "create_domain" POST \
    "$OS_BASE/2021-01-01/opensearch/domain" \
    "$LS_BASE/2021-01-01/opensearch/domain" \
    "$MOTO_BASE/2021-01-01/opensearch/domain" \
    -H "Content-Type: application/json" \
    -d '{"DomainName":"bench-os-'"$$"'"}'

  # ListDomainNames
  bench_targets "opensearch" "list_domain_names" GET \
    "$OS_BASE/2021-01-01/domain" \
    "$LS_BASE/2021-01-01/domain" \
    "$MOTO_BASE/2021-01-01/domain"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.14 Redshift (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "redshift"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Redshift (Query-XML)"

  # CreateCluster
  bench_targets "redshift" "create_cluster" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=CreateCluster&ClusterIdentifier=bench-cluster-$$&NodeType=dc2.large&MasterUsername=admin&MasterUserPassword=BenchPass123&Version=2012-12-01"

  # DescribeClusters
  bench_targets "redshift" "describe_clusters" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=DescribeClusters&Version=2012-12-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.15 CloudFormation (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "cloudformation"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "CloudFormation (Query-XML)"

  CFN_TEMPLATE='{"AWSTemplateFormatVersion":"2010-09-09","Description":"Bench","Resources":{"BenchTopic":{"Type":"AWS::SNS::Topic","Properties":{"TopicName":"bench-cfn-topic-'"$$"'"}}}}'

  # Seed: create stack (each target independently; only openstack is required)
  if seed_all_targets POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -d "Action=CreateStack&StackName=bench-stack-$$&TemplateBody=$CFN_TEMPLATE&Version=2010-05-15"; then

    # CreateStack
    bench_targets "cloudformation" "create_stack" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=CreateStack&StackName=bench-stack-create-$$&TemplateBody=$CFN_TEMPLATE&Version=2010-05-15"

    # DescribeStacks
    bench_targets "cloudformation" "describe_stacks" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=DescribeStacks&Version=2010-05-15"
  else
    skip_service "cloudformation" "Failed to create seed stack"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4.16 Lambda (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "lambda"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1  # reset per-service
  log_section "Lambda (REST-JSON)"

  # Seed: create function with inline zip
  # Minimal Python zip for Lambda
  LAMBDA_ZIP_B64="UEsDBBQAAAAIAAAAAACKIYOwHwAAAB0AAAAJABwAaW5kZXgucHlVVAkAA0FUamYAAAAAAGNgYGBi"

  LAMBDA_CREATE_BODY='{"FunctionName":"bench-fn-'"$$"'","Runtime":"python3.9","Role":"arn:aws:iam::000000000000:role/bench-role","Handler":"index.handler","Code":{"ZipFile":"'"$LAMBDA_ZIP_B64"'"}}'

  OS_FN_ARN=$(curl -sf -X POST "$OS_BASE/2015-03-31/functions" \
    -H "Content-Type: application/json" \
    -d "$LAMBDA_CREATE_BODY" \
    | jq -r '.FunctionArn // empty' || echo "")
  LS_FN_ARN=""
  if target_active ls; then
    LS_FN_ARN=$(curl -sf -X POST "$LS_BASE/2015-03-31/functions" \
      -H "Content-Type: application/json" \
      -d "$LAMBDA_CREATE_BODY" \
      | jq -r '.FunctionArn // empty' || echo "")
  fi
  MOTO_FN_ARN=""
  if target_active moto; then
    MOTO_FN_ARN=$(curl -sf -X POST "$MOTO_BASE/2015-03-31/functions" \
      -H "Content-Type: application/json" \
      -d "$LAMBDA_CREATE_BODY" \
      | jq -r '.FunctionArn // empty' || echo "")
  fi

  if [[ -n "$OS_FN_ARN" ]]; then
    sleep 1

    # Invoke (target-specific function names require separate bench calls)
    log "  lambda/invoke (openstack)..."
    bench "lambda" "invoke" "os" POST \
      "$OS_BASE/2015-03-31/functions/bench-fn-$$/invocations" \
      -H "Content-Type: application/json" \
      -d '{}'
    if target_active ls && [[ -n "$LS_FN_ARN" ]]; then
      log "  lambda/invoke (localstack)..."
      bench "lambda" "invoke" "ls" POST \
        "$LS_BASE/2015-03-31/functions/bench-fn-$$/invocations" \
        -H "Content-Type: application/json" \
        -d '{}'
    fi
    if target_active moto && [[ -n "$MOTO_FN_ARN" ]]; then
      log "  lambda/invoke (moto)..."
      bench "lambda" "invoke" "moto" POST \
        "$MOTO_BASE/2015-03-31/functions/bench-fn-$$/invocations" \
        -H "Content-Type: application/json" \
        -d '{}'
    fi

    # GetFunction
    bench_targets "lambda" "get_function" GET \
      "$OS_BASE/2015-03-31/functions/bench-fn-$$" \
      "$LS_BASE/2015-03-31/functions/bench-fn-$$" \
      "$MOTO_BASE/2015-03-31/functions/bench-fn-$$"

    # ListFunctions
    bench_targets "lambda" "list_functions" GET \
      "$OS_BASE/2015-03-31/functions" \
      "$LS_BASE/2015-03-31/functions" \
      "$MOTO_BASE/2015-03-31/functions"
  else
    skip_service "lambda" "Failed to create seed function"
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
