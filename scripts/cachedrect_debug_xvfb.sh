#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug using Xvfb (virtual display)
# No interference with your desktop - runs on isolated display :99

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
XVFB="/opt/X11/bin/Xvfb"
LOCAL_DISPLAY=":99"
TUNNEL_PORT=7898
SERVER_PORT=7898

mkdir -p "${LOCAL_LOG_DIR}"

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: Viewer not found: ${VIEWER_BIN}" >&2
  exit 1
fi

if [[ ! -x "${XVFB}" ]]; then
  echo "ERROR: Xvfb not found at ${XVFB}" >&2
  exit 1
fi

echo "========================================================================"
echo "CachedRectInit Cross-Host Debug (Xvfb isolated display)"
echo "========================================================================"
echo ""

# Start local Xvfb on display :99
echo "[1/7] Starting Xvfb on display ${LOCAL_DISPLAY} (isolated, no desktop interference)..."
"${XVFB}" ${LOCAL_DISPLAY} -screen 0 1280x1024x24 >/dev/null 2>&1 &
XVFB_PID=$!
sleep 2

# Verify Xvfb started
if ! kill -0 ${XVFB_PID} 2>/dev/null; then
  echo "ERROR: Xvfb failed to start" >&2
  exit 1
fi
echo "  ✓ Xvfb running (PID ${XVFB_PID})"

export DISPLAY=${LOCAL_DISPLAY}

# Start remote server - background the SSH command locally
echo ""
echo "[2/7] Starting remote server on ${REMOTE}..."
ssh "${REMOTE}" "cd ${REMOTE_DIR} && python3 scripts/server_only_cachedrect_test.py --display 997 --port 7898 --duration 90 </dev/null >/tmp/cachedrect_server_stdout.log 2>&1" &
SSH_PID=$!
sleep 3  # Give server time to start

echo "[3/7] Waiting for remote server readiness..."
for i in {1..60}; do
  # Check if server is listening on the port
  if ssh "${REMOTE}" "lsof -i :${SERVER_PORT} >/dev/null 2>&1"; then
    echo "  ✓ Remote server ready (port ${SERVER_PORT} listening)"
    break
  fi
  sleep 1
  if (( i == 60 )); then
    echo "ERROR: Server timeout" >&2
    kill ${XVFB_PID} 2>/dev/null || true
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
echo "[4/7] Connection mode: ${MODE}"

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
echo "[5/7] Starting viewer with full protocol logging..."
echo "  Target: ${TARGET}"
echo "  Log: ${VIEWER_LOG}"
echo ""
echo "  NOTE: The viewer window will appear on your desktop."
echo "  (Native macOS FLTK doesn't support Xvfb isolation)"
echo "  Collecting protocol data for ~30 seconds..."
echo ""

# Run viewer with timeout to auto-close after collecting data
set +e
timeout 30s "${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET}" 2>&1 | tee "${VIEWER_LOG}" &
VIEWER_PID=$!

# Wait for viewer or timeout
wait ${VIEWER_PID} 2>/dev/null
VIEWER_RC=$?
set -e

echo ""
echo "[viewer collected data, exit code: ${VIEWER_RC}]"

echo ""
echo "[6/7] Retrieving and analyzing server log..."
REMOTE_LOG=$(ssh "${REMOTE}" "ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1 || true")

if [[ -n "${REMOTE_LOG}" ]]; then
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/server_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_LOG}" "${LOCAL_SERVER_LOG}"
  
  echo ""
  echo "[7/7] Protocol Analysis"
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
    echo "  Cache misses: $(grep -c 'Cache miss for ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  Stores: $(grep -c 'Storing decoded rect.*cache ID' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  CachedRectInit: $(grep -c 'CachedRectInit' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    echo "  SetEncodings: $(grep -c 'SetEncodings' "${VIEWER_LOG}" 2>/dev/null || echo 0)"
    
    echo ""
    echo "=== Key Issue Checks ==="
    if ! grep -q "CachedRect" "${VIEWER_LOG}"; then
      echo "  ⚠ WARNING: No ContentCache encoding references in client log"
      echo "             Client may not have advertised ContentCache capability"
    fi
    
    if grep -q "unknown encoding" "${VIEWER_LOG}"; then
      echo "  ⚠ WARNING: Client reported unknown encoding(s)"
    fi
  fi
  
  echo ""
  echo "========================================================================"
  echo "Logs saved:"
  echo "  Server: ${LOCAL_SERVER_LOG}"
  echo "  Client: ${VIEWER_LOG}"
else
  echo "WARN: Could not retrieve server log"
  ssh "${REMOTE}" "tail -50 /tmp/cachedrect_server_stdout.log" || true
fi

# Cleanup
echo ""
echo "Cleaning up..."
kill ${XVFB_PID} 2>/dev/null || true
[[ -n "${TUNNEL_PID}" ]] && kill "${TUNNEL_PID}" 2>/dev/null || true

echo ""
read -p "Stop remote server? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
  ssh "${REMOTE}" "pkill -f 'server_only_cachedrect_test.py|Xnjcvnc :998' || true"
  echo "Remote server stopped"
fi

echo ""
echo "Test complete!"
echo "Review logs in: ${LOCAL_LOG_DIR}"
