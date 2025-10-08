# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Build Commands

TigerVNC uses CMake for configuration with a convenience Makefile for building.

### Quick Start

After initial CMake configuration (see below), use these simple commands:

```bash
# Build everything (viewer + server)
make

# Build viewer only
make viewer

# Build server (Xnjcvnc) only
make server
```

### Initial Configuration

**First time only** - configure the build directory with CMake:

```bash
# Basic configuration (out-of-tree build)
cmake -S . -B build

# With common options
cmake -S . -B build \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DENABLE_NLS=ON \
  -DENABLE_GNUTLS=ON \
  -DENABLE_NETTLE=ON \
  -DENABLE_H264=ON \
  -DBUILD_VIEWER=ON

# Debug build
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug
```

### Xserver Setup (Required for Server Build)

**IMPORTANT**: Building the Xnjcvnc server requires a one-time setup of the Xorg server source. This is separate from the CMake configuration.

#### Ubuntu/Debian Setup

```bash
# 1. Install Xorg server source (if not already installed)
sudo apt-get install xorg-server-source

# 2. Set up xserver build directory
mkdir -p build/unix
cp -R unix/xserver build/unix/

# 3. Extract Xorg source into build directory
cd build/unix/xserver
tar xf /usr/src/xorg-server.tar.xz --strip-components=1

# 4. Apply TigerVNC patches (use appropriate patch for your Xorg version)
# For Xorg 21.x:
patch -p1 < ../../../unix/xserver21.patch
# For Xorg 1.20.x:
# patch -p1 < ../../../unix/xserver120.patch

# 5. Run autotools
autoreconf -fiv

# 6. Configure xserver (adjust paths for your system)
./configure --with-pic --without-dtrace --disable-static --disable-dri \
  --disable-xinerama --disable-xvfb --disable-xnest --disable-xorg \
  --disable-dmx --disable-xwin --disable-xephyr --disable-kdrive \
  --disable-config-hal --disable-config-udev --disable-dri2 --enable-glx \
  --with-default-font-path="catalogue:/etc/X11/fontpath.d,built-ins" \
  --with-xkb-path=/usr/share/X11/xkb \
  --with-xkb-output=/var/lib/xkb \
  --with-xkb-bin-directory=/usr/bin \
  --with-serverconfig-path=/usr/lib/xorg

# 7. Return to project root
cd ../../..

# 8. Create symlink for wrapper compatibility
mkdir -p build/unix/vncserver
ln -sf ../xserver/hw/vnc/Xnjcvnc build/unix/vncserver/Xnjcvnc
```

**After this one-time setup**, the `make server` command will work correctly.

#### Other Distributions

- **RHEL/Fedora/CentOS**: Install `xorg-x11-server-source` package, source typically in `/usr/share/xorg-x11-server-source`
- **Arch**: Install `xorg-server` source package
- Adjust configure paths (especially `--with-serverconfig-path`) for your distribution

#### Verifying Xserver Setup

```bash
# Check if xserver is configured
ls -la build/unix/xserver/config.status

# Check if Makefile exists
ls -la build/unix/xserver/hw/vnc/Makefile
```

### Building

The top-level `Makefile` provides convenience targets:

```bash
# Build both viewer and server (default)
make

# Build viewer only (njcvncviewer)
make viewer

# Build server only (Xnjcvnc)
make server
```

**How it works:**
- `make viewer`: Builds njcvncviewer via CMake, automatically rebuilding dependent libraries as needed
- `make server`: Builds CMake libraries (rfb, rdr, network, core, unixcommon) then invokes the Xorg autotools build
- `make` (or `make all`): Builds both viewer and server

**Prerequisites:**
- Viewer: CMake configuration only
- Server: CMake configuration + one-time xserver setup (see "Xserver Setup" above)

**Build timestamps:**
- Viewer: Timestamp is generated at build time in `build/vncviewer/build_timestamp.h`
- Server: Timestamp uses `__DATE__` and `__TIME__` macros in `buildtime.c`
- Both are updated automatically on each build

### Build System Architecture

TigerVNC has **two separate build systems** working together:

1. **CMake** (for common libraries, njcvncviewer, utilities)
2. **Autotools** (for Xnjcvnc server - integrates with Xorg source)

The top-level Makefile coordinates both systems:
- CMake builds: `build/vncviewer/njcvncviewer`
- Xorg builds: `build/unix/xserver/hw/vnc/Xnjcvnc`

**Critical**: The server build passes `TIGERVNC_SRCDIR` and `TIGERVNC_BUILDDIR` to the xserver Makefile, which are required for finding headers and libraries.

### Advanced: Direct CMake/Make Commands

If you need more control, you can invoke the build systems directly:

```bash
# Build viewer with CMake directly
cmake --build build --target njcvncviewer

# Build server manually (requires xserver setup first)
cmake --build build --target rfb rdr network core unixcommon
make -C build/unix/xserver TIGERVNC_SRCDIR=$(pwd) TIGERVNC_BUILDDIR=$(pwd)/build

# Build everything via CMake (libraries + viewer, but not Xnjcvnc server)
cmake --build build
```

### Binary Locations

After building, executables are located at:

```bash
# Viewer
build/vncviewer/njcvncviewer

# Server (actual binary)
build/unix/xserver/hw/vnc/Xnjcvnc

# Server (symlink for wrapper compatibility)
build/unix/vncserver/Xnjcvnc -> build/unix/xserver/hw/vnc/Xnjcvnc
```

**Important**: Don't confuse your build with system binaries:

```bash
# Your build (what you want to test)
build/unix/xserver/hw/vnc/Xnjcvnc

# System binary (not your build!)
/usr/bin/Xnjcvnc
/usr/bin/Xtigervnc  # Old name, may still be installed
```

**Verify which binary is running**:
```bash
# Check running server
ps aux | grep Xnjcvnc

# Verify it's your build
ls -lh build/unix/xserver/hw/vnc/Xnjcvnc

# Check symlink target
readlink -f build/unix/vncserver/Xnjcvnc
```

### Running Tests

```bash
# Run all unit tests (requires GTest)
ctest --test-dir build/tests/unit/ --output-on-failure

# Run all tests with parallel execution
ctest --test-dir build --output-on-failure -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)

# Run specific test by name
ctest --test-dir build -R <test_pattern> -V

# Examples of individual unit tests
ctest --test-dir build -R pixelformat -V
ctest --test-dir build -R hostport -V
```

### Performance Benchmarks

The `tests/perf/` directory contains performance benchmarking tools:

```bash
# After building, run performance tests manually
./build/tests/perf/encperf   # Encoding performance
./build/tests/perf/decperf   # Decoding performance
./build/tests/perf/convperf  # Conversion performance
./build/tests/perf/fbperf    # Framebuffer performance (requires BUILD_VIEWER=ON)
```

### Creating Release Packages

```bash
# Linux/Unix: Binary tarball
make -C build tarball
# Creates: build/tigervnc-<system>-<arch>-<version>.tar.gz

# macOS: DMG disk image
make -C build dmg
# Creates: build/release/TigerVNC-<version>.dmg

# Windows: Installer (requires Inno Setup)
make -C build installer
# Creates: build/release/tigervnc<suffix>-<version>.exe
```

## Code Architecture

TigerVNC is organized as a set of layered libraries with platform-specific components.

### Core Libraries (common/)

These are static libraries that provide the foundation for both viewers and servers:

- **core/**: Cross-platform utilities (logging, configuration, exceptions, timers, string handling, region management via pixman)
- **rdr/**: Reader/Writer abstraction for serialization and stream I/O (buffered streams, file streams, TLS streams, zlib compression streams)
- **network/**: Socket abstractions (TCP sockets, Unix sockets, TLS sockets)
- **rfb/**: RFB protocol implementation
  - Connection handling (CConnection for client, SConnection for server)
  - Message readers/writers (CMsgReader/Writer, SMsgReader/Writer)
  - Security types (Plain, VncAuth, VeNCrypt, TLS, RSA-AES, DH)
  - Encoders/Decoders (Raw, CopyRect, RRE, Hextile, Tight, ZRLE, H.264)
  - **ContentCache**: Content-addressable historical cache with ARC algorithm (see `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`)
  - Server-side components (VNCServerST, VNCSConnectionST, EncodeManager)
  - Pixel formats, cursors, update tracking

**Dependency chain**: `rfb` depends on `core`, `rdr`, and `network`. `network` depends on `rdr`.

### Platform-Specific Components

- **vncviewer/**: Cross-platform VNC viewer (FLTK-based GUI, produces njcvncviewer)
  - Platform-specific keyboard handling (KeyboardMacOS, KeyboardWin32, KeyboardX11)
  - Platform-specific surface rendering (Surface_OSX, Surface_Win32, Surface_X11)
  - Touch handling (Win32TouchHandler, XInputTouchHandler)
  - Connection management (CConn), desktop window (DesktopWindow), viewport (Viewport)

- **unix/**: Unix/Linux server components
  - `x0vncserver/`: Polls existing X11 display and serves it via VNC
  - `vncpasswd/`: Password management utility
  - `vncconfig/`: Configuration tool for running Xnjcvnc
  - `vncserver/`: Service wrapper scripts
  - `w0vncserver/`: Wayland display server (requires Wayland libraries)
  - `xserver/`: Contains patches for integrating with Xorg source to build Xnjcvnc

- **win/**: Windows server components
  - `winvnc/`: Windows VNC server (NOTE: currently unmaintained)
  - `vncconfig/`: Windows configuration GUI
  - `wm_hooks/`: Windows message hooks

- **java/**: Java-based VNC viewer (optional, requires `-DBUILD_JAVA=ON`)

### Testing Structure

- **tests/unit/**: GoogleTest-based unit tests for individual components
  - Tests for pixel formats, host/port parsing, parameters, encodings, gesture handling, etc.
  - Invoked via `ctest` after building with GTest available

- **tests/perf/**: Performance benchmarking executables
  - Manual execution for measuring encoding/decoding/framebuffer performance

## Key Conventions

### Build System

- CMake 3.10+ required
- Out-of-tree builds recommended (use `-B build` or create separate build directory)
- Default build type is `Release` (with assertions kept via `-UNDEBUG`)
- Use `-DCMAKE_BUILD_TYPE=Debug` for debug builds with `-Og` and `-Werror`

### Platform Support

- **Linux/Unix**: Native builds with system X11 libraries
- **macOS**: Native builds, requires FLTK, produces .dmg
- **Windows**: MinGW/MinGW-w64 builds (MSVC not supported), requires FLTK, produces .exe installer

### Optional Features

CMake options control optional functionality:

- `ENABLE_NLS`: Native language support via gettext (default: AUTO)
- `ENABLE_GNUTLS`: TLS encryption via GnuTLS (default: AUTO)
- `ENABLE_NETTLE`: RSA-AES security types via Nettle (default: AUTO)
- `ENABLE_H264`: H.264 encoding support (default: AUTO, uses ffmpeg on Unix/macOS, Media Foundation on Windows)
- `BUILD_VIEWER`: Build the vncviewer client (default: AUTO, requires FLTK)
- `BUILD_JAVA`: Build Java viewer (default: OFF)
- `ENABLE_ASAN`: Address sanitizer support (default: OFF)
- `ENABLE_TSAN`: Thread sanitizer support (default: OFF)

Use `AUTO` to build with the feature if dependencies are found, or `ON`/`OFF` to require or disable it.

### Dependencies

**Required**:
- zlib
- pixman
- libjpeg (libjpeg-turbo strongly recommended for performance)

**Optional** (for full features):
- FLTK 1.3.3+ (for njcvncviewer)
- GnuTLS 3.x (for TLS support)
- Nettle 3.0+ (for RSA-AES)
- PAM (Unix/Linux, for authentication)
- Xorg development libraries (Unix/Linux, for viewer and x0vncserver)
- ffmpeg/libav (for H.264 on Unix/Linux)
- GoogleTest (for unit tests)

### Cross-Compilation Notes

- See `BUILDING.txt` for detailed MinGW cross-compilation recipes (Cygwin, Windows native, Linux host)
- When building Xnjcvnc, you must patch Xorg server source (patches in `unix/xserver*.patch`)

### Compiler Flags

- C: `-std=gnu99`, C++: `-std=gnu++11`
- Strict warnings enabled: `-Wall -Wextra -Wformat=2 -Wvla`
- Debug builds treated as errors: `-Werror` in debug mode
- Assertions active in all build types (release builds have `-UNDEBUG`)

## Important Code Patterns and Gotchas

### Stride is in Pixels, Not Bytes!

**Critical**: `PixelBuffer::getBuffer()` returns stride in **pixels**, not bytes.

```cpp
const uint8_t* buffer;
int stride;
buffer = pb->getBuffer(rect, &stride);

// WRONG - only covers partial data
size_t byteLen = rect.height() * stride;

// CORRECT - multiply by bytesPerPixel
size_t bytesPerPixel = pb->getPF().bpp / 8;
size_t byteLen = rect.height() * stride * bytesPerPixel;
```

**Why this matters**: This caused a critical bug (Oct 7 2025) in ContentCache hash calculation that resulted in frequent hash collisions and severe visual corruption. Always multiply stride by `bytesPerPixel` when calculating byte lengths.

### Process Management Safety

**Critical**: When killing processes, always verify working directory and exact PID to avoid killing unrelated instances.

```bash
# Find candidate processes
ps aux | grep "pattern" | grep -v grep

# Verify working directory of each PID
pwdx <PID>

# Only kill the specific verified PID
kill -TERM <PID>

# NEVER use pattern-based killing like:
# pkill -f script_name.py  # FORBIDDEN - kills ALL matching processes!
```

**Why this matters**: Multiple VNC servers, experiments, or scripts may run simultaneously on the same machine. Pattern-based killing can destroy long-running jobs, corrupt data, or interrupt production services.

### PixelBuffer Access Patterns

```cpp
// Get read-only buffer access
const uint8_t* buffer = pb->getBuffer(rect, &stride);
// Use buffer, no commit needed

// Get read-write buffer access
uint8_t* buffer = pb->getBufferRW(rect, &stride);
// Modify buffer...
pb->commitBufferRW(rect);  // Must call when done!
```

Stride value determines how to traverse rows:
```cpp
for (int y = 0; y < height; y++) {
    const uint8_t* row = buffer + (y * stride * bytesPerPixel);
    for (int x = 0; x < width; x++) {
        const uint8_t* pixel = row + (x * bytesPerPixel);
        // Process pixel...
    }
}
```

## ContentCache Implementation

This fork includes a custom **ContentCache** system that provides 97-99% bandwidth reduction for repeated content by maintaining a content-addressable historical cache.

### Key Files

- `common/rfb/ContentCache.h/cxx`: Core cache with ARC (Adaptive Replacement Cache) algorithm
- `common/rfb/EncodeManager.cxx`: Server-side integration (cache lookups, insertions)
- `common/rfb/DecodeManager.cxx`: Client-side integration (cache retrieval, blitting)
- `common/rfb/encodings.h`: Protocol constants (CachedRect, CachedRectInit, capability negotiation)

### Protocol Overview

**CachedRect** (20 bytes): Server references cached content by ID, client blits from cache
**CachedRectInit** (20 bytes + encoding): Server sends full encoding + cache ID, client stores after decoding

### Configuration

Server parameters (add to `~/.vnc/config`):
```bash
ContentCache=1              # Enable (default: true)
ContentCacheSize=2048       # Cache size in MB (default: 2048)
ContentCacheMaxAge=300      # Max age in seconds (default: 300)
ContentCacheMinRectSize=4096  # Min pixels to cache (default: 4096)
```

### Performance

- **Bandwidth**: 97-99% reduction for cache hits (20 bytes vs KB of compressed data)
- **CPU**: Zero decode cost for hits (memory blit vs decompression)
- **Memory**: ~16KB per cached 64×64 tile

### Debugging

```bash
# Enable verbose logging
Xnjcvnc :2 -Log *:stderr:100

# Monitor cache statistics (logged hourly)
tail -f ~/.vnc/quartz:2.log | grep -i contentcache

# Example output:
# ContentCache: Hit rate: 23.5% (1234 hits, 4032 misses)
# ContentCache: Memory: 156MB / 2048MB (7.6% used), 10245 entries
# ContentCache: ARC balance: T1=8245 (80.5%), T2=2000 (19.5%)
```

### Documentation

See `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` for comprehensive design, implementation details, build system notes, known issues, and troubleshooting guide.

## Related Documentation

- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: Comprehensive ContentCache guide
- `ARC_ALGORITHM.md`: Adaptive Replacement Cache algorithm details
- `CONTENTCACHE_CLIENT_INTEGRATION.md`: Client-side integration summary
- `BUILD_CONTENTCACHE.md`: Build instructions specific to ContentCache
- `README.rst`: General TigerVNC documentation
- `BUILDING.txt`: Detailed build instructions for all platforms
