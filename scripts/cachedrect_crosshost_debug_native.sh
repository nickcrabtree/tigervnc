#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug for macOS using native TigerVNC Viewer
# No X11 required - uses native macOS viewer

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
TUNNEL_PORT=6898
SERVER_PORT=6898

# Use native macOS TigerVNC Viewer
VIEWER_APP="/Applications/MacPorts/TigerVNC Viewer.app/Contents/MacOS/TigerVNC Viewer"

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_APP}" ]]; then
  echo "ERROR: TigerVNC Viewer not found at: ${VIEWER_APP}" >&2
  echo "Install via: sudo port install tigervnc" >&2
  exit 1
fi

echo "[1/5] Starting remote server on ${REMOTE}..."
ssh "${REMOTE}" "cd ${REMOTE_DIR} && nohup python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 90 </dev/null >/tmp/cachedrect_server_stdout.log 2>&1 &"

echo "[2/5] Waiting for remote server readiness..."
for i in {1..60}; do
  if ssh "${REMOTE}" "grep -q 'SERVER_READY' /tmp/cachedrect_server_stdout.log 2>/dev/null"; then
    echo "[ok] Remote server is ready"
    break
  fi
  sleep 1
  if (( i == 60 )); then
    echo "ERROR: Server failed to start" >&2
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
echo "[3/5] Connection mode: ${MODE}"

TUNNEL_PID=""
TARGET_HOST=""
if [[ "${MODE}" == "tunnel" ]]; then
  echo "[3a/5] Setting up SSH tunnel..."
  ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}"
  sleep 1
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET_HOST="localhost"
  TARGET_PORT="${TUNNEL_PORT}"
else
  TARGET_HOST="${REMOTE#*@}"
  TARGET_PORT="${SERVER_PORT}"
fi

echo "[4/5] Launching TigerVNC Viewer..."
echo "      Target: ${TARGET_HOST}:${TARGET_PORT}"
echo "      Logs: ${VIEWER_LOG}"
echo ""
echo "  The viewer window will open."
echo "  Observe the display, then close the window when done."
echo ""

# Note: Native macOS viewer doesn't support -Log flag the same way
# We'll capture what we can from its output
set +e
"${VIEWER_APP}" "${TARGET_HOST}:${TARGET_PORT}" 2>&1 | tee "${VIEWER_LOG}"
VIEWER_RC=$?
set -e

echo ""
echo "[viewer closed with code ${VIEWER_RC}]"

echo "[5/5] Retrieving and analyzing logs..."
REMOTE_ART_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/contentcache_server_only*.log 2>/dev/null | head -1 || true")
if [[ -z "${REMOTE_ART_LOG}" ]]; then
  REMOTE_ART_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")
fi

if [[ -n "${REMOTE_ART_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_ART_LOG}" "${LOCAL_SERVER_LOG}"
  echo "[ok] Server log: ${LOCAL_SERVER_LOG}"
  
  # Show server stats
  echo ""
  echo "=== Server-side ContentCache Statistics ==="
  grep -A 5 "ContentCache statistics" "${LOCAL_SERVER_LOG}" || echo "(no statistics found)"
  
  echo ""
  echo "=== Server Cache Activity ==="
  grep -c "ContentCache protocol hit" "${LOCAL_SERVER_LOG}" 2>/dev/null | \
    xargs -I{} echo "  ContentCache hits logged: {}" || echo "  No hits found"
  
  echo ""
  echo "NOTE: The native macOS viewer doesn't provide verbose protocol logs."
  echo "For detailed client-side analysis, we need to use the C++ viewer build."
  echo ""
  echo "To test with full logging, you would need to:"
  echo "  1. Use the C++ viewer: build/vncviewer/njcvncviewer"
  echo "  2. Run it with DISPLAY=:0 (your desktop) or in a separate X session"
  echo ""
  echo "Logs saved to: ${LOCAL_LOG_DIR}"
else
  echo "WARN: Could not retrieve server log"
  ssh "${REMOTE}" "tail -100 /tmp/cachedrect_server_stdout.log" || true
fi

# Cleanup
if [[ -n "${TUNNEL_PID}" ]]; then
  kill "${TUNNEL_PID}" 2>/dev/null || true
fi

# Ask about stopping remote server
echo ""
read -p "Stop the remote test server? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
  ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py' || pkill -f 'Xnjcvnc :998' || true"
  echo "Remote server stopped"
fi

echo ""
echo "Test complete. Logs in: ${LOCAL_LOG_DIR}"
