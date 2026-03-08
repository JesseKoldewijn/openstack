#!/usr/bin/env bash
# bench_startup.sh
#
# Measures openstack binary startup time (time from process launch to the
# /_localstack/health endpoint returning 200).
#
# Target: < 1 second
#
# Prerequisites:
#   - openstack binary built: cargo build --release
#   - curl installed
#
# Usage:
#   ./bench_startup.sh
#   BINARY=/path/to/openstack ./bench_startup.sh
#   RUNS=10 ./bench_startup.sh

set -euo pipefail

BINARY="${BINARY:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)/target/release/openstack}"
PORT="${PORT:-14566}"
RUNS="${RUNS:-5}"
TARGET_MS=1000
HEALTH_URL="http://127.0.0.1:${PORT}/_localstack/health"

if [ ! -x "${BINARY}" ]; then
    echo "ERROR: binary not found at ${BINARY}"
    echo "Build it first: cargo build --release --bin openstack"
    exit 1
fi

echo "=== openstack startup benchmark ==="
echo "Binary: ${BINARY}"
echo "Runs:   ${RUNS}"
echo "Target: <${TARGET_MS}ms"
echo

TIMES=()

for i in $(seq 1 "${RUNS}"); do
    # Start the binary in the background
    GATEWAY_LISTEN="127.0.0.1:${PORT}" \
    PERSISTENCE=0 \
    LS_LOG=error \
        "${BINARY}" &
    SERVER_PID=$!

    START_NS=$(date +%s%N)

    # Poll until healthy (max 10 s)
    READY=false
    for _ in $(seq 1 200); do
        if curl -sf "${HEALTH_URL}" > /dev/null 2>&1; then
            READY=true
            break
        fi
        sleep 0.05
    done

    END_NS=$(date +%s%N)
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true

    if ! "${READY}"; then
        echo "  Run ${i}: TIMEOUT — server did not become ready"
        continue
    fi

    ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))
    TIMES+=("${ELAPSED_MS}")
    STATUS="PASS"
    if [ "${ELAPSED_MS}" -gt "${TARGET_MS}" ]; then
        STATUS="FAIL (target: <${TARGET_MS}ms)"
    fi
    printf "  Run %-2s: %5dms  %s\n" "${i}" "${ELAPSED_MS}" "${STATUS}"

    # Brief pause before next run
    sleep 0.2
done

echo
if [ ${#TIMES[@]} -eq 0 ]; then
    echo "No successful runs."
    exit 1
fi

# Compute stats
TOTAL=0
MIN=${TIMES[0]}
MAX=${TIMES[0]}
for t in "${TIMES[@]}"; do
    TOTAL=$((TOTAL + t))
    [ "${t}" -lt "${MIN}" ] && MIN="${t}"
    [ "${t}" -gt "${MAX}" ] && MAX="${t}"
done
AVG=$((TOTAL / ${#TIMES[@]}))

echo "=== Results ==="
printf "  Min: %5dms\n" "${MIN}"
printf "  Avg: %5dms\n" "${AVG}"
printf "  Max: %5dms\n" "${MAX}"
echo

if [ "${AVG}" -le "${TARGET_MS}" ]; then
    echo "PASS — average startup time ${AVG}ms <= ${TARGET_MS}ms target"
    exit 0
else
    echo "FAIL — average startup time ${AVG}ms > ${TARGET_MS}ms target"
    exit 1
fi
