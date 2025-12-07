# Known Bugs

## 1. PersistentCache option does not fully disable client-side PersistentCache

### Summary
Historically the viewer-side `PersistentCache` parameter disabled advertising the PersistentCache protocol capability to the server but **did not** prevent the client from initializing and loading the persistent cache from disk, so `-PersistentCache=0` did not actually disable all PersistentCache behaviour.

As of 2025-11-19, the C++ viewer now gates all client-side PersistentCache initialization on this parameter: when `PersistentCache=0`, no client-side PersistentCache instance is constructed and the on-disk cache file is never opened or read.

### Evidence / Reproduction
- Command line: `njcvncviewer -PersistentCache=0 localhost:2`
- Viewer still logs PersistentCache initialization and disk loading, for example:
  - `PersistentCache: PersistentCache created with ARC: maxSize=2048MB, path=~/.cache/tigervnc/persistentcache.dat`
  - `PersistentCache: header valid, loading 69134 entries (2147419492 bytes)`
- The viewer process remains busy and unresponsive during this load, even though the user explicitly requested `PersistentCache=0`.

### Impact
- Users cannot reliably disable PersistentCache client-side behaviour for debugging or resource-constrained environments.
- Large or corrupt `persistentcache.dat` files can hang or significantly delay viewer startup even when the protocol is nominally disabled.

### Expected Behaviour
- When `PersistentCache` is set to `0` (or equivalent "off" forms), the client should:
  - Not initialize any in-memory PersistentCache structures.
  - Not open or read the on-disk persistent cache file.
  - Not allocate memory for PersistentCache, and not emit PersistentCache-related log lines.

### Implementation Notes / Hints
- Identify the configuration plumbing for the `PersistentCache` viewer parameter and ensure it gates **all** PersistentCache initialization paths on the client.
- Early in viewer startup, after parsing parameters, add a guard such that if `PersistentCache` is disabled, no PersistentCache object is constructed and no disk I/O is attempted for it.
- Ensure any later code paths that assume a PersistentCache instance are null-checked or otherwise conditioned on the option being enabled.
- Implemented for the C++ viewer by gating `rfb::DecodeManager`'s `GlobalClientPersistentCache` construction and `loadFromDisk()` calls on the `PersistentCache` boolean parameter, leaving `persistentCache` null when disabled.

---

## 2. PersistentCache is loaded before protocol negotiation with the server

### Summary
The viewer eagerly loads the PersistentCache file from disk during startup, **before** confirming that the server supports and negotiates the PersistentCache protocol. This results in unnecessary work when connecting to servers that do not support (or have disabled) PersistentCache.

### Evidence / Reproduction
- Start the viewer against a server where PersistentCache is not enabled or not implemented.
- Observe in the logs that the viewer still prints lines like:
  - `PersistentCache: PersistentCache: loading from ~/.cache/tigervnc/persistentcache.dat`
  - `PersistentCache: header valid, loading ... entries (...)`
- This happens early in the client lifecycle, before any confirmation that the server will ever send PersistentCache messages.

### Impact
- Wasted startup time and disk I/O when the server will never use PersistentCache.
- Unnecessary memory consumption for cache data that is never referenced during the session.

### Expected Behaviour
- The viewer should defer PersistentCache loading until **after** RFB capability negotiation confirms that both client and server have enabled PersistentCache.
- If the server does not negotiate PersistentCache, the viewer should skip opening and loading the persistent cache file entirely for that session.

### Implementation Notes / Hints
- Locate the point in the RFB negotiation where encodings / capabilities (including PersistentCache) are agreed upon.
- Move PersistentCache disk loading from early viewer startup into a later phase that runs only when PersistentCache has been successfully negotiated.
- Ensure that the code handles the case where negotiation fails or the server does not advertise PersistentCache: in those cases, PersistentCache must remain inactive and the disk file unopened.

---

## 3. PersistentCache disk loading is synchronous and blocks the main viewer thread

### Summary
When the persistent cache file is large (e.g., millions of entries and multi-GB size), the viewer performs a synchronous, eager load of the entire cache on the main thread. During this time, the UI remains unresponsive and no window may appear, leading macOS to report the application as "not responding".

The log shows, for example:
- `PersistentCache: header valid, loading 69134 entries (2147419492 bytes)`

After this point, the process is busy loading data and does not respond to user actions until the load completes or the process is terminated (e.g., SIGINT from the terminal).

### Impact
- Very poor user experience when the persistent cache grows large: the viewer appears hung on startup.
- On platforms like macOS, the Dock reports the app as "Application Not Responding" and there are no visible windows, even though the process is alive.
- Users cannot easily distinguish between a real hang and slow startup caused by cache loading.

### Desired Behaviour / High-level Design
1. **Lazy, staged cache rehydration**
   - On initial load, read only lightweight metadata / IDs needed to index the cache (e.g., mapping from hash/ID to offsets in the file), not the full rectangle data.
   - Defer loading of heavy payloads until the viewer actually needs them during a live session.

2. **Background loading**
   - Perform any substantial disk I/O for PersistentCache in a background thread or worker so that the main GUI / network thread stays responsive.
   - Ensure appropriate synchronization around cache data structures (e.g., mutexes, atomics, or other concurrency primitives) so that readers see consistent state.

3. **Prioritised, on-demand hydration**
   - When the server sends a PersistentCache ID for use in the current session before the cache is fully rehydrated, fetch and decode that entry first.
   - Continue to hydrate the rest of the cache lazily in the background, but do not block the main viewer processing loop on bulk loading.

### Implementation Notes / Hints
- Introduce a clear separation between:
  - Loading the **index/IDs** for existing cache entries (fast, needed early).
  - Loading the **payload data** for entries (slow, can be deferred).
- Consider a small state machine for the PersistentCache:
  - `Uninitialized` -> `IndexLoaded` -> `PartiallyHydrated` -> `FullyHydrated`.
- Ensure that any calls that look up cache entries are safe to run while the cache is in a partially hydrated state:
  - If the ID is known but payload is not yet loaded, trigger a targeted load of that entry instead of treating it as a hard miss.
- Keep all heavy disk operations off the UI / main thread; only small bookkeeping operations should happen synchronously.

### Acceptance Criteria
- With a large `persistentcache.dat` (multi-GB), the viewer:
  - Creates a window promptly and remains responsive during and after startup.
  - Does not block the main thread for extended periods on cache loading.
- `-PersistentCache=0` fully disables both negotiation and any cache initialization / loading. (Done for the C++ viewer via `DecodeManager` gating on 2025-11-19.)
- When connecting to servers that do not support PersistentCache, the disk cache is never opened or loaded for that session.

---

## 4. Viewer bandwidth statistics differ from server-calculated values

**STATUS: RESOLVED (2025-11-27)**

### Summary
The C++ viewer emits bandwidth reduction statistics that may differ significantly from the server-side calculation. End users see viewer stats, but the e2e tests currently rely on server log parsing to get accurate bandwidth metrics.

### Resolution
The viewer now correctly reports bandwidth reduction percentages that match actual savings.
Recent test runs show:
- `test_persistent_cache_bandwidth.py` reports 98.4% reduction from viewer log
- This matches server-side calculations

The earlier 60.7% figure was likely from an older version or different test conditions.

---

## 5. E2E test log parser incorrectly calculates ContentCache hit rate

**STATUS: RESOLVED (2025-11-27)**

### Summary
The `log_parser.py` in `tests/e2e/` treats all `CachedRectInit` messages as cache misses, but some of these occur during initial cache population before any lookups happen. This causes the parser to report lower hit rates than the viewer's self-reported stats.

### Resolution
Fixed `compute_metrics()` in `log_parser.py` to prioritize viewer-reported stats when available.
The viewer reports accurate Lookups/Hits/Misses in its end-of-session summary, and these should
be trusted over protocol message counting. CachedRectInit is NOT a miss - it's initial population
before any lookups happen.

The parser now only falls back to counting CachedRectInit as "misses" when parsing server logs
that don't have viewer-side stats.

---

## 6. Dark rectangle corruption after viewport resize

**STATUS: PARTIALLY RESOLVED (2025-12-06)**  
**Investigation document**: `VIEWPORT_RESIZE_CORRUPTION_INVESTIGATION.md`

### Summary
When VNC viewer windows are resized (e.g., from 800×860 to 1600×1760), dark rectangular artifacts appear at the bottom-right boundary near the old framebuffer dimensions. The corruption manifests as dark gray pixels (~27,27,27) instead of expected content.

### Current Status
**76% reduction in corruption achieved** (from 1132 to 272 pixels).

Three critical bugs fixed:
1. ✅ **Bottom strip width bug**: `CConnection::setFramebuffer()` used new width instead of old width when blacking out bottom strip, overwriting valid copied pixels
2. ✅ **Uninitialized Pixmap**: New X11 Pixmaps contained garbage; now synced immediately after creation
3. ✅ **Missing XSync**: Non-SHM `XPutImage()` calls were missing synchronization

### Remaining Issue
272 pixels (0.01%) still differ between two identical viewers after resize. Investigation suggests:
- Server content size mismatch (server stays at 1600×900 while viewers resize to 1600×1760)
- Race condition in damage handling or screenshot capture
- Incomplete damage region coverage

### Test
```bash
cd tests/e2e
python3 test_dark_rect_corruption.py --mode none --lossless
```

### Next Steps
1. **HIGH**: Investigate what content the VNC server actually has at corruption coordinates
2. **MEDIUM**: Add forced rendering flush (`Fl::flush()`, `Fl::wait(0)`) after resize
3. **MEDIUM**: Verify damage region coverage with comprehensive logging

### Files Modified
- `common/rfb/CConnection.cxx:171` - Fixed bottom strip width
- `vncviewer/Viewport.cxx:428` - Sync new framebuffer Pixmap immediately
- `vncviewer/PlatformPixelBuffer.cxx:129` - Added XSync for non-SHM case
