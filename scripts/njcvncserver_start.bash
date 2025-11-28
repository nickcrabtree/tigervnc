#!/bin/bash
export SHELL=/bin/bash

# TigerVNC build directories
TIGERVNC_ROOT="/data_parallel/PreStackPro/share/nickc/tigervnc"
TIGERVNC_BUILD="${TIGERVNC_ROOT}/build/unix/vncserver"

# Uses the built tigervncserver wrapper with Ubuntu's auto-display-selection
# Add built Perl modules to Perl include path
export PERL5LIB="${TIGERVNC_ROOT}/unix/vncserver:${PERL5LIB}"

# Ensure the wrapper finds our built Xnjcvnc binary (not the system one)
# The wrapper looks in its own directory first (binbase), then PATH
# We put our build directory FIRST in PATH to override system binaries
export PATH="${TIGERVNC_BUILD}:${PATH}"

# GPU Acceleration: Let GLVND auto-discover the appropriate GL vendor
# (This matches system tigervncserver behavior - typically uses software rendering)
export LIBGL_ALWAYS_INDIRECT=0

# Verify the Xnjcvnc binary exists before starting
if [[ ! -x "${TIGERVNC_BUILD}/Xnjcvnc" ]]; then
    echo "Error: Xnjcvnc binary not found or not executable at ${TIGERVNC_BUILD}/Xnjcvnc"
    echo "Please rebuild the server with: cd ${TIGERVNC_ROOT} && make server"
    exit 1
fi

# Reuse the existing VNC password file used by the running X0vncserver
# X0vncserver is started with: --PasswordFile=${HOME}/.vnc/passwd
VNC_PASSWD_FILE="${HOME}/.vnc/passwd"

if [[ ! -f "${VNC_PASSWD_FILE}" ]]; then
    echo "Error: expected VNC password file ${VNC_PASSWD_FILE} (used by X0vncserver) not found."
    echo "Start X0vncserver or create a VNC password with tigervncpasswd, then rerun this script."
    exit 1
fi

# The Xnjcvnc binary was built from TigerVNC 1.15.80 with GLX and contentcache support enabled.
# Verify GPU acceleration with: DISPLAY=:N glxinfo | grep "direct rendering"
# (Display auto-selection will pick :2 since :1 is used by systemd vncserver@:1.service)

# Auto-select next available display (usually :2 since :1 is systemd-managed)
# One screen macbook pro: -geometry 1920x1200
# One 4K screen:
"${TIGERVNC_BUILD}/tigervncserver" -geometry 3840x2100 -localhost 0 -xstartup "${HOME}/.config/tigervnc/xstartup" -PasswordFile="${VNC_PASSWD_FILE}"
# one big screen: -geometry 2560x1600
