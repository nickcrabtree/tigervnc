#!/usr/bin/env bash
set -euo pipefail

# Test ContentCache hit functionality with automated server/viewer
# Server runs on quartz:998, viewer on local Mac (window will appear)

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="/home/nickc/code/tigervnc"
TEST_DISPLAY=998
TEST_PORT=6898
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
TEST_DURATION=45

echo "========================================================================"
echo "ContentCache Hit Rate Test"
echo "========================================================================"
echo ""
echo "This will:"
echo "  1. Start test server on ${REMOTE} display :${TEST_DISPLAY}"
echo "  2. Connect viewer (window will appear - you can minimize it)"
echo "  3. Trigger screen changes to populate cache"
echo "  4. Check for cache hits on repeated content"
echo "  5. Report results from both server and client"
echo ""

# Clean up function
cleanup() {
  echo ""
  echo "Cleaning up..."
  if [[ -n "${VIEWER_PID:-}" ]]; then
    kill "${VIEWER_PID}" 2>/dev/null || true
  fi
  if [[ -n "${SSH_PID:-}" ]]; then
    kill "${SSH_PID}" 2>/dev/null || true
  fi
  # Kill test server by specific display
  TEST_SERVER_PID=$(ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :${TEST_DISPLAY}' | grep -v grep | awk '{print \$2}'" || true)
  if [[ -n "${TEST_SERVER_PID}" ]]; then
    ssh "${REMOTE}" "kill -TERM ${TEST_SERVER_PID}"
    echo "Test server stopped (PID ${TEST_SERVER_PID})"
  fi
}
trap cleanup EXIT

# Check viewer exists
if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: Viewer not found: ${VIEWER_BIN}"
  exit 1
fi

# Kill any existing test server on :998
echo "[1/6] Checking for existing test server..."
EXISTING_PID=$(ssh "${REMOTE}" "ps aux | grep 'Xnjcvnc :${TEST_DISPLAY}' | grep -v grep | awk '{print \$2}'" || true)
if [[ -n "${EXISTING_PID}" ]]; then
  echo "  Stopping existing test server (PID ${EXISTING_PID})..."
  ssh "${REMOTE}" "kill -TERM ${EXISTING_PID}"
  sleep 2
fi

# Start test server with ContentCache enabled
echo "[2/6] Starting test server on ${REMOTE} display :${TEST_DISPLAY}..."
ssh "${REMOTE}" "cd ${REMOTE_DIR}/tests/e2e && python3 run_contentcache_test.py --display-content ${TEST_DISPLAY} --port-content ${TEST_PORT} --duration ${TEST_DURATION} --server-modes local --skip-rust </dev/null >/tmp/test_cache_server.log 2>&1" &
SSH_PID=$!

# Wait for server ready
echo "[3/6] Waiting for server to be ready..."
for i in {1..30}; do
  if ssh "${REMOTE}" "lsof -i :${TEST_PORT} >/dev/null 2>&1"; then
    echo "  ✓ Server ready on port ${TEST_PORT}"
    break
  fi
  sleep 1
  if (( i == 30 )); then
    echo "ERROR: Server startup timeout"
    exit 1
  fi
done

# Connect viewer
echo "[4/6] Starting viewer (window will appear)..."
echo "  You can minimize the window - test continues automatically"
"${VIEWER_BIN}" -Log="*:stderr:100" "${REMOTE#*@}::${TEST_PORT}" >/tmp/test_cache_viewer.log 2>&1 &
VIEWER_PID=$!

sleep 3

# Generate test pattern: open/close windows repeatedly
echo "[5/6] Generating test pattern (${TEST_DURATION}s)..."
echo "  Phase 1: Opening/closing windows to build cache..."

ssh "${REMOTE}" bash <<EOF &
  export DISPLAY=:${TEST_DISPLAY}
  # Open xterm, move it, close it, repeat
  for i in {1..5}; do
    xterm -geometry 80x24+100+100 -e "echo 'Test window \$i'; sleep 2" &
    XTERM_PID=\$!
    sleep 2.5
    kill \$XTERM_PID 2>/dev/null || true
    sleep 0.5
  done
  
  # Now repeat same actions - should hit cache
  sleep 2
  for i in {1..5}; do
    xterm -geometry 80x24+100+100 -e "echo 'Test window \$i'; sleep 2" &
    XTERM_PID=\$!
    sleep 2.5
    kill \$XTERM_PID 2>/dev/null || true
    sleep 0.5
  done
EOF

PATTERN_PID=$!
echo "  Test pattern running (PID ${PATTERN_PID})..."
wait ${PATTERN_PID} 2>/dev/null || true

echo "  Waiting for updates to flush..."
sleep 5

# Stop viewer
echo "[6/6] Stopping viewer and collecting results..."
kill "${VIEWER_PID}" 2>/dev/null || true
wait "${VIEWER_PID}" 2>/dev/null || true

# Wait for server to finish and collect logs
wait "${SSH_PID}" 2>/dev/null || true

# Retrieve server log
echo ""
echo "Retrieving server logs..."
REMOTE_LOG=$(ssh "${REMOTE}" "ls -1t ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/*.log 2>/dev/null | head -1" || echo "")

if [[ -z "${REMOTE_LOG}" ]]; then
  echo "WARN: Could not find server log, checking manual location..."
  ssh "${REMOTE}" "cat /tmp/test_cache_server.log 2>/dev/null | tail -100" > /tmp/server_stats.log || true
  REMOTE_LOG="/tmp/server_stats.log"
else
  scp -q "${REMOTE}:${REMOTE_LOG}" /tmp/server_stats.log
  REMOTE_LOG="/tmp/server_stats.log"
fi

# Analyze results
echo ""
echo "========================================================================"
echo "RESULTS"
echo "========================================================================"
echo ""

echo "=== SERVER STATISTICS ==="
if [[ -f "${REMOTE_LOG}" ]]; then
  grep -A15 "ContentCache statistics" "${REMOTE_LOG}" | tail -20 || echo "No ContentCache stats found"
else
  echo "Server log not available"
fi

echo ""
echo "=== CLIENT STATISTICS ==="
grep -A10 "Client-side ContentCache statistics" /tmp/test_cache_viewer.log | head -15 || echo "No client stats found"

echo ""
echo "=== SUMMARY ==="

# Extract key metrics
SERVER_HITS=$(grep "References sent:" "${REMOTE_LOG}" 2>/dev/null | tail -1 | grep -o '[0-9]\+ ([0-9.]\+%)' | head -1 || echo "0")
CLIENT_HITS=$(grep "Lookups:.*Hits:" /tmp/test_cache_viewer.log 2>/dev/null | tail -1 | grep -oP 'Hits: \K[0-9]+' || echo "0")

echo "Server CachedRect references sent: ${SERVER_HITS}"
echo "Client CachedRect hits: ${CLIENT_HITS}"

if [[ "${CLIENT_HITS}" == "0" ]]; then
  echo ""
  echo "⚠️  NO CACHE HITS DETECTED"
  echo ""
  echo "Debugging info:"
  echo "  - Check server log for 'Processing N pending CachedRectInit'"
  echo "  - Check client log for 'Received CachedRect:' or 'Received CachedRectInit:'"
  echo ""
  echo "Logs saved:"
  echo "  Server: ${REMOTE_LOG}"
  echo "  Client: /tmp/test_cache_viewer.log"
  exit 1
else
  echo ""
  echo "✓ Cache is working!"
fi

echo ""
echo "Test complete!"
