# Agent guide: operations1

Read when selected by AGENTS.md, not at every startup. Paths and commands are
relative to the repository root. Detailed source sections are retained below.
Current AGENTS.md and shared policy supersede dated history: old quiet-window
times, expired governor lifts, amend/reset advice and older host assumptions do
not grant permission. For current quiet-window status consult
`~/scripts/cross_repo_notes/AGENT_POLICY.md`.

## Detailed reference

## Original introduction: Agent Instructions

The bug database for this repository is maintained in `BUGS.md` in the project root.

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

### Quick Start

```bash
make              # Build everything (viewer + server)
make viewer       # C++ viewer only (njcvncviewer)
make rust_viewer  # Rust viewer only (njcvncviewer-rs)
make server       # Server only (Xnjcvnc)
```

### Binary Locations

```text
build/vncviewer/njcvncviewer                    # C++ viewer
rust-vnc-viewer/target/release/njcvncviewer-rs  # Rust viewer
build/unix/xserver/hw/vnc/Xnjcvnc               # Server
```

Don't confuse with system binaries (`/usr/bin/Xnjcvnc`, `/usr/bin/Xtigervnc`).

### Creating Release Packages

```bash
make -C build tarball    # Linux/Unix tarball
make -C build dmg        # macOS DMG
make -C build installer  # Windows installer (requires Inno Setup)
```

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
