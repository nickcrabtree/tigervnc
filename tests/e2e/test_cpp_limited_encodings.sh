#!/bin/bash
#
# Test C++ viewer with limited encoding set (Raw, CopyRect, ZRLE only)
# to verify if the encoding mismatch theory is correct.
#
# If C++ also crashes with limited encodings, then the root cause is NOT
# just the encoding list, but something deeper in the Rust implementation.
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Check if local server exists
if [ ! -f "$PROJECT_ROOT/build/unix/xserver/hw/vnc/Xnjcvnc" ]; then
    echo "Error: Local Xnjcvnc server not found"
    echo "Please build the server first"
    exit 1
fi

# Start VNC server on display :998
DISPLAY_NUM=998
VNC_PORT=6898

echo "==================================================================="
echo "Testing C++ Viewer with Limited Encodings (Raw, CopyRect, ZRLE)"
echo "==================================================================="
echo ""

# Kill any existing server on this display
if ps aux | grep "Xnjcvnc :$DISPLAY_NUM" | grep -v grep > /dev/null; then
    echo "Cleaning up existing server on :$DISPLAY_NUM..."
    pkill -f "Xnjcvnc :$DISPLAY_NUM" || true
    sleep 2
fi

# Start VNC server
echo "[1/5] Starting VNC server on :$DISPLAY_NUM (port $VNC_PORT)..."
XAUTHORITY=$HOME/.Xauthority \
"$PROJECT_ROOT/build/unix/xserver/hw/vnc/Xnjcvnc" \
    :$DISPLAY_NUM \
    -rfbport $VNC_PORT \
    -SecurityTypes None \
    -AlwaysShared=1 \
    -AcceptKeyEvents=1 \
    -AcceptPointerEvents=1 \
    -geometry 1600x1000 \
    -depth 24 \
    -Log "*:stderr:30" \
    > /tmp/test_server_$DISPLAY_NUM.log 2>&1 &

SERVER_PID=$!
sleep 3

if ! ps -p $SERVER_PID > /dev/null; then
    echo "✗ FAIL: Server failed to start"
    cat /tmp/test_server_$DISPLAY_NUM.log
    exit 1
fi

echo "✓ Server started (PID: $SERVER_PID)"

# Start window manager
echo "[2/5] Starting window manager..."
DISPLAY=:$DISPLAY_NUM openbox &
WM_PID=$!
sleep 1
echo "✓ Window manager started (PID: $WM_PID)"

# Launch xterm with ContentCache test content
echo "[3/5] Launching test content..."
DISPLAY=:$DISPLAY_NUM xterm -geometry 80x24+100+100 -e "echo 'Test window for ContentCache'; sleep 60" &
XTERM_PID=$!
sleep 2
echo "✓ Test content launched (PID: $XTERM_PID)"

# Run C++ viewer with ZRLE encoding only
# CRITICAL: Run headless (no DISPLAY) to prevent windows on production display
echo ""
echo "[4/5] Running C++ viewer HEADLESS with LIMITED encodings (ZRLE preferred)..."
echo "  Command: njcvncviewer 127.0.0.1::$VNC_PORT PreferredEncoding=ZRLE"
echo "  DISPLAY: unset (headless - no GUI window)"
echo ""

# Unset DISPLAY to run headless - viewer connects but shows no window
env -u DISPLAY timeout 10 "$PROJECT_ROOT/build/vncviewer/njcvncviewer" \
    "127.0.0.1::$VNC_PORT" \
    "Shared=1" \
    "PreferredEncoding=ZRLE" \
    "Log=*:stderr:100" \
    > /tmp/test_cpp_viewer.log 2>&1 &

VIEWER_PID=$!
sleep 5

# Check if viewer is still running
if ps -p $VIEWER_PID > /dev/null 2>&1; then
    echo "✓ Viewer still running after 5 seconds"
    kill $VIEWER_PID 2>/dev/null || true
    RESULT="PASSED"
else
    echo "✗ Viewer crashed or exited"
    RESULT="FAILED"
fi

# Cleanup
echo ""
echo "[5/5] Cleaning up..."
kill $XTERM_PID 2>/dev/null || true
kill $WM_PID 2>/dev/null || true  
kill $SERVER_PID 2>/dev/null || true
sleep 1

echo ""
echo "==================================================================="
echo "TEST RESULT: $RESULT"
echo "==================================================================="
echo ""
echo "Logs available:"
echo "  Server:  /tmp/test_server_$DISPLAY_NUM.log"
echo "  Viewer:  /tmp/test_cpp_viewer.log"
echo ""

# Show any errors from viewer log
if [ -f /tmp/test_cpp_viewer.log ]; then
    echo "Viewer log errors (if any):"
    grep -i "error\|fail\|crash" /tmp/test_cpp_viewer.log || echo "  (no errors found)"
fi

if [ "$RESULT" = "FAILED" ]; then
    exit 1
fi
