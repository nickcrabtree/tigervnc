#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug for macOS
# Viewer window will appear on desktop (native FLTK)

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
TUNNEL_PORT=7898
SERVER_PORT=7898

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: Viewer not found: ${VIEWER_BIN}" >&2
  exit 1
fi

echo "========================================================================"
echo "CachedRectInit Cross-Host Debug"
echo "========================================================================"
echo ""
echo "NOTE: Viewer window will appear on your desktop (you can minimize it)"
echo ""

# Start remote server
echo "[1/5] Starting remote server on ${REMOTE}..."
ssh "${REMOTE}" "cd ${REMOTE_DIR} && python3 scripts/server_only_cachedrect_test.py --display 997 --port 7898 --duration 90 </dev/null >/tmp/cachedrect_server_stdout.log 2>&1" &
SSH_PID=$!
sleep 3

echo "[2/5] Waiting for remote server readiness..."
for i in {1..60}; do
  if ssh "${REMOTE}" "lsof -i :${SERVER_PORT} >/dev/null 2>&1"; then
    echo "  ✓ Remote server ready (port ${SERVER_PORT} listening)"
    break
  fi
  sleep 1
  if (( i == 60 )); then
    echo "ERROR: Server timeout" >&2
    exit 2
  fi
done

# Determine connection mode
pick_mode() {
  if [[ "${MODE}" == "lan" || "${MODE}" == "tunnel" ]]; then
    echo "${MODE}"
  else
    if nc -z -w 1 "${REMOTE#*@}" ${SERVER_PORT} 2>/dev/null; then
      echo "lan"
    else
      echo "tunnel"
    fi
  fi
}
MODE="$(pick_mode)"
echo ""
echo "[3/5] Connection mode: ${MODE}"

TUNNEL_PID=""
if [[ "${MODE}" == "tunnel" ]]; then
  echo "  Setting up SSH tunnel..."
  ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}"
  sleep 1
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET="localhost::${TUNNEL_PORT}"
else
  TARGET="${REMOTE#*@}::${SERVER_PORT}"
fi

echo ""
echo "[4/5] Starting viewer with full protocol logging..."
echo "  Target: ${TARGET}"
echo "  Log: ${VIEWER_LOG}"
echo ""
echo "  Collecting protocol data for ~30 seconds..."
echo "  You can minimize the viewer window."
echo ""

# Run viewer with timeout
set +e
timeout 30s "${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET}" 2>&1 | tee "${VIEWER_LOG}" &
VIEWER_PID=$!
wait ${VIEWER_PID} 2>/dev/null
VIEWER_RC=$?
set -e

echo ""
echo "[viewer collected data, exit code: ${VIEWER_RC}]"

echo ""
echo "[5/5] Retrieving and analyzing server log..."
REMOTE_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")

if [[ -n "${REMOTE_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_LOG}" "${LOCAL_SERVER_LOG}"
  
  echo ""
  echo "Protocol Analysis"
  echo "========================================================================"
  
  if [[ -x "scripts/compare_cachedrect_logs.py" ]]; then
    python3 scripts/compare_cachedrect_logs.py --server "${LOCAL_SERVER_LOG}" --client "${VIEWER_LOG}"
  else
    # Manual analysis
    echo ""
    echo "=== Server Statistics ==="
    grep "Lookups:" "${LOCAL_SERVER_LOG}" | tail -1 || echo "No stats found"
    
    echo ""
    echo "=== Client Activity ==="
    echo "  CachedRectInit: $(grep -c 'Received CachedRectInit' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  CachedRect: $(grep -c 'Received CachedRect:' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  Cache hits: $(grep -c 'Cache hit for ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  Stores: $(grep -c 'Storing decoded rect.*cache ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
  fi
  
  echo ""
  echo "========================================================================"
  echo "Logs saved:"
  echo "  Server: ${LOCAL_SERVER_LOG}"
  echo "  Client: ${VIEWER_LOG}"
else
  echo "WARN: Could not retrieve server log"
fi

# Cleanup
echo ""
echo "Cleaning up..."
[[ -n "${TUNNEL_PID}" ]] && kill "${TUNNEL_PID}" 2>/dev/null || true

echo ""
read -p "Stop remote server? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
  # Find and kill specific test server PID (safe - only targets display 997)
  TEST_PID=$(ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :997' | grep -v grep | awk '{print \$2}'")
  if [[ -n "${TEST_PID}" ]]; then
    ssh "${REMOTE}" "kill -TERM ${TEST_PID}"
    echo "Remote test server stopped (PID ${TEST_PID})"
  fi
fi

echo ""
echo "Test complete!"
