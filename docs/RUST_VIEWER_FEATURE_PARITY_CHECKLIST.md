# Rust VNC Viewer Feature Parity Checklist

Standing checklist for `rust-vnc-viewer` and `njcvncviewer-rs` feature parity with the C++ TigerVNC viewer.

## Sources to re-check

- Log-driven cache trace, cache tiling, PersistentCache, and lazy 32-bit cache docs.
- Fullscreen and multi-monitor spec plus M1 fullscreen testing docs.
- Recent ARO logs for Rust viewer, cache, fullscreen, monitor, keyboard, clipboard, and cursor work.

## Completed baseline

- [x] Full Rust workspace tests and top-level CTest suite pass on quartz.
- [x] Desktop resize updates viewport framebuffer size and reapplies scale mode.
- [x] `CachedRectSeed` encoding 105 is decoded and wired into Rust PersistentCache.
- [x] Aborted PersistentCache WIP was validated, committed, and pushed.

## M1: Cache parity validation

- [ ] Adapt the log-driven cache trace plan for `njcvncviewer-rs`.
- [ ] Run no-cache, ContentCache-only, PersistentCache-only, and unified cache scenarios.
- [ ] Compare Rust with C++ idle, first display, redisplay, refs, INITs, hits, misses, queries, evictions, and bytes.
- [ ] Ensure Rust logs expose event type, encoding, rect, cache id or ref, bytes, and negotiated protocol.
- [ ] Add a known-good trace fixture and parser for regression testing.

## M2: Remaining parity

- [ ] Add query-classification tests and verify miss, eviction, INIT, offset, seed, disk, reconnect, and geometry cases.
- [ ] Re-run fullscreen, multi-monitor, scaling, resize, cursor, keyboard, mouse, clipboard, UI, reconnect, and CLI checks.
- [ ] Finish cache tiling follow-ups and defer lazy 32-bit cache refresh until baseline trace parity is proven.
