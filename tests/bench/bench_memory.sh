#!/usr/bin/env bash
# bench_memory.sh
#
# Measures openstack binary memory usage at idle and under load.
#
# Targets:
#   Idle:  <50 MB RSS
#   Load:  reported for reference (no hard target)
#
# Prerequisites:
#   - openstack binary built: cargo build --release
#   - curl installed
#   - ps (standard on macOS and Linux)
#
# Usage:
#   ./bench_memory.sh
#   BINARY=/path/to/openstack ./bench_memory.sh
#   LOAD_REQUESTS=200 ./bench_memory.sh

set -euo pipefail

BINARY="${BINARY:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)/target/release/openstack}"
PORT="${PORT:-14567}"
IDLE_TARGET_MB=50
LOAD_REQUESTS="${LOAD_REQUESTS:-200}"
HEALTH_URL="http://127.0.0.1:${PORT}/_localstack/health"

if [ ! -x "${BINARY}" ]; then
    echo "ERROR: binary not found at ${BINARY}"
    echo "Build it first: cargo build --release --bin openstack"
    exit 1
fi

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

wait_healthy() {
    local url="$1"
    for _ in $(seq 1 200); do
        if curl -sf "${url}" > /dev/null 2>&1; then
            return 0
        fi
        sleep 0.05
    done
    echo "ERROR: server did not become healthy within 10 s"
    return 1
}

# Returns RSS in MB for the given PID.
rss_mb() {
    local pid="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        # macOS: ps RSS is in KB
        local rss_kb
        rss_kb=$(ps -o rss= -p "${pid}" 2>/dev/null | tr -d ' ' || echo 0)
        echo $(( rss_kb / 1024 ))
    else
        # Linux: /proc/<pid>/status VmRSS is in kB
        local rss_kb
        rss_kb=$(awk '/^VmRSS:/{print $2}' "/proc/${pid}/status" 2>/dev/null || echo 0)
        echo $(( rss_kb / 1024 ))
    fi
}

# ---------------------------------------------------------------------------
# Start server
# ---------------------------------------------------------------------------

echo "=== openstack memory benchmark ==="
echo "Binary: ${BINARY}"
echo "Idle target: <${IDLE_TARGET_MB} MB RSS"
echo

GATEWAY_LISTEN="127.0.0.1:${PORT}" \
PERSISTENCE=0 \
LS_LOG=error \
    "${BINARY}" &
SERVER_PID=$!

# Ensure we kill the server on exit
trap 'kill "${SERVER_PID}" 2>/dev/null || true; wait "${SERVER_PID}" 2>/dev/null || true' EXIT

if ! wait_healthy "${HEALTH_URL}"; then
    exit 1
fi

# ---------------------------------------------------------------------------
# Idle measurement (sample RSS 5 times over 2 s)
# ---------------------------------------------------------------------------

echo "--- Idle memory (settling for 1 s) ---"
sleep 1

IDLE_SAMPLES=()
for _ in $(seq 1 5); do
    IDLE_SAMPLES+=("$(rss_mb "${SERVER_PID}")")
    sleep 0.4
done

IDLE_TOTAL=0
IDLE_MIN=${IDLE_SAMPLES[0]}
IDLE_MAX=${IDLE_SAMPLES[0]}
for s in "${IDLE_SAMPLES[@]}"; do
    IDLE_TOTAL=$((IDLE_TOTAL + s))
    [ "${s}" -lt "${IDLE_MIN}" ] && IDLE_MIN="${s}"
    [ "${s}" -gt "${IDLE_MAX}" ] && IDLE_MAX="${s}"
done
IDLE_AVG=$((IDLE_TOTAL / ${#IDLE_SAMPLES[@]}))

printf "  Min idle RSS: %4d MB\n" "${IDLE_MIN}"
printf "  Avg idle RSS: %4d MB\n" "${IDLE_AVG}"
printf "  Max idle RSS: %4d MB\n" "${IDLE_MAX}"
echo

IDLE_PASS=true
if [ "${IDLE_AVG}" -gt "${IDLE_TARGET_MB}" ]; then
    IDLE_PASS=false
fi

# ---------------------------------------------------------------------------
# Load measurement — fire LOAD_REQUESTS sequential health checks
# ---------------------------------------------------------------------------

echo "--- Load memory (${LOAD_REQUESTS} sequential requests) ---"

for _ in $(seq 1 "${LOAD_REQUESTS}"); do
    curl -sf "${HEALTH_URL}" > /dev/null 2>&1 || true
done

# Sample RSS 5 times over 1 s after load
LOAD_SAMPLES=()
for _ in $(seq 1 5); do
    LOAD_SAMPLES+=("$(rss_mb "${SERVER_PID}")")
    sleep 0.2
done

LOAD_TOTAL=0
LOAD_MIN=${LOAD_SAMPLES[0]}
LOAD_MAX=${LOAD_SAMPLES[0]}
for s in "${LOAD_SAMPLES[@]}"; do
    LOAD_TOTAL=$((LOAD_TOTAL + s))
    [ "${s}" -lt "${LOAD_MIN}" ] && LOAD_MIN="${s}"
    [ "${s}" -gt "${LOAD_MAX}" ] && LOAD_MAX="${s}"
done
LOAD_AVG=$((LOAD_TOTAL / ${#LOAD_SAMPLES[@]}))

printf "  Min load RSS: %4d MB\n" "${LOAD_MIN}"
printf "  Avg load RSS: %4d MB\n" "${LOAD_AVG}"
printf "  Max load RSS: %4d MB\n" "${LOAD_MAX}"
echo

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo "=== Results ==="
if "${IDLE_PASS}"; then
    echo "PASS — idle avg RSS ${IDLE_AVG} MB <= ${IDLE_TARGET_MB} MB target"
else
    echo "FAIL — idle avg RSS ${IDLE_AVG} MB > ${IDLE_TARGET_MB} MB target"
fi
echo "INFO — load avg RSS ${LOAD_AVG} MB (${LOAD_REQUESTS} requests; informational)"

"${IDLE_PASS}" && exit 0 || exit 1
