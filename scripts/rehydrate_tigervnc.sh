#!/usr/bin/env bash
set -euo pipefail

# Rebuild custom TigerVNC Xnjcvnc server on this CentOS host.
# Assumes repository root is this script's parent directory.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

# --- Prerequisite packages  ---
# Example (CentOS/RHEL):
sudo yum -y install \
     cmake3 gcc gcc-c++ make \
     automake autoconf libtool gettext gettext-devel gettext-autopoint \
     libxkbfile-devel openssl-devel libpciaccess-devel \
     freetype-devel libjpeg-turbo-devel pam-devel \
     gnutls-devel nettle-devel gmp-devel zlib-devel \
     libuuid-devel glib2-devel \
     libX11-devel libXext-devel libXi-devel libXfixes-devel \
     libXdamage-devel libXrandr-devel libXt-devel libXdmcp-devel \
     libXinerama-devel mesa-libGL-devel libxshmfence-devel \
     pixman-devel libdrm-devel mesa-libgbm-devel \
     xorg-x11-util-macros xorg-x11-xtrans-devel libXtst-devel \
     xorg-x11-font-utils libXfont2-devel \
     libselinux-devel selinux-policy-devel systemd-devel \
     fltk-devel xorg-x11-server-devel xorg-x11-server-source \
     perl-File-ReadBackwards

# --- 1. Clean and configure CMake build tree ---
# Patch CMakeLists to drop -Wsuggest-override (not supported by GCC 4.8 on this host)
sed -i '/-Wsuggest-override/d' "${ROOT_DIR}/CMakeLists.txt"

rm -rf "${ROOT_DIR}/build"
mkdir -p "${ROOT_DIR}/build"
cd "${ROOT_DIR}/build"
cmake3 .. -DBUILD_VIEWER=0

# --- 2. One-time Xorg Xserver source setup for Xnjcvnc ---
# NOTE: requires xorg-x11-server-source installed under /usr/share/xorg-x11-server-source
cd "${ROOT_DIR}"
mkdir -p "${ROOT_DIR}/build/unix"

if [ ! -d "${ROOT_DIR}/build/unix/xserver" ]; then
  cp -R "${ROOT_DIR}/unix/xserver" "${ROOT_DIR}/build/unix/"
fi

# Overlay system Xorg source tree
cp -R /usr/share/xorg-x11-server-source/* "${ROOT_DIR}/build/unix/xserver/"

cd "${ROOT_DIR}/build/unix/xserver"

# Apply TigerVNC Xorg patch matching Xorg 1.20.x
patch -p1 < "${ROOT_DIR}/unix/xserver120.patch"

# Regenerate autotools build system
autoreconf -fiv

# Configure Xorg for use as Xnjcvnc backend (paths tuned for CentOS/RHEL-like systems)
./configure --with-pic --without-dtrace --disable-static --disable-dri \
  --disable-xinerama --disable-xvfb --disable-xnest --disable-xorg \
  --disable-dmx --disable-xwin --disable-xephyr --disable-kdrive \
  --disable-config-hal --disable-config-udev --disable-dri2 --enable-glx \
  --with-default-font-path="catalogue:/etc/X11/fontpath.d,built-ins" \
  --with-xkb-path=/usr/share/X11/xkb \
  --with-xkb-output=/var/lib/xkb \
  --with-xkb-bin-directory=/usr/bin \
  --with-serverconfig-path=/usr/lib64/xorg

cd "${ROOT_DIR}"

# Symlink used by wrapper scripts and tests
mkdir -p "${ROOT_DIR}/build/unix/vncserver"
ln -sf ../xserver/hw/vnc/Xnjcvnc "${ROOT_DIR}/build/unix/vncserver/Xnjcvnc"

# --- 3. Build TigerVNC core libraries + Xnjcvnc server ---
JOBS="$(nproc 2>/dev/null || echo 1)"
export CMAKE_BUILD_PARALLEL_LEVEL="${JOBS}"

cd "${ROOT_DIR}"
make -j"${JOBS}" server

# --- 4. Install xstartup for njcvncserver_start.bash ---
CONFIG_DIR="${HOME}/.config/tigervnc"
install -D -m 0755 "${ROOT_DIR}/unix/vncserver/xstartup.centos7" "${CONFIG_DIR}/xstartup"

