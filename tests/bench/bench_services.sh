#!/usr/bin/env bash
# bench_services.sh
#
# Comprehensive shell-based benchmark script for openstack.
# Benchmarks all implemented AWS service crates against openstack, LocalStack,
# and moto,
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
LOCALSTACK_DOCKER_SOCKET_ARGS=()

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

configure_localstack_docker_socket_mount() {
  if [[ -S "/var/run/docker.sock" ]]; then
    LOCALSTACK_DOCKER_SOCKET_ARGS=(-v "/var/run/docker.sock:/var/run/docker.sock")
    log "LocalStack Lambda executor: Docker socket mounted"
  else
    LOCALSTACK_DOCKER_SOCKET_ARGS=()
    log "WARN: /var/run/docker.sock not found; LocalStack Lambda create/invoke may fail"
  fi
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

  # Fast path for default profile: limit dynamic samples so services that
  # require unique request payloads do not dominate overall benchmark time.
  # Stress profile keeps full cardinality.
  local dynamic_n="$REQ_COUNT"
  if [[ "$PROFILE" == "default" && "$REQ_COUNT" -gt 40 ]]; then
    dynamic_n=40
  fi
  local curl_max_time=15
  [[ "$PROFILE" == "stress" ]] && curl_max_time=30

  local start_ts end_ts wall_secs throughput
  start_ts=$(date +%s%3N)   # ms since epoch

  local i
  local pids=()
  for (( i=1; i<=dynamic_n; i++ )); do
    local body="${body_template//\{i\}/$i}"
    # Write result file: "time_total_ms http_code"
    (
      local sample time_s code time_ms
      sample=$(curl -s --connect-timeout 2 --max-time "$curl_max_time" \
        -o /dev/null -w '%{time_total} %{http_code}' -X "$method" \
        "${extra_args[@]}" -d "$body" "$url" 2>/dev/null) || sample="0 000"
      read -r time_s code <<< "$sample"
      # Convert fractional seconds to ms
      time_ms=$(awk "BEGIN{printf \"%.2f\", ${time_s:-0}*1000}")
      printf '%s %s\n' "$time_ms" "${code:-000}" > "$tmpdir/$i"
    ) &
    pids+=("$!")

    # Wait for the current batch only; do not wait on unrelated background jobs
    # such as the openstack binary running in --binary mode.
    if (( i % CONC == 0 )); then
      for pid in "${pids[@]}"; do
        wait "$pid"
      done
      pids=()
    fi
  done
  for pid in "${pids[@]}"; do
    wait "$pid"
  done

  end_ts=$(date +%s%3N)
  wall_secs=$(awk "BEGIN{printf \"%.3f\", ($end_ts - $start_ts)/1000}")

  # Collect results
  local times=() errors=0
  for (( i=1; i<=dynamic_n; i++ )); do
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
  # Connect timeout: 5s.  Max total time: 120s (enough for a 100 MiB seed on
  # a resource-constrained CI runner while still aborting on a server deadlock).
  response=$(curl -s --connect-timeout 5 --max-time 120 -w "\n%{http_code}" -X "$method" "$@" "$url" 2>/dev/null) || true
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

CORE_SERVICES="acm,apigateway,cloudformation,cloudwatch,dynamodb,ec2,ecr,eventbridge,firehose,iam,kinesis,kms,lambda,opensearch,redshift,route53,s3,secretsmanager,ses,sns,sqs,ssm,stepfunctions,sts"

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
OS_DATA_DIR=""

cleanup() {
  log "Cleaning up..."
  if [[ -n "$OS_CONTAINER" ]]; then docker rm -f "$OS_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$LS_CONTAINER" ]]; then docker rm -f "$LS_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$MOTO_CONTAINER" ]]; then docker rm -f "$MOTO_CONTAINER" &>/dev/null || true; fi
  if [[ -n "$OS_PID" ]]; then kill "$OS_PID" &>/dev/null || true; fi
  if [[ -n "$OS_PID" ]]; then wait "$OS_PID" &>/dev/null || true; fi
  if [[ -n "$OS_DATA_DIR" ]]; then rm -rf "$OS_DATA_DIR" &>/dev/null || true; fi
}
trap cleanup EXIT

port_in_use() {
  local port="$1"
  if command -v ss &>/dev/null; then
    ss -ltn "sport = :$port" 2>/dev/null | awk 'NR>1 {found=1} END {exit !found}'
    return $?
  fi
  if command -v lsof &>/dev/null; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1
    return $?
  fi
  return 1
}

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
    -e STUDIO=0 \
    -e DEBUG=0 \
    "$OPENSTACK_IMAGE")

  if target_active ls; then
    log "Starting LocalStack container..."
    LS_CONTAINER=$(docker run -d \
      --name "bench-localstack-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      "${LOCALSTACK_DOCKER_SOCKET_ARGS[@]}" \
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

  if port_in_use "$OS_PORT"; then
    echo "ERROR: Port $OS_PORT is already in use. Stop the existing process and retry." >&2
    exit 1
  fi

  # Use a writable temp directory for the data dir so the binary can be run
  # without root privileges (the default /var/lib/localstack requires root).
  OS_DATA_DIR=$(mktemp -d -t openstack-bench-XXXXXX)
  log "Starting openstack binary ($BINARY_PATH) with data dir $OS_DATA_DIR..."
  GATEWAY_LISTEN="127.0.0.1:$OS_PORT" \
  LOCALSTACK_DATA_DIR="$OS_DATA_DIR" \
  PERSISTENCE=0 \
  LS_LOG=error \
  STUDIO=0 \
  DEBUG=0 \
    "$BINARY_PATH" &
  OS_PID=$!

  if target_active ls; then
    log "Starting LocalStack container..."
    LS_CONTAINER=$(docker run -d \
      --name "bench-localstack-$$" \
      --cpus="$CPU_LIMIT" \
      --memory="$MEMORY_LIMIT" \
      "${LOCALSTACK_DOCKER_SOCKET_ARGS[@]}" \
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
  configure_localstack_docker_socket_mount
  start_binary_mode
else
  configure_localstack_docker_socket_mount
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

  # Moto's multi-service standalone server needs a Host header to route
  # Query-style SNS requests to its SNS backend (same pattern as S3).
  # The Authorization header is included defensively in case moto validates it.
  MOTO_EXTRA=(-H "Host: sns.us-east-1.amazonaws.com" \
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/sns/aws4_request, SignedHeaders=host, Signature=dummy")

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
      -d "Action=CreateTopic&Name=bench-topic-$$" "${MOTO_EXTRA[@]}" 2>/dev/null \
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
        -d "Action=Publish&TopicArn=$MOTO_TOPIC_ARN&Message=benchmark-message" \
        "${MOTO_EXTRA[@]}"
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
        -d "Action=GetTopicAttributes&TopicArn=$MOTO_TOPIC_ARN" \
        "${MOTO_EXTRA[@]}"
    fi

    # ListTopics — bench_targets appends MOTO_EXTRA automatically
    bench_targets "sns" "list_topics" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ListTopics"
  else
    skip_service "sns" "Failed to create seed topic"
  fi

  MOTO_EXTRA=()  # clear after SNS block
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
  # AWS requires the token to be 32–64 alphanumeric/hyphen characters; shorter
  # values (e.g. "bench-seed-12345" = 16 chars) cause LocalStack to reject with
  # HTTP 400.  Pad the PID to 25 digits so the full token is always ≥32 chars.
  _sm_seed_token=$(printf 'bench-seed-%025d' "$$")
  # Dynamic bench tokens: pad PID to 20 digits so token length is 34–37 chars
  # regardless of {i} (1–100), comfortably within the 32–64 char window.
  _sm_pid_pad=$(printf '%020d' "$$")
  if seed_all_targets "secretsmanager" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: secretsmanager.CreateSecret" \
       -d '{"Name":"bench-secret-'"$$"'","SecretString":"benchmark-secret-value","ClientRequestToken":"'"$_sm_seed_token"'"}'; then

    # CreateSecret — unique name and token per iteration via {i}; 0 errors expected
    bench_dynamic_targets "secretsmanager" "create_secret" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      '{"Name":"bench-secret-create-'"$$"'-{i}","SecretString":"new-secret-value","ClientRequestToken":"bench-create-'"$_sm_pid_pad"'-{i}"}' \
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

# ─────────────────────────────────────────────────────────────────────────────
# 3.9 ACM (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "acm"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "ACM (JSON)"

  ACM_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  bench_targets "acm" "list_certificates" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${ACM_HEADERS[@]}" \
    -H "X-Amz-Target: CertificateManager.ListCertificates" \
    -d '{}'
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.10 API Gateway (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "apigateway"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "API Gateway (REST-JSON)"

  # Moto's multi-service root can mis-route API Gateway REST requests to S3
  # unless SigV4-like headers are present.
  MOTO_EXTRA=(
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/apigateway/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy"
    -H "X-Amz-Date: 20260101T000000Z"
  )

  bench_dynamic_targets "apigateway" "create_rest_api" POST \
    "$OS_BASE/restapis" \
    "$LS_BASE/restapis" \
    "$MOTO_BASE/restapis" \
    '{"name":"bench-api-{i}"}' \
    -H "Content-Type: application/json"

  _apigw_seed_os=$(curl -s -X POST "$OS_BASE/restapis" -H "Content-Type: application/json" -d '{"name":"bench-api-get-os"}' || true)
  _apigw_seed_ls=$(curl -s -X POST "$LS_BASE/restapis" -H "Content-Type: application/json" -d '{"name":"bench-api-get-ls"}' || true)
  _apigw_seed_moto=$(curl -s -X POST "$MOTO_BASE/restapis" \
    -H "Content-Type: application/json" \
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/apigateway/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy" \
    -H "X-Amz-Date: 20260101T000000Z" \
    -d '{"name":"bench-api-get-moto"}' || true)

  _apigw_id_os=$(printf '%s' "$_apigw_seed_os" | jq -r '.id // empty' 2>/dev/null)
  _apigw_id_ls=$(printf '%s' "$_apigw_seed_ls" | jq -r '.id // empty' 2>/dev/null)
  _apigw_id_moto=$(printf '%s' "$_apigw_seed_moto" | jq -r '.id // empty' 2>/dev/null)

  bench_targets "apigateway" "get_rest_api" GET \
    "$OS_BASE/restapis/${_apigw_id_os:-missing}" \
    "$LS_BASE/restapis/${_apigw_id_ls:-missing}" \
    "$MOTO_BASE/restapis/${_apigw_id_moto:-missing}"

  bench_targets "apigateway" "get_rest_apis" GET \
    "$OS_BASE/restapis" \
    "$LS_BASE/restapis" \
    "$MOTO_BASE/restapis"

  MOTO_EXTRA=()
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.11 CloudFormation (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "cloudformation"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "CloudFormation (Query-XML)"

  _cfn_template='{"Resources":{"Bucket":{"Type":"AWS::S3::Bucket"}}}'
  _cfn_template_encoded=$(printf '%s' "$_cfn_template" | jq -sRr @uri)

  # Seed one stack for describe/list benchmarking.
  if seed_all_targets "cloudformation" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-www-form-urlencoded" \
       -d "Action=CreateStack&Version=2010-05-15&StackName=bench-stack-$$&TemplateBody=${_cfn_template_encoded}"; then

    bench_dynamic_targets "cloudformation" "create_stack" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "Action=CreateStack&Version=2010-05-15&StackName=bench-stack-create-$$-{i}&TemplateBody=${_cfn_template_encoded}" \
      -H "Content-Type: application/x-www-form-urlencoded"

    bench_targets "cloudformation" "describe_stacks" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=DescribeStacks&Version=2010-05-15&StackName=bench-stack-$$"

    bench_targets "cloudformation" "list_stacks" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=ListStacks&Version=2010-05-15"
  else
    skip_service "cloudformation" "Failed to create seed stack"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.12 CloudWatch (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "cloudwatch"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "CloudWatch (Query-XML)"

  bench_targets "cloudwatch" "put_metric_data" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=PutMetricData&Version=2010-08-01&Namespace=OpenStackBench&MetricData.member.1.MetricName=Latency&MetricData.member.1.Value=1"

  bench_targets "cloudwatch" "list_metrics" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=ListMetrics&Version=2010-08-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.13 EC2 (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ec2"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "EC2 (Query-XML)"

  # Seed one instance for describe benchmarking.
  if seed_all_targets "ec2" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-www-form-urlencoded" \
       -d "Action=RunInstances&Version=2016-11-15&ImageId=ami-00000000&InstanceType=t2.micro&MinCount=1&MaxCount=1"; then

    bench_dynamic_targets "ec2" "run_instances" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "Action=RunInstances&Version=2016-11-15&ImageId=ami-00000000&InstanceType=t2.micro&MinCount=1&MaxCount=1" \
      -H "Content-Type: application/x-www-form-urlencoded"

    bench_targets "ec2" "describe_instances" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-www-form-urlencoded" \
      -d "Action=DescribeInstances&Version=2016-11-15"
  else
    skip_service "ec2" "Failed to create seed instance"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.14 ECR (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ecr"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "ECR (JSON)"

  ECR_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")
  _ecr_repo="bench-ecr-$$"
  _ecr_tag="bench-img-$$"
  # Minimal valid OCI image manifest (stored as a string; jq encodes it properly
  # when embedding as a JSON string value — raw interpolation breaks on the inner quotes)
  _ecr_manifest='{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","size":0,"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"},"layers":[]}'
  _ecr_put_image_body=$(jq -n \
    --arg repo "$_ecr_repo" \
    --arg manifest "$_ecr_manifest" \
    --arg tag "$_ecr_tag" \
    '{"repositoryName":$repo,"imageManifest":$manifest,"imageTag":$tag}')

  if seed_all_targets "ecr" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository" \
       -d '{"repositoryName":"'"$_ecr_repo"'"}'; then

    # Seed image in repository so read benchmarks have data (best-effort; read
    # benchmarks run regardless and simply return empty results if this fails)
    seed_all_targets "ecr" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.PutImage" \
      -d "$_ecr_put_image_body" || true

    # CreateRepository — unique name per iteration; 0 errors expected
    bench_dynamic_targets "ecr" "create_repository" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      '{"repositoryName":"bench-ecr-'"$$"'-{i}"}' \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.CreateRepository"

    # DescribeRepositories
    bench_targets "ecr" "describe_repositories" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${ECR_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.DescribeRepositories" \
      -d '{}'

    # ListImages
    bench_targets "ecr" "list_images" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${ECR_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.ListImages" \
      -d '{"repositoryName":"'"$_ecr_repo"'"}'

    # BatchGetImage
    bench_targets "ecr" "batch_get_image" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${ECR_HEADERS[@]}" \
      -H "X-Amz-Target: AmazonEC2ContainerRegistry_V20150921.BatchGetImage" \
      -d '{"repositoryName":"'"$_ecr_repo"'","imageIds":[{"imageTag":"'"$_ecr_tag"'"}]}'
  else
    skip_service "ecr" "Failed to create seed ECR repository"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.15 EventBridge (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "eventbridge"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "EventBridge (JSON)"

  EVENTS_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")
  _eb_bus="bench-bus-$$"
  _eb_rule="bench-rule-$$"

  if seed_all_targets "eventbridge" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-amz-json-1.1" \
       -H "X-Amz-Target: AWSEvents.CreateEventBus" \
       -d '{"Name":"'"$_eb_bus"'"}'; then

    # Seed a rule on the custom bus
    seed_all_targets "eventbridge" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: AWSEvents.PutRule" \
      -d '{"Name":"'"$_eb_rule"'","ScheduleExpression":"rate(5 minutes)","EventBusName":"'"$_eb_bus"'"}'

    # Seed a target on the rule
    seed_all_targets "eventbridge" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: AWSEvents.PutTargets" \
      -d '{"Rule":"'"$_eb_rule"'","EventBusName":"'"$_eb_bus"'","Targets":[{"Id":"t1","Arn":"arn:aws:sqs:us-east-1:000000000000:bench-queue"}]}'

    # PutRule — unique name per iteration; 0 errors expected
    bench_dynamic_targets "eventbridge" "put_rule" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      '{"Name":"bench-rule-'"$$"'-{i}","ScheduleExpression":"rate(5 minutes)","EventBusName":"'"$_eb_bus"'"}' \
      -H "Content-Type: application/x-amz-json-1.1" \
      -H "X-Amz-Target: AWSEvents.PutRule"

    # ListEventBuses
    bench_targets "eventbridge" "list_event_buses" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EVENTS_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.ListEventBuses" \
      -d '{}'

    # ListRules
    bench_targets "eventbridge" "list_rules" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EVENTS_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.ListRules" \
      -d '{"EventBusName":"'"$_eb_bus"'"}'

    # DescribeRule
    bench_targets "eventbridge" "describe_rule" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EVENTS_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.DescribeRule" \
      -d '{"Name":"'"$_eb_rule"'","EventBusName":"'"$_eb_bus"'"}'

    # ListTargetsByRule
    bench_targets "eventbridge" "list_targets_by_rule" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
      "${EVENTS_HEADERS[@]}" \
      -H "X-Amz-Target: AWSEvents.ListTargetsByRule" \
      -d '{"Rule":"'"$_eb_rule"'","EventBusName":"'"$_eb_bus"'"}'
  else
    skip_service "eventbridge" "Failed to create seed EventBridge bus"
  fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.16 KMS (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "kms"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "KMS (JSON)"

  KMS_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  bench_targets "kms" "list_keys" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${KMS_HEADERS[@]}" \
    -H "X-Amz-Target: TrentService.ListKeys" \
    -d '{}'
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.17 Lambda (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "lambda"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "Lambda (REST-JSON)"

  LAMBDA_FUNCTION_NAME="bench-fn-$$"
  LAMBDA_DELETE_FUNCTION_NAME="bench-fn-del-$$"
  LAMBDA_ROLE_NAME="bench-role-$$"
  LAMBDA_ROLE_POLICY='{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}'
  LAMBDA_ROLE_ARN_DEFAULT="arn:aws:iam::000000000000:role/${LAMBDA_ROLE_NAME}"
  LAMBDA_ROLE_ARN_MOTO="arn:aws:iam::123456789012:role/${LAMBDA_ROLE_NAME}"
  LAMBDA_ZIP_B64="UEsDBBQAAAAAABatdlysKm9YNQAAADUAAAASAAAAbGFtYmRhX2Z1bmN0aW9uLnB5ZGVmIGhhbmRsZXIoZXZlbnQsIGNvbnRleHQpOgogICAgcmV0dXJuIHsib2siOiBUcnVlfQpQSwECFAMUAAAAAAAWrXZcrCpvWDUAAAA1AAAAEgAAAAAAAAAAAAAAgAEAAAAAbGFtYmRhX2Z1bmN0aW9uLnB5UEsFBgAAAAABAAEAQAAAAGUAAAAAAA=="

  LAMBDA_CREATE_BODY_OS=$(jq -cn \
    --arg fn "$LAMBDA_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_DEFAULT" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  LAMBDA_CREATE_BODY_LS=$(jq -cn \
    --arg fn "$LAMBDA_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_DEFAULT" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  LAMBDA_CREATE_BODY_MOTO=$(jq -cn \
    --arg fn "$LAMBDA_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_MOTO" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  LAMBDA_DELETE_CREATE_BODY_OS=$(jq -cn \
    --arg fn "$LAMBDA_DELETE_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_DEFAULT" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  LAMBDA_DELETE_CREATE_BODY_LS=$(jq -cn \
    --arg fn "$LAMBDA_DELETE_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_DEFAULT" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  LAMBDA_DELETE_CREATE_BODY_MOTO=$(jq -cn \
    --arg fn "$LAMBDA_DELETE_FUNCTION_NAME" \
    --arg zip "$LAMBDA_ZIP_B64" \
    --arg role "$LAMBDA_ROLE_ARN_MOTO" \
    '{
      FunctionName: $fn,
      Runtime: "python3.12",
      Handler: "lambda_function.handler",
      Role: $role,
      Code: { ZipFile: $zip }
    }')

  # Moto's multi-service root can mis-route Lambda REST requests to S3 unless
  # SigV4-like headers are present. The signature content is not validated, so
  # static dummy values are sufficient to force Lambda routing.
  MOTO_EXTRA=(
    -H "Host: lambda.us-east-1.amazonaws.com"
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/lambda/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy"
    -H "X-Amz-Date: 20260101T000000Z"
  )

  # Create IAM role first for targets that validate RoleAssumePolicy.
  seed_request "os" POST "$OS_BASE" \
    --data-urlencode "Action=CreateRole" \
    --data-urlencode "Version=2010-05-08" \
    --data-urlencode "RoleName=${LAMBDA_ROLE_NAME}" \
    --data-urlencode "AssumeRolePolicyDocument=${LAMBDA_ROLE_POLICY}" >/dev/null 2>&1 || true
  if target_active ls; then
    seed_request "ls" POST "$LS_BASE" \
      --data-urlencode "Action=CreateRole" \
      --data-urlencode "Version=2010-05-08" \
      --data-urlencode "RoleName=${LAMBDA_ROLE_NAME}" \
      --data-urlencode "AssumeRolePolicyDocument=${LAMBDA_ROLE_POLICY}" >/dev/null 2>&1 || true
  fi
  if target_active moto; then
    seed_request "moto" POST "$MOTO_BASE" \
      -H "Host: iam.amazonaws.com" \
      -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/iam/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy" \
      -H "X-Amz-Date: 20260101T000000Z" \
      --data-urlencode "Action=CreateRole" \
      --data-urlencode "Version=2010-05-08" \
      --data-urlencode "RoleName=${LAMBDA_ROLE_NAME}" \
      --data-urlencode "AssumeRolePolicyDocument=${LAMBDA_ROLE_POLICY}" >/dev/null 2>&1 || true
  fi

  SEED_OS=0; SEED_LS=0; SEED_MOTO=0
  seed_request "os" POST "$OS_BASE/2015-03-31/functions/" \
    -H "Content-Type: application/json" \
    -d "$LAMBDA_CREATE_BODY_OS" && SEED_OS=1 || true
  if target_active ls; then
    seed_request "ls" POST "$LS_BASE/2015-03-31/functions/" \
      -H "Content-Type: application/json" \
      -d "$LAMBDA_CREATE_BODY_LS" && SEED_LS=1 \
      || record_seed_failure "lambda" "ls" "seed request failed"
  fi
  if target_active moto; then
    seed_request "moto" POST "$MOTO_BASE/2015-03-31/functions/" \
      -H "Content-Type: application/json" \
      -d "$LAMBDA_CREATE_BODY_MOTO" \
      "${MOTO_EXTRA[@]}" && SEED_MOTO=1 \
      || record_seed_failure "lambda" "moto" "seed request failed"
  fi

  if [[ $SEED_OS -eq 1 ]]; then

    bench_targets "lambda" "list_functions" GET \
      "$OS_BASE/2015-03-31/functions/" \
      "$LS_BASE/2015-03-31/functions/" \
      "$MOTO_BASE/2015-03-31/functions/"

    bench_targets "lambda" "get_function" GET \
      "$OS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}" \
      "$LS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}" \
      "$MOTO_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}"

    bench_targets "lambda" "invoke" POST \
      "$OS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/invocations" \
      "$LS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/invocations" \
      "$MOTO_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/invocations" \
      -H "Content-Type: application/json" \
      -H "X-Amz-Invocation-Type: DryRun" \
      -d '{}'

    bench_targets "lambda" "update_function_configuration" PUT \
      "$OS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}" \
      "$LS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}" \
      "$MOTO_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}" \
      -H "Content-Type: application/json" \
      -d '{"Description":"bench-updated","Timeout":30,"MemorySize":256}'

    bench_targets "lambda" "update_function_code" PUT \
      "$OS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/code" \
      "$LS_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/code" \
      "$MOTO_BASE/2015-03-31/functions/${LAMBDA_FUNCTION_NAME}/code" \
      -H "Content-Type: application/json" \
      -d "{\"ZipFile\":\"${LAMBDA_ZIP_B64}\"}"

    # Dedicated delete seed to avoid removing the main benchmark function.
    _lambda_seed_os=$SEED_OS
    _lambda_seed_ls=$SEED_LS
    _lambda_seed_moto=$SEED_MOTO

    _lambda_del_seed_os=0
    _lambda_del_seed_ls=0
    _lambda_del_seed_moto=0

    if [[ $_lambda_seed_os -eq 1 ]]; then
      seed_request "os" POST "$OS_BASE/2015-03-31/functions/" \
        -H "Content-Type: application/json" \
        -d "$LAMBDA_DELETE_CREATE_BODY_OS" && _lambda_del_seed_os=1 || true
    fi
    if target_active ls && [[ $_lambda_seed_ls -eq 1 ]]; then
      seed_request "ls" POST "$LS_BASE/2015-03-31/functions/" \
        -H "Content-Type: application/json" \
        -d "$LAMBDA_DELETE_CREATE_BODY_LS" && _lambda_del_seed_ls=1 \
        || record_seed_failure "lambda" "ls" "delete seed request failed"
    fi
    if target_active moto && [[ $_lambda_seed_moto -eq 1 ]]; then
      seed_request "moto" POST "$MOTO_BASE/2015-03-31/functions/" \
        -H "Content-Type: application/json" \
        -d "$LAMBDA_DELETE_CREATE_BODY_MOTO" \
        "${MOTO_EXTRA[@]}" && _lambda_del_seed_moto=1 \
        || record_seed_failure "lambda" "moto" "delete seed request failed"
    fi

    if [[ $_lambda_del_seed_os -eq 1 ]]; then
      SEED_OS=$_lambda_del_seed_os
      SEED_LS=$_lambda_del_seed_ls
      SEED_MOTO=$_lambda_del_seed_moto
      _lambda_saved_req=$REQ_COUNT
      _lambda_saved_conc=$CONC
      REQ_COUNT=1
      CONC=1
      bench_targets "lambda" "delete_function" DELETE \
        "$OS_BASE/2015-03-31/functions/${LAMBDA_DELETE_FUNCTION_NAME}" \
        "$LS_BASE/2015-03-31/functions/${LAMBDA_DELETE_FUNCTION_NAME}" \
        "$MOTO_BASE/2015-03-31/functions/${LAMBDA_DELETE_FUNCTION_NAME}"
      REQ_COUNT=$_lambda_saved_req
      CONC=$_lambda_saved_conc
      SEED_OS=$_lambda_seed_os
      SEED_LS=$_lambda_seed_ls
      SEED_MOTO=$_lambda_seed_moto
    else
      record_seed_failure "lambda" "os" "delete seed request failed"
    fi
  else
    skip_service "lambda" "Failed to create seed function"
  fi

  MOTO_EXTRA=()
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.18 OpenSearch (REST-JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "opensearch"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "OpenSearch (REST-JSON)"

  # Moto's multi-service root can mis-route OpenSearch REST requests to S3
  # unless SigV4-like headers are present.
  MOTO_EXTRA=(
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/es/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy"
    -H "X-Amz-Date: 20260101T000000Z"
  )

  bench_targets "opensearch" "list_domain_names" GET \
    "$OS_BASE/2021-01-01/opensearch/domain" \
    "$LS_BASE/2021-01-01/opensearch/domain" \
    "$MOTO_BASE/2021-01-01/opensearch/domain"

  MOTO_EXTRA=()
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.19 Redshift (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "redshift"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "Redshift (Query-XML)"

  bench_targets "redshift" "describe_clusters" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=DescribeClusters&Version=2012-12-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.20 Route53 (REST-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "route53"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "Route53 (REST-XML)"

  # Moto's multi-service root can mis-route Route53 REST requests to S3 unless
  # SigV4-like headers are present.
  MOTO_EXTRA=(
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/route53/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy"
    -H "X-Amz-Date: 20260101T000000Z"
  )

  bench_targets "route53" "list_hosted_zones" GET \
    "$OS_BASE/2013-04-01/hostedzone" \
    "$LS_BASE/2013-04-01/hostedzone" \
    "$MOTO_BASE/2013-04-01/hostedzone"

  MOTO_EXTRA=()
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.21 SES (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ses"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "SES (Query-XML)"

  bench_targets "ses" "list_identities" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "Action=ListIdentities&Version=2010-12-01"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.22 SQS (Query-XML)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "sqs"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "SQS (Query-XML)"

  # Moto needs SQS routing hints; otherwise query requests may be routed to S3.
  MOTO_EXTRA=(-H "Host: sqs.us-east-1.amazonaws.com" \
    -H "X-Amz-Date: 20260101T000000Z" \
    -H "Authorization: AWS4-HMAC-SHA256 Credential=testing/20260101/us-east-1/sqs/aws4_request, SignedHeaders=host;x-amz-date, Signature=dummy")

  SQS_QUEUE_NAME="bench-queue-$$"
  SQS_QUEUE_URL="http://localhost:4566/000000000000/${SQS_QUEUE_NAME}"
  SQS_QUEUE_URL_ENC=$(jq -nr --arg v "$SQS_QUEUE_URL" '$v|@uri')

  if seed_all_targets "sqs" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
       -H "Content-Type: application/x-www-form-urlencoded" \
       -d "Action=CreateQueue&Version=2012-11-05&QueueName=${SQS_QUEUE_NAME}"; then

    if seed_all_targets "sqs" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
         -H "Content-Type: application/x-www-form-urlencoded" \
         -d "Action=SendMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MessageBody=seed-message"; then

      bench_targets "sqs" "list_queues" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=ListQueues&Version=2012-11-05"

      bench_targets "sqs" "send_message" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=SendMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MessageBody=benchmark-message"

      bench_targets "sqs" "receive_message" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "Action=ReceiveMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MaxNumberOfMessages=1&VisibilityTimeout=0"

      # Ensure a fresh visible message exists before extracting receipt handles
      # for delete_message benchmarking. Some targets may mark earlier messages
      # invisible during the receive benchmark window.
      if [[ "${SEED_OS:-0}" -eq 1 ]]; then
        seed_request "os" POST "$OS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=SendMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MessageBody=delete-seed-message" || true
      fi
      if target_active ls && [[ "${SEED_LS:-0}" -eq 1 ]]; then
        seed_request "ls" POST "$LS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=SendMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MessageBody=delete-seed-message" || true
      fi
      if target_active moto && [[ "${SEED_MOTO:-0}" -eq 1 ]]; then
        seed_request "moto" POST "$MOTO_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=SendMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MessageBody=delete-seed-message" \
          "${MOTO_EXTRA[@]}" || true
      fi

      SQS_RH_OS=""
      if [[ "${SEED_OS:-0}" -eq 1 ]]; then
        _sqs_recv_os=$(curl -s -X POST "$OS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=ReceiveMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MaxNumberOfMessages=1") || true
        SQS_RH_OS="${_sqs_recv_os#*ReceiptHandle>}"
        if [[ "$SQS_RH_OS" != "$_sqs_recv_os" ]]; then
          SQS_RH_OS="${SQS_RH_OS%%<*}"
        else
          SQS_RH_OS=""
        fi
      fi

      SQS_RH_LS=""
      if target_active ls && [[ "${SEED_LS:-0}" -eq 1 ]]; then
        _sqs_recv_ls=$(curl -s -X POST "$LS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=ReceiveMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MaxNumberOfMessages=1") || true
        SQS_RH_LS="${_sqs_recv_ls#*ReceiptHandle>}"
        if [[ "$SQS_RH_LS" != "$_sqs_recv_ls" ]]; then
          SQS_RH_LS="${SQS_RH_LS%%<*}"
        else
          SQS_RH_LS=""
        fi
      fi

      SQS_RH_MOTO=""
      if target_active moto && [[ "${SEED_MOTO:-0}" -eq 1 ]]; then
        _sqs_recv_moto=$(curl -s -X POST "$MOTO_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=ReceiveMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&MaxNumberOfMessages=1" \
          "${MOTO_EXTRA[@]}") || true
        SQS_RH_MOTO="${_sqs_recv_moto#*ReceiptHandle>}"
        if [[ "$SQS_RH_MOTO" != "$_sqs_recv_moto" ]]; then
          SQS_RH_MOTO="${SQS_RH_MOTO%%<*}"
        else
          SQS_RH_MOTO=""
        fi
      fi

      if [[ -n "$SQS_RH_OS" ]]; then
        SQS_RH_OS_ENC=$(jq -nr --arg v "$SQS_RH_OS" '$v|@uri')
        log "  sqs/delete_message (openstack)..."
        bench "sqs" "delete_message" "os" POST "$OS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=DeleteMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&ReceiptHandle=${SQS_RH_OS_ENC}"
      fi
      if target_active ls && [[ -n "$SQS_RH_LS" ]]; then
        SQS_RH_LS_ENC=$(jq -nr --arg v "$SQS_RH_LS" '$v|@uri')
        log "  sqs/delete_message (localstack)..."
        bench "sqs" "delete_message" "ls" POST "$LS_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=DeleteMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&ReceiptHandle=${SQS_RH_LS_ENC}"
      fi
      if target_active moto && [[ -n "$SQS_RH_MOTO" ]]; then
        SQS_RH_MOTO_ENC=$(jq -nr --arg v "$SQS_RH_MOTO" '$v|@uri')
        log "  sqs/delete_message (moto)..."
        bench "sqs" "delete_message" "moto" POST "$MOTO_BASE" \
          -H "Content-Type: application/x-www-form-urlencoded" \
          -d "Action=DeleteMessage&Version=2012-11-05&QueueUrl=${SQS_QUEUE_URL_ENC}&ReceiptHandle=${SQS_RH_MOTO_ENC}"
      fi
    else
      skip_service "sqs" "Unable to seed SendMessage on openstack"
    fi
  else
    skip_service "sqs" "Unable to seed CreateQueue on openstack"
  fi

  MOTO_EXTRA=()
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.23 SSM (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "ssm"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "SSM (JSON)"

  SSM_HEADERS=(-H "Content-Type: application/x-amz-json-1.1")

  seed_all_targets "ssm" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${SSM_HEADERS[@]}" \
    -H "X-Amz-Target: AmazonSSM.PutParameter" \
    -d '{"Name":"/bench/static-param","Value":"seed","Type":"String","Overwrite":true}'

  bench_targets "ssm" "put_parameter" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${SSM_HEADERS[@]}" \
    -H "X-Amz-Target: AmazonSSM.PutParameter" \
    -d '{"Name":"/bench/static-param","Value":"updated-benchmark-value","Type":"String","Overwrite":true}'

  bench_targets "ssm" "get_parameter" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${SSM_HEADERS[@]}" \
    -H "X-Amz-Target: AmazonSSM.GetParameter" \
    -d '{"Name":"/bench/static-param"}'

  bench_targets "ssm" "describe_parameters" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${SSM_HEADERS[@]}" \
    -H "X-Amz-Target: AmazonSSM.DescribeParameters" \
    -d '{}'
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3.24 StepFunctions (JSON)
# ─────────────────────────────────────────────────────────────────────────────

if is_active "stepfunctions"; then
  SEED_OS=1; SEED_LS=1; SEED_MOTO=1
  log_section "StepFunctions (JSON)"

  SFN_HEADERS=(-H "Content-Type: application/x-amz-json-1.0")

  bench_targets "stepfunctions" "list_state_machines" POST "$OS_BASE" "$LS_BASE" "$MOTO_BASE" \
    "${SFN_HEADERS[@]}" \
    -H "X-Amz-Target: AWSStepFunctions.ListStateMachines" \
    -d '{}'
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
