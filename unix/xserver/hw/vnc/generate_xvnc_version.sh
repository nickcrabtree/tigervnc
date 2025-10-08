#!/bin/sh
# Generate xvnc version header with git metadata
# Usage: generate_xvnc_version.sh <output_header> <base_version>

OUTPUT="$1"
BASE_VERSION="$2"

# Save current directory and convert OUTPUT to absolute path if relative
ORIG_DIR="$(pwd)"
case "$OUTPUT" in
    /*) OUTPUT_ABS="$OUTPUT" ;;
    *)  OUTPUT_ABS="$ORIG_DIR/$OUTPUT" ;;
esac

# Find the TigerVNC git root
# Prefer TIGERVNC_SRCDIR if set by Makefile, otherwise traverse up from script location
if [ -n "$TIGERVNC_SRCDIR" ]; then
    TIGERVNC_ROOT="$TIGERVNC_SRCDIR"
else
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    TIGERVNC_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"
fi

# If base version looks like Xorg version (e.g. 21.x.x), try to get TigerVNC version from CMakeLists.txt
case "$BASE_VERSION" in
    2[0-9].*)
        if [ -f "$TIGERVNC_ROOT/CMakeLists.txt" ]; then
            CMAKE_VERSION=$(grep -E '^set\(VERSION' "$TIGERVNC_ROOT/CMakeLists.txt" | sed -E 's/set\(VERSION ([0-9.]+)\)/\1/')
            if [ -n "$CMAKE_VERSION" ]; then
                BASE_VERSION="$CMAKE_VERSION"
            fi
        fi
        ;;
esac

# Get git information if available (from tigervnc root, not xorg server)
if cd "$TIGERVNC_ROOT" && git rev-parse --git-dir > /dev/null 2>&1; then
    # Short commit hash
    GIT_HASH=$(git rev-parse --short=7 HEAD 2>/dev/null || echo "unknown")
    
    # Count commits since initial commit
    GIT_COUNT=$(git rev-list --count HEAD 2>/dev/null || echo "0")
    
    # Check if tree is dirty
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        GIT_DIRTY="-dirty"
    else
        GIT_DIRTY=""
    fi
    
    FULL_VERSION="${BASE_VERSION}+build.${GIT_COUNT}.${GIT_HASH}${GIT_DIRTY}"
else
    FULL_VERSION="${BASE_VERSION}+build.unknown"
fi

# Generate header
cat > "$OUTPUT_ABS" << EOF
/* Auto-generated version header - do not edit */
#ifndef __XVNC_VERSION_H__
#define __XVNC_VERSION_H__

#define XVNC_VERSION "${FULL_VERSION}"
#define XVNC_BASE_VERSION "${BASE_VERSION}"
#define XVNC_GIT_HASH "${GIT_HASH:-unknown}"
#define XVNC_GIT_COUNT "${GIT_COUNT:-0}"

#endif /* __XVNC_VERSION_H__ */
EOF
