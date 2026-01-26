
# Known Bugs

## 1. PersistentCache option does not fully disable client-side PersistentCache

### 1. Summary

Historically the viewer-side `PersistentCache` parameter disabled advertising the
PersistentCache protocol capability to the server but **did not** prevent the
client from initializing and loading the persistent cache from disk. As a
result, `-PersistentCache=0` did not actually disable all PersistentCache
behaviour.

As of 2025-11-19, the C++ viewer now gates all client-side PersistentCache
initialization on this parameter. When `PersistentCache=0`, no client-side
PersistentCache instance is constructed and the on-disk cache file is never
opened or read.

### 1. Evidence / Reproduction

- Command line:

  - `njcvncviewer -PersistentCache=0 localhost:2`

- Viewer still logs PersistentCache initialization and disk loading, for example:

  - `PersistentCache: PersistentCache created with ARC: maxSize=2048MB,
    path=~/.cache/tigervnc/persistentcache.dat`

  - `PersistentCache: header valid, loading 69134 entries (2147419492 bytes)`

- The viewer remains busy and unresponsive during this load, despite
  `PersistentCache=0`.

### 1. Impact

- Users cannot reliably disable PersistentCache client-side behaviour.
- Large or corrupt cache files can hang or delay viewer startup.

### 1. Expected Behaviour

- When `PersistentCache=0`, the client should:

  - Not initialize PersistentCache structures
  - Not open or read the disk cache
  - Not emit PersistentCache-related log lines

### 1. Implementation Notes / Hints

- Gate **all** PersistentCache initialization on the viewer parameter.
- Ensure later code paths tolerate a null cache.
- Implemented in the C++ viewer by gating
  `rfb::DecodeManager::GlobalClientPersistentCache`.

---

## 2. PersistentCache is loaded before protocol negotiation with the server

### 2. Summary

The viewer loads the PersistentCache from disk during startup **before**
confirming that the server supports the PersistentCache protocol. This causes
unnecessary work when connecting to servers that do not support PersistentCache.

### 2. Evidence / Reproduction

- Connect to a server without PersistentCache enabled.
- Observe logs such as:

  - `PersistentCache: loading from ~/.cache/tigervnc/persistentcache.dat`
  - `PersistentCache: header valid, loading ... entries (...)`

### 2. Impact

- Wasted startup time and disk I/O.
- Unused memory allocation for cache data.

### 2. Expected Behaviour

- Load PersistentCache only **after** successful RFB capability negotiation.
- Skip cache loading entirely if the server does not negotiate support.

### 2. Implementation Notes / Hints

- Move cache loading to post-negotiation.
- Ensure cache remains inactive when negotiation fails.

---

## 3. PersistentCache disk loading blocks the main viewer thread

### 3. Summary

Large PersistentCache files are loaded synchronously on the main thread. During
this time, the UI is unresponsive and the application may appear hung.

### 3. Impact

- Poor user experience during startup.
- macOS may report the app as “Not Responding”.

### 3. Desired Behaviour / High-level Design

1. **Lazy rehydration**

   - Load lightweight metadata first.
   - Defer heavy payload loading.

2. **Background loading**

   - Perform disk I/O off the main thread.
   - Synchronize access safely.

3. **On-demand hydration**

   - Prioritise entries required for the current session.

### 3. Implementation Notes / Hints

- Separate index loading from payload loading.
- Introduce cache states:

  - `Uninitialized → IndexLoaded → PartiallyHydrated → FullyHydrated`

### 3. Acceptance Criteria

- Viewer remains responsive during startup.
- No disk I/O when `-PersistentCache=0`.
- No cache loading when the server does not support it.

---

## 4. Viewer bandwidth statistics differ from server-calculated values

### 4. Status

RESOLVED (2025-11-27)

### 4. Summary

Viewer-reported bandwidth reduction differed from server-calculated values used
by E2E tests.

### 4. Resolution

Viewer statistics now match server-side calculations.

- `test_persistent_cache_bandwidth.py` reports ~98.4% reduction
- Matches server-derived metrics

---

## 5. E2E test log parser incorrectly calculates cache hit rate

### 5. Status

RESOLVED (2025-11-27)

### 5. Summary

`log_parser.py` treated `CachedRectInit` messages as cache misses even during
initial population.

### 5. Resolution

- Viewer-reported stats are now preferred when available.
- `CachedRectInit` during population is no longer treated as a miss.

---

## 6. Dark rectangle corruption after viewport resize

### 6. Status

PARTIALLY RESOLVED (2025-12-06)

**Investigation document**: `VIEWPORT_RESIZE_CORRUPTION_INVESTIGATION.md`

### 6. Summary

Dark rectangular artifacts appear after resizing viewer windows near old
framebuffer boundaries.

### 6. Current Status

**76% reduction in corruption achieved**.

Fixed issues:

1. ✅ Bottom strip width bug
2. ✅ Uninitialized Pixmap
3. ✅ Missing XSync

### 6. Remaining Issue

- ~0.01% pixel mismatch remains
- Likely server size mismatch or race conditions

### 6. Test

```bash

cd tests/e2e
python3 test_dark_rect_corruption.py --mode none --lossless
