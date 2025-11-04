#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug orchestrator for macOS
# Uses local XQuartz for viewer GUI display

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
SERVER_STDOUT_REMOTE="/tmp/cachedrect_server_stdout.log"
TUNNEL_PORT=6898
SERVER_PORT=6898

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: VIEWER_BIN not found or not executable: ${VIEWER_BIN}" >&2
  exit 1
fi

# macOS native viewer doesn't require X11/XQuartz

echo "[1/6] Starting remote server on ${REMOTE}..."
ssh "${REMOTE}" "cd ${REMOTE_DIR} && python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 60 </dev/null >/tmp/cachedrect_server_stdout.log 2>&1 &"

echo "[2/6] Waiting for remote server readiness..."
for i in {1..60}; do
  if ssh "${REMOTE}" "grep -q 'SERVER_READY' /tmp/cachedrect_server_stdout.log 2>/dev/null"; then
    echo "[ok] Remote server reports SERVER_READY"
    break
  fi
  sleep 1
  if (( i == 60 )); then
    echo "ERROR: Timed out waiting for server" >&2
    ssh "${REMOTE}" "tail -50 /tmp/cachedrect_server_stdout.log" || true
    exit 2
  fi
done

# Decide connection mode
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
echo "[3/6] Connection mode: ${MODE}"

TUNNEL_PID=""
TARGET_HOST=""
if [[ "${MODE}" == "tunnel" ]]; then
  echo "[3a/6] Establishing SSH tunnel..."
  ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}"
  sleep 1
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET_HOST="localhost"
else
  TARGET_HOST="${REMOTE#*@}"
fi

echo "[4/6] Starting local viewer..."
echo "      Logs: ${VIEWER_LOG}"
echo "      Target: ${TARGET_HOST}::${SERVER_PORT}"
echo ""
echo "  The TigerVNC viewer window will appear on your desktop."
echo "  Use it normally, then close it when done testing."
echo ""

set +e
"${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET_HOST}::${SERVER_PORT}" 2>&1 | tee "${VIEWER_LOG}"
VIEWER_RC=$?
set -e
echo "[viewer exit code] ${VIEWER_RC}"

echo "[5/6] Retrieving server log..."
REMOTE_ART_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/contentcache_server_only*.log 2>/dev/null | head -1 || true")
if [[ -z "${REMOTE_ART_LOG}" ]]; then
  echo "WARN: Could not locate remote log, checking alternative names..."
  REMOTE_ART_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")
fi

if [[ -n "${REMOTE_ART_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_ART_LOG}" "${LOCAL_SERVER_LOG}"
  echo "[ok] Downloaded server log"

  echo "[6/6] Comparing logs..."
  python3 scripts/compare_cachedrect_logs.py --server "${LOCAL_SERVER_LOG}" --client "${VIEWER_LOG}" || true
else
  echo "WARN: Could not find server log, showing stdout:"
  ssh "${REMOTE}" "tail -100 /tmp/cachedrect_server_stdout.log" || true
fi

# Cleanup
if [[ -n "${TUNNEL_PID}" ]]; then
  kill "${TUNNEL_PID}" 2>/dev/null || true
fi

# Ask user if they want to stop the remote server
echo ""
read -p "Stop the remote test server? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
  echo "Stopping remote server..."
  ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py' || true"
  ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :998' | grep -v grep | awk '{print \$2}' | xargs -r kill 2>/dev/null || true"
fi

echo ""
echo "Done. Logs in ${LOCAL_LOG_DIR}"
echo "  Server: ${LOCAL_SERVER_LOG:-not retrieved}"
echo "  Client: ${VIEWER_LOG}"
