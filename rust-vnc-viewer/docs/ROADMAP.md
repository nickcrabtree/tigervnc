# Rust VNC Viewer roadmap
> **2026-05-16 rebaseline:** Rust PersistentCache is now implemented and full-suite verified around **64-bit cache IDs (`u64`)** for PersistentCachedRect, PersistentCachedRectInit, query, eviction, and seed paths. Older references below to 16-byte hashes, `[u8; 16]`, or hash-wire-format semantics are historical context until rewritten. Use `rust-vnc-viewer/docs/CONVERGENCE_GATES.md` as the current checklist.


Development roadmap for bringing the Rust viewer to feature parity with the
C++ viewer while keeping the implementation notes and test expectations aligned
with the code.

## Roadmap principles

- Treat `docs/ROADMAP.md` as the canonical roadmap for the Rust viewer.
- Keep documentation updates in the same series as the code and test changes
  they describe.
- Prefer small, reviewable commits that make the parity story easy to follow.
- Preserve protocol details that matter for interoperability, especially for
  PersistentCache negotiation and wire-format behaviour.

## Milestone overview

- **M0 (complete):** Core desktop VNC functionality is stable.
- **M1 (next):** Enhanced fullscreen behaviour for desktop use.
- **M2 (next):** Multi-monitor support and monitor-navigation polish.
- **M3 (priority after M2):** PersistentCache protocol parity with the C++
  viewer.
- **M4+:** Windowed UX polish and lower-priority advanced features.

## M0 foundation

**Status:** Complete.

**Scope:** Core desktop VNC viewer functionality.

### M0 completed features

- Complete RFB protocol implementation for the standard encodings.
- ContentCache protocol support.
- Cross-platform GUI built on `egui` and `eframe`.
- Basic keyboard and mouse input handling.
- Bidirectional clipboard synchronisation.
- CLI-based configuration.
- Production-ready error handling and logging.

### M0 architecture achievements

- Workspace crate structure with extensive test coverage.
- Async Tokio-based networking.
- Comprehensive automated test coverage.

## M1 enhanced fullscreen support

**Priority:** High.

**Timeline:** 1 to 2 weeks.

**Goal:** Excellent single-monitor fullscreen behaviour.

### M1 core fullscreen checklist

- [x] Reliable `F11` fullscreen toggle.
- [x] `--fullscreen` CLI flag for immediate fullscreen startup.
- [ ] Intelligent borderless versus exclusive fullscreen mode selection with
  fallback.
- [ ] Windowed-state preservation for position and size restoration.

### M1 DPI and scaling checklist

- [x] Per-monitor DPI detection wired via monitor enumeration.
- [ ] High-DPI rendering support for Retina and 4K displays.
- [ ] Scaling policies for fit, fill, and 1:1 presentation.
- [ ] Configurable aspect-ratio preservation.

### M1 keyboard shortcut checklist

- [x] `F11` as the primary fullscreen toggle.
- [x] `Ctrl+Alt+F` as an alternative fullscreen toggle.
- [ ] Optional `Esc` fullscreen exit.
- [ ] `F1` connection information overlay in fullscreen mode.

### M1 acceptance criteria

- [ ] Smooth fullscreen transitions in under 200 ms.
- [ ] No visible flicker or fullscreen artefacts.
- [ ] Correct scaling on common monitor configurations.
- [ ] Reliable transitions between windowed and fullscreen modes.
- [ ] Consistent behaviour across X11 and Wayland.

### M1 manual QA checklist

- [ ] Standard 1920x1080 monitor.
- [ ] 4K monitor at 150% scaling.
- [ ] Ultrawide 21:9 monitor.
- [ ] Remote desktop larger than the local display.
- [ ] Remote desktop smaller than the local display.

## M2 multi-monitor support

**Priority:** High.

**Timeline:** 1 to 2 weeks after M1.

**Goal:** Seamless multi-monitor fullscreen behaviour.

### M2 monitor enumeration checklist

- [x] Detect all monitors with basic metadata.
- [x] Identify the primary monitor reliably.
- [x] Record monitor name, resolution, and DPI.
- [ ] Guarantee deterministic ordering across runs.

### M2 monitor selection checklist

- [x] Parse and store `--monitor primary|index|name` CLI selection.
- [ ] Runtime switching between monitors.
- [x] Fallback to the primary monitor when a requested monitor is missing.
- [ ] Hotplug detection for monitor connect and disconnect.

### M2 multi-monitor navigation checklist

- [x] `Ctrl+Alt+Left` and `Ctrl+Alt+Right` move fullscreen between monitors.
- [x] `Ctrl+Alt+0-9` jumps to a monitor by index.
- [x] `Ctrl+Alt+P` jumps to the primary monitor.
- [ ] Brief visual feedback shows the selected target monitor.

### M2 acceptance criteria

- [ ] Accurate enumeration on 2 to 4 monitor setups.
- [ ] Smooth movement between monitors without visible artefacts.
- [ ] Correct behaviour on mixed-DPI setups.
- [ ] Persistent monitor preferences across sessions.
- [ ] Clear error messages for invalid selections.

### M2 manual QA matrix

| Configuration | Enumeration | Selection | Hotkeys | Mixed DPI |
| --- | --- | --- | --- | --- |
| Dual 1080p | ✓ | ✓ | ✓ | N/A |
| Dual mixed DPI | ✓ | ✓ | ✓ | ✓ |
| Triple setup | ✓ | ✓ | ✓ | ✓ |
| Portrait mode | ✓ | ✓ | ✓ | ✓ |

### M2 stretch goals

- [ ] Remember the last-used monitor per connection.
- [ ] Track logical monitor positioning for navigation.
- [ ] Document multi-monitor span behaviour while keeping implementation
  deferred.

## M3 PersistentCache protocol parity

**Priority:** High.

**Timeline:** 1 to 2 weeks after M2.

**Goal:** Persistent, hash-based caching for cross-session and cross-server
reuse.

### M3 overview

The current C++ TigerVNC cache implementation has moved beyond the older split
ContentCache-versus-PersistentCache design and now centres on a unified cache
path driven by PersistentCache negotiation and message handling. The Rust viewer
still reflects the older split model, so this milestone is about converging on
the current C++ behaviour rather than just re-implementing the older design
notes.

### M3 expected outcomes

- Cross-session persistence so cache entries survive client restarts.
- Cross-server hits when different servers render the same content.
- Meaningful bandwidth reduction for repeated content.
- Convergence on the current C++ unified-cache model, including clearer
  documentation of where the Rust code still lags behind.

### M3 protocol foundation checklist

- [ ] Add pseudo-encoding `-321` and encodings `102` and `103`.
- [ ] Add protocol message support for `PersistentCacheQuery` (`254`) and
  `PersistentHashList` (`253`).
- [ ] Match the C++ content hashing behaviour using SHA-256 truncated to
  16 bytes.
- [ ] Implement the variable-length wire format used for hash transmission.

### M3 cache storage checklist

- [ ] Implement `GlobalClientPersistentCache` with hash-indexed storage and ARC
  eviction.
- [ ] Track ARC state using `T1`, `T2`, `B1`, and `B2` lists.
- [ ] Enforce byte-accurate capacity limits.
- [ ] Record hits, misses, evictions, and cache-size statistics.

### M3 client protocol integration checklist

- [ ] Add `PersistentCachedRect` (`102`) and `PersistentCachedRectInit` (`103`)
  decoders.
- [ ] Batch cache misses efficiently.
- [ ] Synchronise the initial set of known hashes.
- [ ] Converge negotiation and cache wiring on the current C++ unified-cache
  model rather than treating ContentCache and PersistentCache as separate peer
  capabilities.

### M3 disk persistence checklist

- [ ] Persist the cache under the user cache directory.
- [ ] Load on startup and save on shutdown.
- [ ] Recover cleanly from corrupt cache files.
- [ ] Validate persisted content with checksums.

### M3 implementation phases

| Phase | Estimated duration | Description |
| --- | --- | --- |
| PC-1 | 0.5 day | Protocol constants and content-hash utility |
| PC-2 | 2 days | `GlobalClientPersistentCache` with ARC bookkeeping |
| PC-3 | 1 day | Client protocol messages and decoders |
| PC-4 | 1 day | Integration and negotiation |
| PC-5 | 1 to 2 days | Disk persistence |
| PC-6 | 1 day | Testing and validation |

### M3 dependencies

```toml
sha2 = "0.10"
byteorder = "1"
indexmap = "2"
directories = "5"
```

### M3 acceptance criteria

- [ ] Protocol negotiation prefers PersistentCache when both cache protocols
  are available.
- [ ] Hash computation matches the C++ `ContentHash::computeRect` behaviour.
- [ ] ARC eviction maintains the configured size limit.
- [ ] Disk persistence survives restarts without data loss.
- [ ] Cross-session hits are verified with the end-to-end test framework.
- [ ] Hashing and disk persistence meet the target latency budget.

### M3 testing strategy

- **Unit tests:** Hash computation, ARC eviction, and disk round trips.
- **Integration tests:** Mock server flows for encodings `102` and `103`.
- **Cross-session tests:** Connect, disconnect, reconnect, and verify cache
  hits.
- **Performance checks:** Benchmark hashing and cache I/O.
- **Safety note:** Continue to respect the project guidance for the dedicated
  end-to-end test displays.

### M3 critical PersistentCache gotchas

- **Stride units:** Stride is measured in pixels, not bytes; multiply by bytes
  per pixel when walking rows.
- **Hash length:** PersistentCache uses 16-byte identifiers on the wire.
- **Negotiation model:** current Rust code still advertises `-321` before
  `-320`, but parity work should target the newer unified C++ cache model
  rather than preserving the older split design indefinitely.
- **ARC accounting:** Capacity limits must be tracked in bytes, not entry
  count.

### M3 related documentation

- C++ and fork design notes: `../../docs/PERSISTENTCACHE_DESIGN.md`
- Rust viewer PersistentCache notes: `protocol/persistent_cache.md`
- ARC algorithm background: `../../docs/ARC_ALGORITHM.md`

## M4 windowed UX polish

**Priority:** Medium.

**Timeline:** After M3.

**Goal:** A smoother day-to-day windowed experience.

### M4 feature checklist

- [ ] Remember window size and position per connection.
- [ ] Choose better default initial window sizes.
- [ ] Optional minimise-to-tray support.
- [ ] Recent connections and favourites.
- [ ] Connection quality and latency indicators.
- [ ] Theme preference support.

## M5 advanced features

**Priority:** Low.

**Timeline:** After M4.

**Goal:** Power-user features and further optimisation.

### M5 feature checklist

- [ ] Detailed performance monitoring.
- [ ] Save and load connection profiles.
- [ ] Per-connection encoding preferences.
- [ ] TLS and certificate-management enhancements.
- [ ] Accessibility improvements.

## Out-of-scope features

The following items remain explicitly out of scope in line with `SEP-0001`.

### Permanent exclusions

- Touch and gesture support.
- GUI settings and profile editors.
- Built-in screenshot functionality.

### Rationale

These items add complexity without enough value for the desktop-first viewer
goal. CLI configuration and host-operating-system tooling remain the preferred
solutions.

## Dependencies and risks

### Technical dependencies

- `winit` for window and monitor management.
- `egui` and `eframe` for the GUI.
- Ongoing X11 and Wayland compatibility work.

### Risk mitigation

- Test X11 and Wayland paths explicitly.
- Keep fallback behaviour for unsupported monitor APIs.
- Handle unusual or changing monitor layouts gracefully.

### Cross-cutting test strategy

- Unit tests for monitor selection and scaling behaviour.
- Integration tests for fullscreen transitions and monitor movement.
- Manual QA on real hardware.
- End-to-end compatibility checks against the supported test displays.

## Timeline summary

| Milestone | Estimated duration | Main dependency | Status |
| --- | --- | --- | --- |
| M0 | Complete | None | Complete |
| M1 | 1 to 2 weeks | Monitor and fullscreen work | Next |
| M2 | 1 to 2 weeks | M1 complete | Planned |
| M3 | 1 to 2 weeks | M2 complete | Planned |
| M4 | 2 to 3 weeks | M3 complete | Future |
| M5 | TBD | M4 complete | Future |

**Estimated total for M1 to M3:** 3 to 6 weeks.

## Success metrics

### M1 success criteria

- [ ] Fullscreen works reliably on single-monitor systems.
- [ ] Common desktop environments show no major fullscreen regressions.
- [ ] User feedback confirms smooth behaviour.

### M2 success criteria

- [ ] Multi-monitor users can select and switch monitors easily.
- [ ] Hotkey navigation feels responsive and predictable.
- [ ] Mixed-DPI environments behave correctly.

### M3 success criteria

- [ ] PersistentCache is fully functional.
- [ ] Cross-session persistence is verified through restart testing.
- [ ] Hashing and persistence meet the agreed performance budget.
- [ ] Cache interoperability with the C++ viewer is confirmed.

### Overall project success criteria

- [ ] The Rust viewer reaches practical feature parity with the C++ viewer for
  desktop workflows.
- [ ] Caching delivers measurable performance wins.
- [ ] The viewer is credible for day-to-day desktop VNC use.

---

**Related documents:** [SEP-0001 Out-of-Scope](SEP/SEP-0001-out-of-scope.md),
[CLI Usage](cli/USAGE.md), and
[Fullscreen & Multi-Monitor Spec](spec/fullscreen-and-multimonitor.md).
