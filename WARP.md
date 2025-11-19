# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

The bug database for this repository is maintained in `BUGS.md` in the project root.

## ⚠️ CRITICAL: ALWAYS USE TIMEOUTS

**🔴 MANDATORY: All commands MUST use timeouts 🔴**

When running commands that might hang or wait for user input, **ALWAYS** use `timeout`:

```bash
# ✅ CORRECT: Use timeout for all commands that might hang
timeout 60 ssh user@host 'command'
timeout 120 ./script.sh
timeout 30 make test

# ❌ WRONG: Never run potentially blocking commands without timeout
ssh user@host 'command'  # Can hang indefinitely
./script.sh              # May wait for user input
make test                # Tests may hang
```

**Why this is critical:**
- Commands can hang waiting for user input
- Network operations can stall indefinitely  
- GUI applications block until closed
- Without timeouts, the AI agent becomes unresponsive

**Default timeout values:**
- Quick commands (ls, grep, etc.): 10 seconds
- SSH operations: 30 seconds
- Build operations: 300 seconds (5 minutes)
- Test runs: 120-300 seconds depending on test
- Interactive scripts with GUI: 60-120 seconds

**Exception:** Only skip timeout for commands that are known to be instant (e.g., `echo`, variable assignment).

## ⚠️ CRITICAL SAFETY WARNINGS

### Production Servers and Viewers

**🔴 ABSOLUTELY FORBIDDEN: pkill, killall, pkill -f, killall -f 🔴**

**NEVER, UNDER ANY CIRCUMSTANCES, use pkill or killall commands!**

These commands will kill ALL matching processes across the ENTIRE system, including:
- Production VNC servers on Linux (quartz)
- Production VNC viewers on macOS (user's desktop)
- Any other user processes that match the pattern

**This system has production processes that must NEVER be killed:**

On Linux (quartz):
- `Xtigervnc :1` (port 5901) - PRODUCTION SERVER
- `Xnjcvnc :2` (port 5902) - PRODUCTION SERVER
- `Xtigervnc :3` (port 5903) - PRODUCTION SERVER

On macOS (user's desktop):
- `njcvncviewer` - User's active VNC viewer sessions
- `Xvfb` - May be used for legitimate purposes

**MANDATORY Rules for safe process management:**

1. **🚫 ABSOLUTELY FORBIDDEN COMMANDS:**
   ```bash
   pkill <anything>        # ❌ NEVER - kills ALL matching processes
   pkill -f <anything>     # ❌ NEVER - kills ALL matching patterns  
   killall <anything>      # ❌ NEVER - kills ALL matching processes
   pkill -9 <anything>     # ❌ NEVER - force kills ALL matching
   ```
   **These commands are COMPLETELY BANNED. Do not use them for ANY reason.**

2. **✅ ONLY ACCEPTABLE METHOD - Kill by specific verified PID:**
   ```bash
   # Step 1: Find candidate processes
   ps aux | grep "Xnjcvnc :99[89]"
   
   # Step 2: Verify EACH PID individually
   ps -p <PID> -o pid,args=    # Check full command
   pwdx <PID>                  # Verify working directory
   
   # Step 3: Only after manual verification, kill specific PID
   kill <specific-verified-pid>
   
   # If it doesn't stop, use SIGKILL on that SPECIFIC PID only
   kill -9 <specific-verified-pid>
   ```

3. **Test servers only on isolated displays**: Use `:997`, `:998`, `:999` (managed by `tests/e2e/` framework)

4. **Never manually start viewers on displays `:1`, `:2` or `:3`** - these are the user's working desktop

5. **On macOS**: User may have production viewers running. NEVER kill any viewer process without explicit user confirmation of the specific PID

6. **Use the e2e test framework** which properly manages isolated test servers with specific PID tracking

### Safe Testing Approach

```bash
# Good: Use e2e framework for testing
cd tests/e2e
python3 run_contentcache_test.py --server-modes local

# Good: Kill only specific test server PIDs
ps aux | grep "Xnjcvnc :99[89]"
kill <specific-test-pid>

# BAD: Pattern-based killing (will kill production!)
pkill -f Xnjcvnc  # ❌ NEVER DO THIS
killall Xnjcvnc   # ❌ NEVER DO THIS
```

## Build Commands

TigerVNC uses CMake for configuration with a convenience Makefile for building.

### Quick Start

After initial CMake configuration (see below), use these simple commands:

```bash
# Build everything (viewer + server)
make

# Build C++ viewer only (njcvncviewer)
make viewer

# Build Rust viewer only (njcvncviewer-rs)
make rust_viewer

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

```bash
make              # Build viewer + server (default)
make viewer       # C++ viewer only
make rust_viewer  # Rust viewer only  
make server       # Server only (requires xserver setup)
```

### Build System Architecture

TigerVNC uses **CMake** (common libraries, viewer) and **Autotools** (Xnjcvnc server). Top-level Makefile coordinates both.

### Binary Locations

```bash
build/vncviewer/njcvncviewer                    # C++ viewer
rust-vnc-viewer/target/release/njcvncviewer-rs  # Rust viewer
build/unix/xserver/hw/vnc/Xnjcvnc               # Server
```

**Don't confuse with system binaries** (`/usr/bin/Xnjcvnc`, `/usr/bin/Xtigervnc`).

### Running Tests

```bash
# Run all unit tests (requires GTest)
ctest --test-dir build --output-on-failure -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)

# Run specific test
ctest --test-dir build -R <test_pattern> -V

# Performance benchmarks
./build/tests/perf/encperf   # Encoding
./build/tests/perf/decperf   # Decoding
```

### Creating Release Packages

```bash
make -C build tarball    # Linux/Unix tarball
make -C build dmg        # macOS DMG
make -C build installer  # Windows installer (requires Inno Setup)
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
- **unix/**: Unix/Linux server components (x0vncserver, vncpasswd, vncconfig, xserver patches)
- **win/**: Windows server components (winvnc - unmaintained)
- **java/**: Java-based VNC viewer (optional, requires `-DBUILD_JAVA=ON`)

### Testing Structure

- **tests/unit/**: GoogleTest unit tests
- **tests/perf/**: Performance benchmarks
- **tests/e2e/**: End-to-end ContentCache/PersistentCache protocol tests

**Test Status** (November 13, 2025):
- 6 passing tests (ContentCache, PersistentCache, eviction handling)
- 8 tests with known issues (see `tests/e2e/TEST_TRIAGE_FINDINGS.md`):
  - Primary bug: Viewer doesn't call `logStats()` on shutdown (affects 4 tests)
  - Test issues: Outdated thresholds, unbounded waits, obsolete assumptions

For detailed test analysis, fixes, and evidence, see:
- `tests/e2e/TEST_TRIAGE_FINDINGS.md` - Complete root cause analysis
- `tests/e2e/README.md` - Test documentation and known issues

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

CMake options: `ENABLE_NLS`, `ENABLE_GNUTLS`, `ENABLE_NETTLE`, `ENABLE_H264`, `BUILD_VIEWER`, `BUILD_JAVA`, `ENABLE_ASAN`, `ENABLE_TSAN`

Use `AUTO` (build if found), `ON` (require), or `OFF` (disable).

### Dependencies

**Required**: zlib, pixman, libjpeg

**Optional**: FLTK 1.3.3+, GnuTLS 3.x, Nettle 3.0+, PAM, Xorg dev libs, ffmpeg, GoogleTest

### Cross-Compilation

See `BUILDING.txt` for MinGW details. Xnjcvnc requires patching Xorg source (`unix/xserver*.patch`).

### Compiler Flags

- C: `-std=gnu99`, C++: `-std=gnu++11`
- Strict warnings enabled: `-Wall -Wextra -Wformat=2 -Wvla`
- Debug builds treated as errors: `-Werror` in debug mode
- Assertions active in all build types (release builds have `-UNDEBUG`)

## Test Environment and Production Servers

### Server Infrastructure

The development and testing environment uses a remote server at `nickc@birdsurvey.hopto.org` (hostname: `quartz`).

**Server Code Location**: `/home/nickc/code/tigervnc`

### VNC Server Processes

There are **two different VNC server binaries** running on the same machine:

#### Production Servers (DO NOT MODIFY)

```bash
# Standard TigerVNC (system-installed)
Xtigervnc :1  # Display :1, port 5901 - PRODUCTION
Xtigervnc :2  # Display :2, port 5902 - PRODUCTION (USER'S ACTIVE DESKTOP)

# Custom fork (Xnjcvnc) - PRODUCTION
Xnjcvnc  :3   # Display :3, port 5903 - PRODUCTION (with ContentCache/PersistentCache)
```

**🔴 CRITICAL**: All of the above are **production servers in active use**. DO NOT:
- Stop or restart these processes
- Modify their configuration  
- Kill these processes (even temporarily!)
- Send them test traffic
- Connect test viewers to them
- Display test windows on display :2 (user's working desktop)

**Why this matters**: Display :2 is the user's active working desktop. Interrupting it or
popping up test windows will disrupt their work. The other displays are also production services.

#### Test Architecture

Use the end-to-end test framework under `tests/e2e`, which launches isolated VNC servers on high-numbered displays (e.g., `:998`, `:999`). See `tests/e2e/README.md`.

**Test servers are safe to manage**:
- Displays: `:998` (port 6898), `:999` (port 6899)
- Managed by e2e framework
- Can be safely killed by specific PID after verification

**⚠️ NEVER run test binaries on production displays**:
- Do NOT run viewers that connect to displays `:1`, `:2`, or `:3`
- Do NOT start test servers on displays `:1`, `:2`, or `:3`
- Do NOT pop up GUI windows on display :2 (user's working desktop)

Note: The local build binary exists at `/home/nickc/code/tigervnc/build/unix/vncserver/Xnjcvnc`. Do not run it on displays `:1`, `:2`, or `:3`.

### SSH Tunnels

**⚠️ Do not tunnel to production displays** `:1`, `:2`, or `:3`.

For testing, use the e2e framework (which starts servers on high-numbered displays `:998`, `:999`) or set up tunnels only to those test displays as needed. See `tests/e2e/README.md`.

**Safe tunnel example** (test servers only):
```bash
# Connect to test server on :998
ssh -L 5998:localhost:6898 user@host

# Connect to test server on :999  
ssh -L 5999:localhost:6899 user@host
```

### Safely Identifying Processes

**ALWAYS verify before killing any process**:

```bash
# List all VNC servers
ps aux | grep -E 'Xnjcvnc|Xtigervnc' | grep -v grep

# Check specific process details
pwdx <PID>                    # Working directory
ps -p <PID> -o pid,args       # Full command line

# Safe identification of test servers
ps aux | grep -E "Xnjcvnc :99[89]"  # Only matches test displays :998, :999

# Example output:
# nickc  849543  Xtigervnc :1      <- PRODUCTION (display :1, do not touch!)
# nickc 3221111  Xtigervnc :2      <- PRODUCTION (display :2, user's desktop, do not touch!)
# nickc 1497451  Xnjcvnc :3        <- PRODUCTION (display :3, do not touch!)
# nickc 3250351  Xnjcvnc :998      <- TEST SERVER (safe to kill by PID only)
# nickc 3250371  Xnjcvnc :999      <- TEST SERVER (safe to kill by PID only)
```

**Safe process termination**:
```bash
# ✅ Good: Kill specific test server by verified PID
kill 3250351 3250371

# ❌ Bad: Pattern matching (kills production!)
pkill -f "Xnjcvnc"           # Kills :3, :998, :999 - WRONG!
killall Xnjcvnc              # Kills all Xnjcvnc - WRONG!
pkill -f "display_number"    # Still dangerous
```

### Test Architecture Management

Use the e2e test framework to start/stop isolated test servers on high-numbered displays. Do not manage or restart production servers.

See `tests/e2e/README.md` for commands and options.

#### Log Locations

```bash
# Production server logs (do not read/modify)
/home/nickc/.vnc/quartz:1.log
/home/nickc/.vnc/quartz:2.log
/home/nickc/.vnc/quartz:3.log

# Test framework logs
# See tests/e2e output and logs produced by the harness

# Client logs (on local Mac)
/tmp/vncviewer_*.log   # Created by njcvncviewer_start.sh
```

### ContentCache Debugging

When debugging rectangle corruption issues:

1. **Capture synchronized logs**:
   ```bash
   # Use the e2e test framework (isolated displays :998/:999)
   python3 tests/e2e/run_contentcache_test.py --verbose
   
   # In another terminal: run the Rust viewer against the e2e test server
   cargo run --package njcvncviewer-rs -- -vv localhost:999 2>&1 | tee /tmp/client_debug.log
   ```

2. **Check ContentCache message flow**:
   - Server sends `CachedRect` (reference) or `CachedRectInit` (full data)
   - Client receives and checks local cache
   - Client sends `RequestCachedData` on cache miss
   - Server queues `CachedRectInit` response
   
3. **Key log patterns to look for**:
   ```
   # Server side
   "ContentCache protocol hit: rect [x,y-x,y] cacheId=N"
   "Client requested cached data for ID N"
   "Targeted refresh for cacheId=N"
   
   # Client side  
   "Received CachedRect: [x,y-x,y] cacheId=N"
   "Cache miss for ID N, requesting from server"
   "Storing decoded rect [x,y-x,y] with cache ID N"
   ```

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

**🔴 Critical**: When killing processes, always verify working directory and exact PID to avoid killing production instances.

```bash
# Step 1: Find candidate processes
ps aux | grep Xnjcvnc | grep -v grep

# Step 2: Verify EACH PID before killing
pwdx <PID>                          # Check working directory
ps -p <PID> -o pid,args             # Check display number

# Step 3: Only kill specific verified test PIDs
kill -TERM <verified-test-pid>

# ❌ FORBIDDEN: Pattern-based killing
# pkill -f Xnjcvnc              # Kills ALL Xnjcvnc including production!
# killall Xnjcvnc               # Kills ALL Xnjcvnc including production!
# pkill -f "script_name.py"     # Kills ALL matching scripts!
```

**Why this is critical**:
- Multiple VNC servers run simultaneously (production `:1`, `:2`, `:3` + test `:998`, `:999`)
- Pattern-based killing will destroy production servers and interrupt user's work
- Display `:2` is the user's active desktop - ANY interruption is disruptive
- Recovery from killing production servers requires manual restart and may lose state
- The user has explicitly warned about this multiple times - it's very important to them!

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

## ContentCache and PersistentCache Implementation

This fork includes two custom cache protocols that provide 63-99% bandwidth reduction for repeated content:

- **ContentCache**: Session-based cache with server-assigned IDs (20-byte references)
- **PersistentCache**: Disk-backed cache with content hashes (47-byte references, survives sessions)

### Key Files

- `common/rfb/ContentCache.h/cxx`: ContentCache with ARC (Adaptive Replacement Cache) algorithm
- `common/rfb/PersistentCache.h/cxx`: PersistentCache with disk persistence
- `common/rfb/EncodeManager.cxx`: Server-side integration (cache lookups, insertions)
- `common/rfb/DecodeManager.cxx`: Client-side integration (cache retrieval, blitting)
- `common/rfb/encodings.h`: Protocol constants and capability negotiation

### Protocol Overview

**ContentCache**:
- `CachedRect` (20 bytes): Server references by cache ID
- `CachedRectInit` (20 bytes + encoding): Full data + ID for storage

**PersistentCache**:
- `PersistentCachedRect` (47 bytes): Server references by content hash
- `PersistentCachedRectInit` (47 bytes + encoding): Full data + hash for storage

### Configuration

Server parameters (add to `~/.vnc/config`):
```bash
# ContentCache (session-only)
EnableContentCache=1          # Enable (default: true)
ContentCacheSize=2048         # Cache size in MB (default: 2048)
ContentCacheMaxAge=0          # Max age in seconds (0 = unlimited)
ContentCacheMinRectSize=2048  # Min pixels to cache (default: 2048)

# PersistentCache (survives sessions)
EnablePersistentCache=1       # Enable (default: true)
PersistentCacheSize=256       # Cache size in MB (default: 256)
PersistentCacheMinRectSize=2048  # Min pixels to cache (default: 2048)
```

### Test Results (November 2025)

**ContentCache** (128×128 logos, 30s duration):
- Hit rate: 63-67%, Bandwidth saved: ~300 KB
- Test: `tests/e2e/test_cpp_contentcache.py`

**PersistentCache** (128×128 logos, 30s duration):
- Hit rate: 100%, Bandwidth reduction: 99.7%, Saved: ~517 KB
- Test: `tests/e2e/test_cpp_persistentcache.py`

### Performance

- **Bandwidth**: 63-99% reduction for cache hits (20-47 bytes vs KB of compressed data)
- **CPU**: Zero decode cost for hits (memory blit vs decompression)
- **Memory**: ~16KB per cached 64×64 tile

### Debugging

```bash
# Enable verbose logging in the e2e test framework as needed
# See tests/e2e/README.md for options
```

### Documentation

See `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` for comprehensive design, implementation details, build system notes, known issues, and troubleshooting guide.

## Related Documentation

- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: Comprehensive ContentCache guide
- `PERSISTENTCACHE_DESIGN.md`: PersistentCache protocol specification
- `ARC_ALGORITHM.md`: Adaptive Replacement Cache algorithm details
- `tests/e2e/README.md`: End-to-end test suite documentation
- `README.rst`: General TigerVNC documentation
- `BUILDING.txt`: Detailed build instructions for all platforms
