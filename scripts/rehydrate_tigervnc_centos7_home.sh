#!/usr/bin/env bash
set -euo pipefail

# Rehydrate script for TigerVNC on CentOS 7 using /data_parallel/PreStackPro/share/nickc as install root
# - Installs system packages via yum (requires sudo)
# - Builds and installs a modern CMake (>= 3.10) into /data_parallel/PreStackPro/share/nickc
# - Leaves the actual TigerVNC build to the user (e.g. `make viewer server`)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

PREFIX="/data_parallel/PreStackPro/share/nickc"  # All new tools go under PREFIX/{bin,lib,include,...}
CMAKE_VERSION="3.24.4"   # Any >= 3.10 is fine; 3.24.x still builds on CentOS 7

mkdir -p "$PREFIX/bin" "$PREFIX/lib" "$PREFIX/include" "$PREFIX/src"

# Helper: check if a sufficiently new cmake is already available on PATH
have_modern_cmake() {
  if ! command -v cmake >/dev/null 2>&1; then
    return 1
  fi
  local ver
  ver="$(cmake --version 2>/dev/null | head -1 | awk '{print $3}')"
  # crude numeric compare: major.minor >= 3.10
  local major minor
  major="${ver%%.*}"
  minor="${ver#*.}"; minor="${minor%%.*}"
  if [[ "$major" -gt 3 ]] || { [[ "$major" -eq 3 ]] && [[ "$minor" -ge 10 ]]; }; then
    return 0
  fi
  return 1
}

# --- 1. System packages via yum (CentOS 7) ---
# Note: these install headers and libraries system-wide; custom tools still go under $PREFIX.
printf "[rehydrate] Installing development packages via yum (requires sudo)...\n"
sudo yum -y groupinstall "Development Tools" || true
sudo yum -y install \
  git wget curl \
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
  perl-File-ReadBackwards || true

# --- 2. Modern CMake under $PREFIX ---
if have_modern_cmake; then
  echo "[rehydrate] Existing cmake is new enough; skipping local build."
else
  echo "[rehydrate] Building CMake $CMAKE_VERSION under $PREFIX..."
  cd "$PREFIX/src"
  TARBALL="cmake-$CMAKE_VERSION.tar.gz"
  URL="https://github.com/Kitware/CMake/releases/download/v$CMAKE_VERSION/$TARBALL"

  if [[ ! -f "$TARBALL" ]]; then
    echo "[rehydrate] Downloading $URL ..."
    curl -L "$URL" -o "$TARBALL"
  fi

  rm -rf "cmake-$CMAKE_VERSION"
  tar -xf "$TARBALL"
  cd "cmake-$CMAKE_VERSION"

  ./bootstrap --prefix="$PREFIX" --parallel="$(nproc 2>/dev/null || echo 1)"
  make -j"$(nproc 2>/dev/null || echo 1)"
  make install

  echo "[rehydrate] Installed CMake $CMAKE_VERSION into $PREFIX/bin/cmake"
fi

# --- 3. Environment hints ---
echo
printf "[rehydrate] Done. To use the locally installed tools under %s, ensure these are in your shell init (e.g. ~/.bashrc):\n" "$PREFIX"
printf "  export PATH=\"$PREFIX/bin:\$PATH\"\n"
echo

# Optional: show cmake version we will now use
if command -v cmake >/dev/null 2>&1; then
  echo "[rehydrate] cmake on PATH is:" && cmake --version | head -1
else
  echo "[rehydrate] WARNING: cmake is still not on PATH; check your PATH settings."
fi

cd "$ROOT_DIR"

# --- 4. Install default xstartup matching current :0 KDE session ---
CONFIG_DIR="$HOME/.config/tigervnc"
mkdir -p "$CONFIG_DIR"
install -m 0755 "$ROOT_DIR/unix/vncserver/xstartup.centos7" "$CONFIG_DIR/xstartup"

echo "[rehydrate] Installed xstartup to $CONFIG_DIR/xstartup (KDE/startkde-based session)"

echo "[rehydrate] You can now run, for example:"
echo "  cmake -S . -B build -DBUILD_VIEWER=ON"
echo "  make viewer server"

# Create symlinks for tigervnc binaries
BUILD_DIR="/data_parallel/PreStackPro/share/nickc/tigervnc/build/unix"
ln -sf ../vncpasswd/vncpasswd "$BUILD_DIR/vncserver/tigervncpasswd" 2>/dev/null
ln -sf ../vncconfig/vncconfig "$BUILD_DIR/vncserver/vncconfig" 2>/dev/null
echo "[rehydrate] Created symlinks for tigervnc binaries"
