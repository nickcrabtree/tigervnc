#!/bin/bash
# Rebuild TigerVNC server (Xvnc) after library changes
# This script ensures the server picks up changes in librfb, librdr, etc.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"

echo "Rebuilding TigerVNC libraries..."
cd "$BUILD_DIR"
cmake --build . --target rfb rdr network core -j$(nproc)

echo "Rebuilding Xvnc server..."
cd "$BUILD_DIR/unix/xserver"
make -j$(nproc)

echo
echo "Xvnc rebuilt successfully!"
ls -lh "$BUILD_DIR/unix/xserver/xorg-server/hw/vnc/Xvnc"
