# Agent Instructions

## Host Topology and Test Flow

- Mac / ARO host (Nicks-Mac): local working clone is /Users/nickc/code/tigervnc; use it for lightweight inspection, editing, commits, and macOS Rust builds/checks only.
- quartz Linux host: quartz.local on the home LAN and birdsurvey.hopto.org from outside are the same machine; its repo is normally /home/nickc/code/tigervnc.
- Run Linux/X11 gates on quartz: C++/Rust e2e, Xvfb/Openbox, screenshot, CTest, cache trace, and PersistentCache server/viewer tests belong on quartz, not the Mac GUI.
- Remote VM flow: a rehydrated VM may use /data_parallel/PreStackPro/share/nickc/tigervnc; git askpass there SSHes back to quartz/birdsurvey for ~/bin/ghapp-token.
- Move changes between clones with git commit/push/pull. Do not copy working-tree files between Mac, quartz, and VM clones.

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

## Rust viewer convergence / PersistentCache parity

Current convergence authority:

- `rust-vnc-viewer/docs/CONVERGENCE_GATES.md` is the current checklist for Rust viewer ↔ C++ reference convergence work.
- The current Rust PersistentCache implementation is 64-bit cache-ID (`u64`) based. Older references in planning docs to 16-byte hashes, `[u8; 16]`, or hash-wire-format semantics should be treated as historical until rewritten.

Checkpoint commits:

- Implementation checkpoint: `aaa3e986 rust viewer: unify PersistentCache IDs to u64`.
- Documentation/checklist checkpoint: `406cfe8d docs: rebaseline Rust viewer convergence gates`.

Gate 2 starting point — C++ reference protocol parity:

- Start with `tests/e2e/test_cpp_persistentcache.py` and `tests/e2e/test_rust_persistentcache.py` as the smallest comparable C++ vs Rust PersistentCache pair.
- Useful follow-on harnesses include:
  - `tests/e2e/test_cpp_cache_back_to_back.py`
  - `tests/e2e/test_cache_parity.py`
  - `tests/e2e/test_persistent_cache_bandwidth.py`
  - `tests/e2e/test_persistent_cache_eviction.py`
- Known viewer binaries/artifacts:
  - C++ viewer: `build/vncviewer/njcvncviewer`
  - Rust viewer artifacts: `rust-vnc-viewer/target/...`, including `njcvncviewer-rs` builds.

Protocol parity evidence to compare:

- `PersistentCachedRect`
- `PersistentCachedRectInit`
- `PersistentCacheQuery`
- `PersistentCacheEviction`
- hit/miss counters
- cache path / disk save-load markers
- message order, encoding IDs, byte sizes, query batches, and eviction behaviour

Workflow note:

- Avoid unguarded `grep/find | head` pipelines under `set -euo pipefail`; they can return `rc=141` from SIGPIPE even when the generated output file is useful. Prefer bounded Python filtering, `awk`, `sed -n`, or append `|| true` around intentionally truncated pipelines.
