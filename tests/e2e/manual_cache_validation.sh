#!/bin/bash
#
# Manual validation script to demonstrate ContentCache/PersistentCache hits
# 
# This script starts a test server and opens a viewer, then you can manually
# interact to see cache hits in the logs.
#

set -e

DISPLAY_NUM=998
PORT=6898
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$TEST_DIR/../.." && pwd)"
VIEWER="$PROJECT_ROOT/build/vncviewer/njcvncviewer"
SERVER="$PROJECT_ROOT/build/unix/xserver/hw/vnc/Xnjcvnc"

# Check if binaries exist
if [[ ! -x "$VIEWER" ]]; then
    echo "ERROR: Viewer not found: $VIEWER"
    echo "Run: make viewer"
    exit 1
fi

if [[ ! -x "$SERVER" ]]; then
    echo "Using system Xtigervnc instead"
    SERVER="Xtigervnc"
fi

echo "==================================================================="
echo "Manual Cache Validation"
echo "==================================================================="
echo ""
echo "This will start a VNC server on display :$DISPLAY_NUM (port $PORT)"
echo "and open a viewer. You can then:"
echo ""
echo "1. Open an xterm: DISPLAY=:$DISPLAY_NUM xterm &"
echo "2. Type some commands"
echo "3. Close the xterm"
echo "4. Open another xterm at the same position"
echo "5. Type the SAME commands"
echo "6. Watch the logs for cache HITs"
echo ""
echo "Press Ctrl+C to stop"
echo ""

# Clean up any existing display
if [[ -e "/tmp/.X11-unix/X$DISPLAY_NUM" ]]; then
    echo "Cleaning up existing display :$DISPLAY_NUM..."
    rm -f "/tmp/.X11-unix/X$DISPLAY_NUM"
fi

# Start server
echo "Starting VNC server on :$DISPLAY_NUM..."
SERVER_LOG="/tmp/vnc_server_$DISPLAY_NUM.log"
$SERVER :$DISPLAY_NUM \
    -rfbport $PORT \
    -SecurityTypes None \
    -geometry 1920x1080 \
    -Log '*:stderr:100' \
    > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!

# Wait for server to start
sleep 2

# Check if server is running
if ! kill -0 $SERVER_PID 2>/dev/null; then
    echo "ERROR: Server failed to start"
    cat "$SERVER_LOG"
    exit 1
fi

echo "✓ Server started (PID: $SERVER_PID)"
echo "  Log: $SERVER_LOG"

# Start window manager
echo "Starting openbox on :$DISPLAY_NUM..."
DISPLAY=:$DISPLAY_NUM openbox --sm-disable > /dev/null 2>&1 &
WM_PID=$!
sleep 1

echo "✓ Window manager started (PID: $WM_PID)"

# Start viewer
echo ""
echo "Starting viewer..."
VIEWER_LOG="/tmp/vnc_viewer_$DISPLAY_NUM.log"
$VIEWER localhost::$PORT \
    Log='*:stderr:100' \
    PersistentCache=1 \
    > "$VIEWER_LOG" 2>&1 &
VIEWER_PID=$!

echo "✓ Viewer started (PID: $VIEWER_PID)"
echo "  Log: $VIEWER_LOG"

echo ""
echo "==================================================================="
echo "READY FOR TESTING"
echo "==================================================================="
echo ""
echo "To generate cache hits:"
echo "  1. Open xterm: DISPLAY=:$DISPLAY_NUM xterm -geometry 80x24+100+100 &"
echo "  2. Type: ls -la"
echo "  3. Close xterm"
echo "  4. Open again: DISPLAY=:$DISPLAY_NUM xterm -geometry 80x24+100+100 &"
echo "  5. Type: ls -la (same command)"
echo ""
echo "Watch for cache activity:"
echo "  tail -f $SERVER_LOG | grep -i 'persistentcache\\|contentcache'"
echo ""
echo "Press Ctrl+C when done..."
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    kill $VIEWER_PID 2>/dev/null || true
    kill $WM_PID 2>/dev/null || true
    kill $SERVER_PID 2>/dev/null || true
    
    # Print cache statistics from logs
    echo ""
    echo "==================================================================="
    echo "Cache Statistics"
    echo "==================================================================="
    echo ""
    echo "Server cache activity:"
    grep -i "persistentcache hit\|persistentcache miss" "$SERVER_LOG" 2>/dev/null | head -20 || echo "  (none found)"
    
    echo ""
    echo "Logs saved to:"
    echo "  Server: $SERVER_LOG"
    echo "  Viewer: $VIEWER_LOG"
}

trap cleanup EXIT INT TERM

# Wait for user interrupt
wait $VIEWER_PID
