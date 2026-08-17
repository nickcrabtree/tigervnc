# Agent Instructions

> **Global rules:** see `~/code/AGENTS.md` for conventions that span all repos
> and machines. Those apply here too, in addition to anything below.

The bug database for this repository is maintained in `BUGS.md` in the project root.

## Host Topology and Test Flow

- Mac / ARO host (Nicks-Mac): local working clone is /Users/nickc/code/tigervnc; use it for lightweight inspection,
  editing, commits, and macOS Rust builds/checks only.
- quartz Linux host: quartz.local on the home LAN and birdsurvey.hopto.org from outside are the same machine; its repo
  is normally /home/nickc/code/tigervnc.
- Run Linux/X11 gates on quartz: C++/Rust e2e, Xvfb/Openbox, screenshot, CTest, cache trace, and PersistentCache
  server/viewer tests belong on quartz, not the Mac GUI.
- Remote VM flow: a rehydrated VM may use /data_parallel/PreStackPro/share/nickc/tigervnc; git askpass there SSHes back
  to quartz/birdsurvey for ~/bin/ghapp-token.
- Move changes between clones with git commit/push/pull. Do not copy working-tree files between Mac, quartz, and VM
  clones. All code sync between the local dev machine and remote servers must use git, never rsync/scp — it preserves
  history, avoids corrupting the remote repo, and keeps both sides in sync with origin.

### Server Infrastructure

1. **quartz** (`nickc@birdsurvey.hopto.org`): local development server
   - Code location: `/home/nickc/code/tigervnc`
   - Production displays: `:1`, `:2`, `:3`
2. **AWS Jakarta** (`pspuser@108.136.194.23`): remote testing server
   - SSH: `ssh -i ~/premierJakarta.key pspuser@108.136.194.23`
   - Code location: `/data_parallel/PreStackPro/share/nickc/tigervnc`
   - Production display: `:1` (port 5901)

## ⚠️ ALWAYS USE TIMEOUTS

Commands that might hang or wait for user input must always run under `timeout`:

```bash
timeout 60 ssh user@host 'command'
timeout 120 ./script.sh
timeout 30 make test
```

Defaults: quick commands 10s, SSH 30s, builds 300s, test runs 120-300s, interactive/GUI scripts 60-120s. Skip only for
commands known to be instant (`echo`, variable assignment).

**macOS host note:** GNU coreutils `timeout` is available as `~/bin/timeout` (a symlink → `/opt/local/bin/gtimeout`).
Use `~/bin/timeout` as the preferred form (`~/bin` is on `$PATH`). `/usr/local/bin/timeout` does **not** exist on this
machine.

## VNC Viewer Testing Guardrail

Do not launch GUI VNC viewer windows on Nick's Mac during automated testing,
trace capture, or ARO-driven investigation unless Nick explicitly asks for a
local interactive viewer.
Use quartz and headless VNC server/test harness workflows for
Rust TigerVNC viewer testing. Follow the existing tests for how to exercise
protocol/cache behaviour without opening a macOS viewer window.

## Production VNC Server Guardrail

Do not touch, restart, reconfigure, connect automated experiments to, or
otherwise interfere with Nick's production VNC servers on ports 5901, 5902, or
5903. Treat those ports as reserved unless Nick explicitly authorises their use
for a specific interactive task.
For automated Rust/C++ VNC viewer testing, copy the existing C++ viewer
headless test harness pattern and create isolated throwaway VNC servers on
non-production ports. Do not invent ad-hoc local macOS GUI viewer runs or point
experiments at the production servers.

This repo runs two VNC server binaries with production instances that must
**never** be killed, beyond the global no-pkill policy in `~/code/AGENT_POLICY.md`
(kill only specific, verified PIDs — never `pkill`/`killall`):

```text
Xtigervnc :1  # port 5901 - PRODUCTION
Xtigervnc :2  # port 5902 - PRODUCTION (user's active desktop)
Xnjcvnc   :3  # port 5903 - PRODUCTION (session cache/PersistentCache)
```

On macOS: `njcvncviewer` (user's active VNC viewer sessions) and `Xvfb` (may be legitimate) are similarly off-limits
without explicit PID verification.

Test servers only run on isolated displays `:998`/`:999` (ports 6898/6899), managed by the `tests/e2e/` framework — safe
to kill by specific PID after verification:

```bash
ps aux | grep -E "Xnjcvnc :99[89]"   # only matches test displays
pwdx <PID>                           # verify working directory
ps -p <PID> -o pid,args=             # verify full command
kill <specific-verified-pid>         # only after verification
```

Never manually start viewers on displays `:1`, `:2`, or `:3` — these are the user's working desktop. Never kill or
restart the VNC server on AWS Jakarta or quartz without explicit user permission.

### Starting and Managing Servers

Always use the startup scripts rather than raw `Xnjcvnc` commands.

```bash
# AWS Jakarta (pspuser) — auto-selects display, correct binary
timeout 30 ssh -i ~/premierJakarta.key pspuser@108.136.194.23 \
  '/data_parallel/PreStackPro/share/nickc/tigervnc/scripts/njcvncserver_start.bash'

# quartz (nickc)
~/scripts/njcvncserver_start.bash

# Local viewer (macOS) — handles password file, redirects stderr to timestamped log
~/scripts/njcvncviewer_start.sh [options] host[:display]
~/scripts/njcvncviewer_start.sh -FullColor=1 -AutoSelect=0 108.136.194.23:1
```

### SSH Tunnels

Do not tunnel to production displays `:1`, `:2`, or `:3`. For testing, tunnel only to the e2e test displays:

```bash
ssh -L 5998:localhost:6898 user@host  # test server :998
ssh -L 5999:localhost:6899 user@host  # test server :999
```

## Build Commands

TigerVNC uses CMake for configuration with a convenience Makefile for building.

### Quick Start

```bash
make              # Build everything (viewer + server)
make viewer       # C++ viewer only (njcvncviewer)
make rust_viewer  # Rust viewer only (njcvncviewer-rs)
make server       # Server only (Xnjcvnc)
```

### Initial Configuration (first time only)

```bash
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

### Xserver Setup (required for server build)

Building the Xnjcvnc server requires a one-time setup of the Xorg server source, separate from the CMake configuration.

**Ubuntu/Debian:**

```bash
sudo apt-get install xorg-server-source

mkdir -p build/unix
cp -R unix/xserver build/unix/

cd build/unix/xserver
tar xf /usr/src/xorg-server.tar.xz --strip-components=1

# For Xorg 21.x
patch -p1 < ../../../unix/xserver21.patch
# For Xorg 1.20.x: patch -p1 < ../../../unix/xserver120.patch

autoreconf -fiv

./configure --with-pic --without-dtrace --disable-static --disable-dri \
  --disable-xinerama --disable-xvfb --disable-xnest --disable-xorg \
  --disable-dmx --disable-xwin --disable-xephyr --disable-kdrive \
  --disable-config-hal --disable-config-udev --disable-dri2 --enable-glx \
  --with-default-font-path="catalogue:/etc/X11/fontpath.d,built-ins" \
  --with-xkb-path=/usr/share/X11/xkb \
  --with-xkb-output=/var/lib/xkb \
  --with-xkb-bin-directory=/usr/bin \
  --with-serverconfig-path=/usr/lib/xorg

cd ../../..

mkdir -p build/unix/vncserver
ln -sf ../xserver/hw/vnc/Xnjcvnc build/unix/vncserver/Xnjcvnc
```

**RHEL/Fedora/CentOS**: install `xorg-x11-server-source` (source typically in `/usr/share/xorg-x11-server-source`).
**Arch**: install `xorg-server` source package. Adjust `--with-serverconfig-path` for your distribution.

Verify setup: `ls -la build/unix/xserver/config.status` and `ls -la build/unix/xserver/hw/vnc/Makefile`.

### Binary Locations

```text
build/vncviewer/njcvncviewer                    # C++ viewer
rust-vnc-viewer/target/release/njcvncviewer-rs  # Rust viewer
build/unix/xserver/hw/vnc/Xnjcvnc               # Server
```

Don't confuse with system binaries (`/usr/bin/Xnjcvnc`, `/usr/bin/Xtigervnc`).

### Running Tests

Run all tests (including e2e) via `./run_tests.sh` from the repository root.

```bash
ctest --test-dir build --output-on-failure -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)
ctest --test-dir build -R <test_pattern> -V   # specific test

./build/tests/perf/encperf   # Encoding benchmark
./build/tests/perf/decperf   # Decoding benchmark
```

### Creating Release Packages

```bash
make -C build tarball    # Linux/Unix tarball
make -C build dmg        # macOS DMG
make -C build installer  # Windows installer (requires Inno Setup)
```

## Code Architecture

TigerVNC is organized as layered libraries with platform-specific components.

### Core Libraries (common/)

- **core/**: Cross-platform utilities (logging, configuration, exceptions, timers, string handling, region management
  via pixman)
- **rdr/**: Reader/Writer abstraction for serialization and stream I/O (buffered streams, file streams, TLS streams,
  zlib compression streams)
- **network/**: Socket abstractions (TCP sockets, Unix sockets, TLS sockets)
- **rfb/**: RFB protocol implementation — connection handling (CConnection/SConnection), message readers/writers,
  security types (Plain, VncAuth, VeNCrypt, TLS, RSA-AES, DH), encoders/decoders (Raw, CopyRect, RRE, Hextile, Tight,
  ZRLE, H.264), server-side components (VNCServerST, VNCSConnectionST, EncodeManager), pixel formats, cursors, update
  tracking

Dependency chain: `rfb` depends on `core`, `rdr`, and `network`. `network` depends on `rdr`.

### Platform-Specific Components

- **vncviewer/**: Cross-platform VNC viewer (FLTK-based GUI, produces njcvncviewer)
- **unix/**: Unix/Linux server components (x0vncserver, vncpasswd, vncconfig, xserver patches)
- **win/**: Windows server components (winvnc - unmaintained)
- **java/**: Java-based VNC viewer (optional, requires `-DBUILD_JAVA=ON`)

### Testing Structure

- **tests/unit/**: GoogleTest unit tests
- **tests/perf/**: Performance benchmarks
- **tests/e2e/**: End-to-end session cache/PersistentCache protocol tests — see `tests/e2e/README.md` and
  `tests/e2e/TEST_TRIAGE_FINDINGS.md` for current triage status.

## Key Conventions

- **Build system**: CMake 3.10+ required; out-of-tree builds recommended (`-B build`). Default build type `Release`
  (assertions kept via `-UNDEBUG`). `-DCMAKE_BUILD_TYPE=Debug` gives `-Og` and `-Werror`.
- **Platform support**: Linux/Unix native with system X11 libs; macOS native (requires FLTK, produces .dmg); Windows via
  MinGW/MinGW-w64 (MSVC not supported, requires FLTK, produces .exe installer).
- **Optional features** (CMake options): `ENABLE_NLS`, `ENABLE_GNUTLS`, `ENABLE_NETTLE`, `ENABLE_H264`, `BUILD_VIEWER`,
  `BUILD_JAVA`, `ENABLE_ASAN`, `ENABLE_TSAN` — `AUTO` (build if found), `ON` (require), `OFF` (disable).
- **Dependencies**: required — zlib, pixman, libjpeg. Optional — FLTK 1.3.3+, GnuTLS 3.x, Nettle 3.0+, PAM, Xorg dev
  libs, ffmpeg, GoogleTest.
- **Cross-compilation**: see `BUILDING.txt` for MinGW details. Xnjcvnc requires patching Xorg source (`unix/xserver*.patch`).
- **Compiler flags**: C `-std=gnu99`, C++ `-std=gnu++11`. Strict warnings: `-Wall -Wextra -Wformat=2 -Wvla`. Debug
  builds treat warnings as errors (`-Werror`). Assertions active in all build types.
- Avoid unguarded `grep/find | head` pipelines under `set -euo pipefail`; they can return `rc=141` from SIGPIPE even
  when the generated output file is useful. Prefer bounded Python filtering, `awk`, `sed -n`, or append `|| true` around
  intentionally truncated pipelines.

## Log Locations

```text
# quartz production server logs (do not modify)
/home/nickc/.vnc/quartz:1.log
/home/nickc/.vnc/quartz:2.log
/home/nickc/.vnc/quartz:3.log

# AWS Jakarta server log (pspuser)
/home/pspuser/.config/tigervnc/ip-10-20-0-24.ap-southeast-3.compute.internal:1.log

# Client (macOS) — created by njcvncviewer_start.sh wrapper
/tmp/njcvncviewer_YYYY-MM-DD_HH-MM-SS.log

# PersistentCache debug logs (when TIGERVNC_PERSISTENTCACHE_DEBUG=1)
/tmp/persistentcache_debug_*.log
```

- stdout: startup/shutdown messages, hourly stats. stderr (log file): debug messages, vlog output, cache operations.

## Debug Logging Framework

The viewer uses `vlog`: `vlog.error()` (always shown), `vlog.info()` (stdout by default), `vlog.debug()` (stderr, goes
to log file). Enable verbose output with `-v`/`-vv`:

```bash
~/scripts/njcvncviewer_start.sh -v host:display
# Debug output goes to /tmp/njcvncviewer_YYYY-MM-DD_HH-MM-SS.log
```

Grep patterns for log analysis:

```bash
grep -E 'PC(CLT|SRV)|CACHE_' /tmp/njcvncviewer_*.log                       # Cache operations
grep -E 'pixel format|FullColor|Throughput|rgb[0-9]+' /tmp/njcvncviewer_*.log  # Pixel format / auto-select
grep -E 'hash|Hash|canonical|lossy' /tmp/njcvncviewer_*.log                # Hash operations
grep -E 'imageRect|blit|DecodeManager' /tmp/njcvncviewer_*.log             # Decode/blit operations
```

Server-side (EncodeManager): look for `CC doUpdate`, `PCSRV TOPBAND_*`, `PersistentCache protocol HIT/INIT` — server
logs cache lookups, hits, misses, and ID tracking.

### Cache Debugging

When debugging rectangle corruption issues:

1. Capture synchronized logs via the e2e test framework (isolated displays `:998`/`:999`):

   ```bash
   python3 tests/e2e/run_cache_test.py --verbose
   # In another terminal, run the Rust viewer against the e2e test server:
   cargo run --package njcvncviewer-rs -- -vv localhost:999 2>&1 | tee /tmp/client_debug.log
   ```

2. Check cache message flow: server sends `CachedRect` (reference) or `CachedRectInit` (full data); client checks
   local cache and sends `RequestCachedData` on miss; server queues `CachedRectInit` response.
3. Key log patterns — server: `"Cache protocol hit: rect [...] cacheId=N"`, `"Client requested cached data for ID N"`,
   `"Targeted refresh for cacheId=N"`; client: `"Received CachedRect: [...] cacheId=N"`, `"Cache miss for ID N,
   requesting from server"`, `"Storing decoded rect [...] with cache ID N"`.
4. **Auto-Select color depth issue**: AutoSelect can cause visual corruption by switching between 8bpp (rgb332) and
   32bpp (rgb888) based on bandwidth — symptom is purple/color-shifted areas that "creep" across the display, caused by
   format switching at the 256kbit/s threshold combined with lossy cache entries. Disable with `-AutoSelect=0
   -FullColor=1` (or `AutoSelect=0` / `FullColor=1` in `~/.vnc/default.tigervnc`).

## Important Code Patterns and Gotchas

### Stride is in Pixels, Not Bytes

`PixelBuffer::getBuffer()` returns stride in **pixels**, not bytes.

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

This caused a critical bug (Oct 7 2025) in cache hash calculation that resulted in frequent hash collisions and severe
visual corruption. Always multiply stride by `bytesPerPixel` when calculating byte lengths.

### PixelBuffer Access Patterns

```cpp
// Read-only
const uint8_t* buffer = pb->getBuffer(rect, &stride);

// Read-write — must call commitBufferRW when done!
uint8_t* buffer = pb->getBufferRW(rect, &stride);
// Modify buffer...
pb->commitBufferRW(rect);
```

Stride determines row traversal:

```cpp
for (int y = 0; y < height; y++) {
    const uint8_t* row = buffer + (y * stride * bytesPerPixel);
    for (int x = 0; x < width; x++) {
        const uint8_t* pixel = row + (x * bytesPerPixel);
        // Process pixel...
    }
}
```

## Cache and PersistentCache Implementation

This fork includes two custom cache protocols providing 63-99% bandwidth reduction for repeated content:

- **session cache**: session-based cache with server-assigned IDs (20-byte references)
- **PersistentCache**: disk-backed cache with content hashes (47-byte references, survives sessions)

### Key Files

- `common/rfb/session cache.h/cxx`: session cache with ARC (Adaptive Replacement Cache) algorithm
- `common/rfb/PersistentCache.h/cxx`: PersistentCache with disk persistence
- `common/rfb/EncodeManager.cxx`: server-side integration (cache lookups, insertions)
- `common/rfb/DecodeManager.cxx`: client-side integration (cache retrieval, blitting)
- `common/rfb/encodings.h`: protocol constants and capability negotiation

### Protocol Overview

- **session cache**: `CachedRect` (20 bytes, server references by cache ID), `CachedRectInit` (20 bytes + encoding, full
  data + ID for storage)
- **PersistentCache**: `PersistentCachedRect` (47 bytes, server references by content hash), `PersistentCachedRectInit`
  (47 bytes + encoding, full data + hash for storage)

### Configuration

Server parameters (add to `~/.vnc/config`):

```bash
EnablePersistentCache=1          # Enable (default: true)
PersistentCacheSize=256          # Cache size in MB (default: 256)
PersistentCacheMinRectSize=2048  # Min pixels to cache (default: 2048)
```

### Test Results (November 2025)

- **Session cache** (128×128 logos, 30s duration): hit rate 63-67%, ~300 KB saved. Test: `tests/e2e/test_cpp_sessioncache.py`.
- **PersistentCache** (128×128 logos, 30s duration): hit rate 100%, 99.7% bandwidth reduction, ~517 KB saved. Test: `tests/e2e/test_cpp_persistentcache.py`.

Overall: 63-99% bandwidth reduction for cache hits (20-47 bytes vs KB of compressed data); zero decode cost for hits
(memory blit vs decompression); ~16KB memory per cached 64×64 tile.

## Rust viewer convergence / PersistentCache parity

Current convergence authority:

- `rust-vnc-viewer/docs/CONVERGENCE_GATES.md` is the current checklist for Rust viewer ↔ C++ reference convergence work.
- The current Rust PersistentCache implementation is 64-bit cache-ID (`u64`) based. Older references in planning docs to
  16-byte hashes, `[u8; 16]`, or hash-wire-format semantics should be treated as historical until rewritten.

Checkpoint commits:

- Implementation checkpoint: `aaa3e986 rust viewer: unify PersistentCache IDs to u64`.
- Documentation/checklist checkpoint: `406cfe8d docs: rebaseline Rust viewer convergence gates`.

Gate 2 starting point — C++ reference protocol parity:

- Start with `tests/e2e/test_cpp_persistentcache.py` and `tests/e2e/test_rust_persistentcache.py` as the smallest
  comparable C++ vs Rust PersistentCache pair.
- Useful follow-on harnesses: `tests/e2e/test_cpp_cache_back_to_back.py`, `tests/e2e/test_cache_parity.py`,
  `tests/e2e/test_persistent_cache_bandwidth.py`, `tests/e2e/test_persistent_cache_eviction.py`.
- Known viewer binaries/artifacts: C++ viewer `build/vncviewer/njcvncviewer`; Rust viewer artifacts under
  `rust-vnc-viewer/target/...`, including `njcvncviewer-rs` builds.

Protocol parity evidence to compare: `PersistentCachedRect`, `PersistentCachedRectInit`, `PersistentCacheQuery`,
`PersistentCacheEviction`, hit/miss counters, cache path / disk save-load markers, message order, encoding IDs, byte
sizes, query batches, and eviction behaviour.

## Code quality (desloppify)

See [docs/DESLOPPIFY.md](docs/DESLOPPIFY.md) for current scores, dimension breakdown, open issues,
next steps, and the full workflow for running mechanical checks and subjective review batches.

Desloppify is scoped to `rust-vnc-viewer/` only — the upstream C++ codebase is not tracked.

Current state: overall 22.8 / objective 91.4 / strict 22.8 (target 85.0).
Objective is above 80 (mechanical quality good). Strict is low because subjective review has not been run.

## Related Documentation

- `PERSISTENTCACHE_DESIGN.md`: PersistentCache protocol specification
- `ARC_ALGORITHM.md`: Adaptive Replacement Cache algorithm details
- `tests/e2e/README.md`: End-to-end test suite documentation
- `README.rst`: General TigerVNC documentation
- `BUILDING.txt`: Detailed build instructions for all platforms

## Shared utility scripts

- `~/scripts/fix_markdown_blanks.py` — fixes MD022/MD032 markdownlint violations
  (missing blank lines around headings and list blocks) in any Markdown file.
  Code-fence aware and idempotent. Usage: `python ~/scripts/fix_markdown_blanks.py <file.md>`
  or `--check` to report without modifying.
