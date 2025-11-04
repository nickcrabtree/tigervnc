#!/usr/bin/env bash
set -euo pipefail

# Cross-host CachedRectInit debug orchestrator
# - Starts the server-only e2e test on quartz (Linux)
# - Waits for readiness
# - Connects the local macOS C++ viewer with verbose logging
# - Retrieves and compares logs

REMOTE="${REMOTE:-nickc@quartz.local}"
REMOTE_DIR="${REMOTE_DIR:-/home/nickc/code/tigervnc}"
MODE="${MODE:-auto}"   # auto|lan|tunnel
VIEWER_BIN="${VIEWER_BIN:-build/vncviewer/njcvncviewer}"
LOCAL_LOG_DIR="${LOCAL_LOG_DIR:-/tmp/cachedrect_debug}"
VIEWER_LOG="${LOCAL_LOG_DIR}/viewer_$(date +%Y%m%d_%H%M%S).log"
SERVER_STDOUT_REMOTE="/tmp/cachedrect_server_stdout.log"
SERVER_CTL_REMOTE="/tmp/start_cachedrect_server_only.sh"
TUNNEL_PORT=6898
SERVER_PORT=6898

mkdir -p "${LOCAL_LOG_DIR}"

usage() {
  cat >&2 <<EOF
Usage: REMOTE=user@host MODE=lan|tunnel|auto VIEWER_BIN=... ${0##*/}

Env vars:
  REMOTE        SSH target (default: nickc@quartz.local)
  REMOTE_DIR    Remote repo root (default: /home/nickc/code/tigervnc)
  MODE          auto|lan|tunnel (default: auto)
  VIEWER_BIN    Local viewer binary (default: build/vncviewer/njcvncviewer)
  LOCAL_LOG_DIR Local log dir (default: /tmp/cachedrect_debug)

Examples:
  MODE=lan ${0##*/}                   # On same LAN, connect quartz.local::6898
  MODE=tunnel ${0##*/}                # Use SSH tunnel to localhost::6898
  REMOTE=nickc@quartz.local MODE=auto ${0##*/}  # Auto-detect reachability
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage; exit 0
fi

if [[ ! -x "${VIEWER_BIN}" ]]; then
  echo "ERROR: VIEWER_BIN not found or not executable: ${VIEWER_BIN}" >&2
  exit 1
fi

echo "[1/7] Installing remote server-control script on ${REMOTE}..."
ssh -o BatchMode=yes "${REMOTE}" "bash -lc 'cat > ${SERVER_CTL_REMOTE} <<\"EOS\"
#!/usr/bin/env bash
set -euo pipefail
cd \"${REMOTE_DIR}\"

ART_DIR_BASE=\"\${PWD}/tests/e2e/_artifacts\"
mkdir -p \"\${ART_DIR_BASE}\"

# Stop any existing test servers on :998/:999 SAFELY (verify PID and display)
safe_kill_tests() {
  # List candidates; do not kill production!
  pids=(\$(ps -eo pid,args | awk \"/Xnjcvnc :99[89]/ {print \\\$1}\"))
  for pid in \"\${pids[@]:-}\"; do
    if [[ -n \"\${pid}\" ]]; then
      # Double-check it is a test display
      if ps -p \"\${pid}\" -o args= | grep -q \"Xnjcvnc :99[89]\"; then
        echo \"[remote] Killing test server PID \${pid}\"
        kill -TERM \"\${pid}\" || true
      fi
    fi
  done
}

safe_kill_tests

echo \"[remote] Starting server-only e2e test (will target :998 / port 6898)...\"
# Use our custom server-only script that keeps the server running
: > /tmp/cachedrect_server_stdout.log
( cd \"${REMOTE_DIR}\" && python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 60 ) >> /tmp/cachedrect_server_stdout.log 2>&1 &

echo \$! > /tmp/cachedrect_server_pid

# Wait for port 6898 to open
echo \"[remote] Waiting for server to listen on TCP :6898...\"
for i in \$(seq 1 60); do
  if command -v ss >/dev/null 2>&1; then
    if ss -tln | grep -q \":6898\"; then echo \"SERVER_READY\"; break; fi
  else
    if netstat -tln 2>/dev/null | grep -q \":6898\"; then echo \"SERVER_READY\"; break; fi
  fi
  sleep 1
done

# Stream server-side console messages until process exits
server_pid=\$(cat /tmp/cachedrect_server_pid 2>/dev/null || echo \"\")
if [[ -n \"\$server_pid\" ]]; then
  echo \"[remote] Server test PID: \$server_pid\"
  # Tail artifacts log if it appears
  (
    for j in \$(seq 1 120); do
      latest=\$(ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/* 2>/dev/null | head -1 || true)
      if [[ -n \"\$latest\" ]] && [[ -f \"\$latest/logs/contentcache_test.log\" ]]; then
        echo \"[remote] Tailing server log: \$latest/logs/contentcache_test.log\"
        tail -F \"\$latest/logs/contentcache_test.log\" 2>/dev/null &
        break
      fi
      sleep 1
    done
    wait || true
  ) &

  # Wait for server-only test process to exit
  while kill -0 \"\$server_pid\" 2>/dev/null; do sleep 2; done
  echo \"[remote] Server test process exited\"
fi
EOS
chmod +x ${SERVER_CTL_REMOTE}
'"

echo "[2/7] Launching server-only test on ${REMOTE}..."
ssh "${REMOTE}" "bash -lc 'nohup ${SERVER_CTL_REMOTE} >/dev/null 2>&1 & echo started'"

echo "[3/7] Waiting for remote server readiness..."
for i in {1..90}; do
  if ssh "${REMOTE}" "bash -lc 'grep -q SERVER_READY ${SERVER_STDOUT_REMOTE} 2>/dev/null || false'"; then
    echo "[ok] Remote server reports SERVER_READY"
    break
  fi
  sleep 1
  if (( i == 90 )); then
    echo "ERROR: Timed out waiting for server to become ready." >&2
    ssh "${REMOTE}" "bash -lc 'tail -n +1 ${SERVER_STDOUT_REMOTE} || true'" || true
    exit 2
  fi
done

# Decide connection mode
pick_mode() {
  if [[ "${MODE}" == "lan" || "${MODE}" == "tunnel" ]]; then
    echo "${MODE}"
  else
    # auto: try direct TCP to quartz.local:6898
    if nc -z -w 1 "${REMOTE#*@}" ${SERVER_PORT} 2>/dev/null; then
      echo "lan"
    else
      echo "tunnel"
    fi
  fi
}
MODE="$(pick_mode)"
echo "[4/7] Connection mode: ${MODE}"

TUNNEL_PID=""
TARGET_HOST=""
if [[ "${MODE}" == "tunnel" ]]; then
  echo "[4a/7] Establishing SSH tunnel localhost:${TUNNEL_PORT} -> ${REMOTE#*@}:6898..."
  ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT} "${REMOTE}"
  # Find the most recent ssh -N -L PID (best-effort)
  sleep 1
  TUNNEL_PID="$(pgrep -f "ssh -fN -L ${TUNNEL_PORT}:localhost:${SERVER_PORT}" | head -1 || true)"
  TARGET_HOST="localhost"
else
  TARGET_HOST="${REMOTE#*@}"
fi

echo "[5/7] Starting local viewer with maximum verbosity..."
echo "      Logs: ${VIEWER_LOG}"
echo "      Target: ${TARGET_HOST}::${SERVER_PORT}"
set +e
"${VIEWER_BIN}" -Log="*:stderr:100" "${TARGET_HOST}::${SERVER_PORT}" 2>&1 | tee "${VIEWER_LOG}"
VIEWER_RC=$?
set -e
echo "[viewer exit code] ${VIEWER_RC}"

echo "[6/7] Retrieving server artifacts log..."
# Find latest artifact log path on remote
REMOTE_ART_LOG=$(ssh "${REMOTE}" "bash -lc 'ls -1dt ${REMOTE_DIR}/tests/e2e/_artifacts/*/logs/contentcache_test.log 2>/dev/null | head -1 || true'")
if [[ -z "${REMOTE_ART_LOG}" ]]; then
  echo "WARN: Could not locate remote contentcache_test.log; dumping server stdout trace:"
  ssh "${REMOTE}" "bash -lc 'tail -n +1 ${SERVER_STDOUT_REMOTE} || true'" || true
else
  LOCAL_SERVER_LOG="${LOCAL_LOG_DIR}/contentcache_test_$(date +%Y%m%d_%H%M%S).log"
  scp -q "${REMOTE}:${REMOTE_ART_LOG}" "${LOCAL_SERVER_LOG}"
  echo "[ok] Downloaded server log to ${LOCAL_SERVER_LOG}"

  echo "[7/7] Comparing logs..."
  if [[ -x "scripts/compare_cachedrect_logs.py" ]]; then
    python3 scripts/compare_cachedrect_logs.py --server "${LOCAL_SERVER_LOG}" --client "${VIEWER_LOG}" || true
  else
    echo "WARN: scripts/compare_cachedrect_logs.py not found, skipping comparison"
  fi
fi

# Cleanup tunnel
if [[ -n "${TUNNEL_PID}" ]]; then
  kill "${TUNNEL_PID}" 2>/dev/null || true
fi

echo ""
echo "Done. Logs in ${LOCAL_LOG_DIR}"
echo "  Server: ${LOCAL_SERVER_LOG:-not retrieved}"
echo "  Client: ${VIEWER_LOG}"
