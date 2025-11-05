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
DISPLAY_NUM="${DISPLAY_NUM:-999}"
SERVER_PORT=$((5900 + DISPLAY_NUM))  # VNC port = 5900 + display number
TUNNEL_PORT=${SERVER_PORT}
VIEWER_DURATION="${VIEWER_DURATION:-60}"  # How long to run viewer (seconds)
NONINTERACTIVE="${NONINTERACTIVE:-0}"  # Set to 1 for non-interactive mode

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: VIEWER_BIN not found or not executable: ${VIEWER_BIN}" >&2
  exit 1
fi

# macOS native viewer doesn't require X11/XQuartz

# Function to wait for remote port to be listening
wait_for_remote_port() {
  local remote="$1"
  local port="$2"
  local timeout_s="${3:-60}"
  local interval="${4:-2}"

  local start now
  start=$(date +%s)

  while true; do
    # Try ss first, then netstat
    if timeout 10 ssh -o BatchMode=yes -o ConnectTimeout=5 "$remote" \
      'if command -v ss >/dev/null 2>&1; then ss -tln; elif command -v netstat >/dev/null 2>&1; then netstat -tln; else echo "NO_SS_OR_NETSTAT"; fi' 2>/dev/null \
      | grep -q ":${port}\b"; then
      return 0
    fi

    now=$(date +%s)
    if [ $((now - start)) -ge "$timeout_s" ]; then
      echo "ERROR: Server did not listen on :${port} within ${timeout_s}s" >&2
      return 1
    fi
    sleep "$interval"
  done
}

echo "[1/6] Starting remote server on ${REMOTE} (display :${DISPLAY_NUM}, port ${SERVER_PORT})..."
# Background the entire SSH command locally to avoid waiting for remote command
(timeout 30 ssh "${REMOTE}" "cd ${REMOTE_DIR} && nohup python3 scripts/server_only_cachedrect_test.py --display ${DISPLAY_NUM} --port ${SERVER_PORT} --duration 60 >/tmp/cachedrect_server_stdout.log 2>&1 </dev/null & echo \$! > /tmp/cachedrect_server.pid" </dev/null >/dev/null 2>&1) &
SSH_START_PID=$!

# Give SSH command time to execute
sleep 3

# Wait for SSH command to complete (with timeout)
wait ${SSH_START_PID} 2>/dev/null || true

echo "[2/6] Waiting for remote server readiness..."
if wait_for_remote_port "${REMOTE}" "${SERVER_PORT}" 60; then
  echo "[ok] Remote server is listening on port ${SERVER_PORT}"
else
  echo "ERROR: Server not ready; aborting." >&2
  timeout 10 ssh "${REMOTE}" "tail -50 /tmp/cachedrect_server_stdout.log" || true
  exit 2
fi

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
  timeout 30 ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}" &
  sleep 2
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET_HOST="localhost"
else
  TARGET_HOST="${REMOTE#*@}"
fi

echo "[4/6] Starting local viewer..."
echo "      Logs: ${VIEWER_LOG}"
echo "      Target: ${TARGET_HOST}::${SERVER_PORT}"

if [[ "${NONINTERACTIVE}" == "1" ]]; then
  echo "      Mode: Non-interactive (${VIEWER_DURATION}s timeout)"
  echo ""
  
  # Start viewer in background
  set +e
  "${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET_HOST}::${SERVER_PORT}" 2>&1 | tee "${VIEWER_LOG}" &
  VIEWER_PID=$!
  set -e
  
  echo "  Viewer started (PID ${VIEWER_PID}), running for ${VIEWER_DURATION}s..."
  
  # Wait for specified duration
  sleep "${VIEWER_DURATION}"
  
  # Kill viewer
  echo "  Stopping viewer after ${VIEWER_DURATION}s..."
  if kill -0 "${VIEWER_PID}" 2>/dev/null; then
    kill "${VIEWER_PID}" 2>/dev/null || true
    sleep 2
    # Force kill if still running
    if kill -0 "${VIEWER_PID}" 2>/dev/null; then
      kill -9 "${VIEWER_PID}" 2>/dev/null || true
    fi
  fi
  VIEWER_RC=0
else
  echo "      Mode: Interactive"
  echo ""
  echo "  The TigerVNC viewer window will appear on your desktop."
  echo "  Use it normally, then close it when done testing."
  echo ""
  
  set +e
  "${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET_HOST}::${SERVER_PORT}" 2>&1 | tee "${VIEWER_LOG}"
  VIEWER_RC=$?
  set -e
fi

echo "[viewer exit code] ${VIEWER_RC}"

echo "[5/6] Retrieving server log..."
REMOTE_ART_LOG=$(timeout 30 ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/contentcache_server_only*.log 2>/dev/null | head -1 || true")
if [[ -z "${REMOTE_ART_LOG}" ]]; then
  echo "WARN: Could not locate remote log, checking alternative names..."
  REMOTE_ART_LOG=$(timeout 30 ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")
fi

if [[ -n "${REMOTE_ART_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  timeout 60 scp -q "${REMOTE}:${REMOTE_ART_LOG}" "${LOCAL_SERVER_LOG}"
  echo "[ok] Downloaded server log"

  echo "[6/6] Comparing logs..."
  timeout 30 python3 scripts/compare_cachedrect_logs.py --server "${LOCAL_SERVER_LOG}" --client "${VIEWER_LOG}" || true
else
  echo "WARN: Could not find server log, showing stdout:"
  timeout 30 ssh "${REMOTE}" "tail -100 /tmp/cachedrect_server_stdout.log" || true
fi

# Cleanup
if [[ -n "${TUNNEL_PID}" ]]; then
  kill "${TUNNEL_PID}" 2>/dev/null || true
fi

# Stop remote server
if [[ "${NONINTERACTIVE}" == "1" ]]; then
  echo ""
  echo "Stopping remote test server..."
  timeout 30 ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py' || true"
  timeout 30 ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :${DISPLAY_NUM}' | grep -v grep | awk '{print \$2}' | xargs -r kill 2>/dev/null || true"
else
  # Ask user if they want to stop the remote server
  echo ""
  read -t 60 -p "Stop the remote test server? (y/N) " -n 1 -r || true
  echo
  if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Stopping remote server..."
    timeout 30 ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py' || true"
    timeout 30 ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :${DISPLAY_NUM}' | grep -v grep | awk '{print \$2}' | xargs -r kill 2>/dev/null || true"
  fi
fi

echo ""
echo "Done. Logs in ${LOCAL_LOG_DIR}"
echo "  Server: ${LOCAL_SERVER_LOG:-not retrieved}"
echo "  Client: ${VIEWER_LOG}"
