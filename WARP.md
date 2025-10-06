# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Build Commands

TigerVNC uses CMake for building. Out-of-tree builds are recommended.

### Initial Configuration

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

### Building

```bash
# Build all targets
cmake --build build

# Build with parallel jobs
cmake --build build -- -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)

# Build specific target
cmake --build build --target vncviewer
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
  - Server-side components (VNCServerST, VNCSConnectionST)
  - Pixel formats, cursors, update tracking

**Dependency chain**: `rfb` depends on `core`, `rdr`, and `network`. `network` depends on `rdr`.

### Platform-Specific Components

- **vncviewer/**: Cross-platform VNC viewer (FLTK-based GUI)
  - Platform-specific keyboard handling (KeyboardMacOS, KeyboardWin32, KeyboardX11)
  - Platform-specific surface rendering (Surface_OSX, Surface_Win32, Surface_X11)
  - Touch handling (Win32TouchHandler, XInputTouchHandler)
  - Connection management (CConn), desktop window (DesktopWindow), viewport (Viewport)

- **unix/**: Unix/Linux server components
  - `x0vncserver/`: Polls existing X11 display and serves it via VNC
  - `vncpasswd/`: Password management utility
  - `vncconfig/`: Configuration tool for running Xvnc
  - `vncserver/`: Service wrapper scripts
  - `w0vncserver/`: Wayland display server (requires Wayland libraries)
  - `xserver/`: Contains patches for integrating with Xorg source to build Xvnc

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
- FLTK 1.3.3+ (for vncviewer)
- GnuTLS 3.x (for TLS support)
- Nettle 3.0+ (for RSA-AES)
- PAM (Unix/Linux, for authentication)
- Xorg development libraries (Unix/Linux, for viewer and x0vncserver)
- ffmpeg/libav (for H.264 on Unix/Linux)
- GoogleTest (for unit tests)

### Cross-Compilation Notes

- See `BUILDING.txt` for detailed MinGW cross-compilation recipes (Cygwin, Windows native, Linux host)
- When building Xvnc, you must patch Xorg server source (patches in `unix/xserver*.patch`)

### Compiler Flags

- C: `-std=gnu99`, C++: `-std=gnu++11`
- Strict warnings enabled: `-Wall -Wextra -Wformat=2 -Wvla`
- Debug builds treated as errors: `-Werror` in debug mode
- Assertions active in all build types (release builds have `-UNDEBUG`)
