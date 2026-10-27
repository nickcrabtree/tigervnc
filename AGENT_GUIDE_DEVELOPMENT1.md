# Agent guide: development1

Read when selected by AGENTS.md, not at every startup. Paths and commands are
relative to the repository root. Detailed source sections are retained below.
Current AGENTS.md and shared policy supersede dated history: old quiet-window
times, expired governor lifts, amend/reset advice and older host assumptions do
not grant permission. For current quiet-window status consult
`~/scripts/cross_repo_notes/AGENT_POLICY.md`.

## Detailed reference

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

## Build Commands

TigerVNC uses CMake for configuration with a convenience Makefile for building.

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

### Running Tests

Run all tests (including e2e) via `./run_tests.sh` from the repository root.

```bash
ctest --test-dir build --output-on-failure -j$(sysctl -n hw.ncpu 2>/dev/null || nproc)
ctest --test-dir build -R <test_pattern> -V   # specific test

./build/tests/perf/encperf   # Encoding benchmark
./build/tests/perf/decperf   # Decoding benchmark
```

### Testing Structure

- **tests/unit/**: GoogleTest unit tests
- **tests/perf/**: Performance benchmarks
- **tests/e2e/**: End-to-end session cache/PersistentCache protocol tests — see `tests/e2e/README.md` and
  `tests/e2e/TEST_TRIAGE_FINDINGS.md` for current triage status.

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

## Code quality (desloppify)

See [docs/DESLOPPIFY.md](docs/DESLOPPIFY.md) for current scores, dimension breakdown, open issues,
next steps, and the full workflow for running mechanical checks and subjective review batches.

Desloppify is scoped to `rust-vnc-viewer/` only — the upstream C++ codebase is not tracked.

Current state: overall 22.8 / objective 91.4 / strict 22.8 (target 85.0).
Objective is above 80 (mechanical quality good). Strict is low because subjective review has not been run.
