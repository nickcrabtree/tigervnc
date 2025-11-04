#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug - C++ viewer with full logging
# Uses XQuartz :0 but you can minimize the window

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
TUNNEL_PORT=6898
SERVER_PORT=6898

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: Viewer not found: ${VIEWER_BIN}" >&2
  exit 1
fi

# Ensure XQuartz is running
if ! pgrep -x "Xquartz" > /dev/null; then
  echo "[0/6] Starting XQuartz (needed for viewer GUI)..."
  open -a XQuartz
  for i in {1..30}; do
    if pgrep -x "Xquartz" > /dev/null; then
      sleep 2
      break
    fi
    sleep 1
  done
fi

export DISPLAY=:0

echo "[1/6] Starting remote server..."
ssh "${REMOTE}" "cd ${REMOTE_DIR} && nohup python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 90 </dev/null >/tmp/cachedrect_server_stdout.log 2>&1 &"

echo "[2/6] Waiting for server..."
for i in {1..60}; do
  if ssh "${REMOTE}" "grep -q 'SERVER_READY' /tmp/cachedrect_server_stdout.log 2>/dev/null"; then
    echo "[ok] Server ready"
    break
  fi
  sleep 1
  if (( i == 60 )); then
    echo "ERROR: Timeout" >&2
    exit 2
  fi
done

# Connection mode
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
echo "[3/6] Mode: ${MODE}"

TUNNEL_PID=""
if [[ "${MODE}" == "tunnel" ]]; then
  ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}"
  sleep 1
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET="${TUNNEL_PORT}"
else
  TARGET="${REMOTE#*@}::${SERVER_PORT}"
fi

echo "[4/6] Launching viewer with full logging..."
echo "      Target: ${TARGET}"
echo "      Log: ${VIEWER_LOG}"
echo ""
echo "  A viewer window will appear - you can minimize it."
echo "  It will collect protocol logs for ~30 seconds."
echo "  Close the window when you've seen enough activity."
echo ""

# Run viewer with maximum verbosity
set +e
"${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET}" 2>&1 | tee "${VIEWER_LOG}"
VIEWER_RC=$?
set -e

echo ""
echo "[5/6] Analyzing logs..."

# Get server log
REMOTE_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")
if [[ -n "${REMOTE_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_LOG}" "${LOCAL_SERVER_LOG}"
  
  echo "[6/6] Comparison:"
  python3 scripts/compare_cachedrect_logs.py --server "${LOCAL_SERVER_LOG}" --client "${VIEWER_LOG}" 2>/dev/null || {
    # Fallback manual analysis
    echo ""
    echo "=== Server Stats ==="
    grep "Lookups:" "${LOCAL_SERVER_LOG}" | tail -1 || echo "No server stats"
    
    echo ""
    echo "=== Client Activity ==="
    echo "  Cache misses: $(grep -c 'Cache miss for ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  Stores: $(grep -c 'Storing decoded rect.*cache ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  CachedRectInit: $(grep -c 'CachedRectInit' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    
    echo ""
    echo "Check logs for details:"
    echo "  Server: ${LOCAL_SERVER_LOG}"
    echo "  Client: ${VIEWER_LOG}"
  }
else
  echo "WARN: Could not retrieve server log"
fi

# Cleanup
[[ -n "${TUNNEL_PID}" ]] && kill "${TUNNEL_PID}" 2>/dev/null || true

echo ""
read -p "Stop remote server? (y/N) " -n 1 -r
echo
[[ $REPLY =~ ^[Yy]$ ]] && ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py|Xnjcvnc :998' || true"

echo ""
echo "Done. Logs: ${LOCAL_LOG_DIR}"
