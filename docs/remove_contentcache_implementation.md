# Remove ContentCache Implementation – Tracking

Status: **In progress**  
Owner: experimental fork (server + C++ viewer)

This document tracks the hard cutover from separate ContentCache and PersistentCache implementations to a **single cache protocol/engine** based on 64-bit IDs and `ContentKey` (width, height, hash64).

The end state is:

- Only one cache implementation on both server and viewer.
- ContentCache protocol becomes an **ephemeral policy** of the unified cache:
  - Same wire format and IDs as PersistentCache.
  - Viewer does **not** read/write cache state to disk when in "ContentCache mode".
- PersistentCache remains the same cache engine, but with disk persistence enabled.

---

## 1. Invariants and Constraints

- Cache identity is always `ContentKey(width, height, contentHash64)`.
- 64-bit `contentHash64` is derived from `ContentHash::computeRect` (first 8 bytes).
- There is **no** separate `rfb::ContentCache` class in the final state.
- Viewer must support two policies using the same engine:
  - **Ephemeral** (memory-only, ContentCache-like).
  - **Persistent** (memory+disk, PersistentCache-like).
- Tiling, stats, and protocol helpers operate on the unified cache model.

---

## 2. High-Level Plan

1. **Viewer-side unification**
   - Remove `ContentCache` member and related logic from `DecodeManager`.
   - Route `CachedRect` / `CachedRectInit` handling through `GlobalClientPersistentCache` (ephemeral mode when persistence is disabled).
   - Ensure `PersistentCache=0` truly disables disk I/O but still allows ephemeral cache if desired.

2. **Server-side unification**
   - Remove `ContentCache` implementation and all direct usage from `EncodeManager` and friends.
   - Use a single 64-bit ID cache path (today's PersistentCache path) for all cache references.
   - Keep `knowsPersistentId`, `markPersistentIdKnown`, etc. as the only session knowledge API.

3. **Protocol and capability cleanup**
   - Treat `pseudoEncodingContentCache` and `pseudoEncodingPersistentCache` as two **policies** of the same protocol:
     - Both use the 64-bit ID wire format already implemented for PersistentCache.
     - The viewer decides whether to persist to disk based on its configuration.
   - Update any remaining message comments/docs that still assume hash-vectors or separate cache semantics.

4. **Tests and docs**
   - Update e2e tests that currently assume a separate ContentCache engine.
   - Trim or port unit tests that were specific to `rfb::ContentCache` internals.
   - Document the new unified model in existing cache design docs.

---

## 3. Detailed Checklist

### 3.1 Viewer (DecodeManager + client cache)

- [x] Remove `ContentCache* contentCache` from `DecodeManager` (ctor, dtor, members).
- [x] Delete or rewrite `handleCachedRect` to use `GlobalClientPersistentCache` via `ContentKey` (now forwards to `handlePersistentCachedRect`).
- [x] Delete or rewrite `storeCachedRect` to insert into `GlobalClientPersistentCache` (now forwards to `storePersistentCachedRect`).
- [ ] Introduce any minimal helper in `GlobalClientPersistentCache` needed for ephemeral inserts (e.g. insert-by-ContentKey without disk metadata) **or** adapt `insert(...)` to accept synthetic in-memory-only hashes.
- [ ] Collapse ContentCache-specific bandwidth stats into unified cache stats (or clearly separate "ephemeral" vs "persistent" in one struct).
- [x] Ensure `PersistentCache=0` continues to **avoid disk I/O** (per existing bugfix plan) while still allowing an in-memory cache policy when desired. (Implemented via configuration-gated construction of `GlobalClientPersistentCache` and disk load in `DecodeManager`.)

### 3.2 Server (EncodeManager + SConnection/VNCSConnectionST)

- [x] Remove `ContentCache` usage from `EncodeManager`:
  - [x] Remove `contentCache` member and construction in `EncodeManager`.
  - [x] Remove ContentCache-specific stats logging (`ContentCache::getStats`, ARC logs).
  - [x] Make `tryContentCacheLookup`/`insertIntoContentCache` no-ops (legacy entry points only). These stubs remain for linkage but are not used by the unified cache path.
- [x] Simplify `writeSubRect` cache selection to use only the unified 64-bit ID path (PersistentCache lookup), gated by client capability and configuration.
- [ ] Verify that targeted refresh logic (`onCachedRectRef`, `handleRequestCachedData`, `queueCachedInit`, etc.) still works purely from ID→rect mappings without any server-side cache store. (EncodeManager now relies solely on `tryPersistentCacheLookup`; SConnection/VNCSConnectionST should be audited next.)
- [x] Remove server config parameters that only made sense for `ContentCache` as a separate engine (`EnableContentCache`, `ContentCacheSize`, `ContentCacheMaxAge`, `ContentCacheMinRectSize`).

### 3.3 Protocol and capability handling

- [ ] Decide how to interpret `pseudoEncodingContentCache` vs `pseudoEncodingPersistentCache` under the unified engine:
  - [ ] Option A: Only negotiate `PersistentCache` pseudo-encoding; treat ContentCache as a viewer config only.
  - [ ] Option B: Keep both encodings but handle them with the same encode/decode logic and differentiate policies on the viewer.
- [ ] Ensure `SMsgWriter`/`CMsgReader` use the 64-bit ID PersistentCache wire format for all cache references and inits.
- [ ] Clean up any remaining uses of full hash vectors in the on-wire path (if any remain after the earlier 64-bit convergence work).

### 3.4 Tests and documentation

- [ ] Update unit tests that directly reference `rfb::ContentCache` internals or stats.
- [ ] Update e2e tests under `tests/e2e/` that assume a separate ContentCache protocol vs PersistentCache (especially `test_cpp_contentcache.py` and related scripts).
- [ ] Add/adjust tests to cover:
  - [ ] Unified cache behavior in ephemeral mode (no disk I/O, session-only behavior).
  - [ ] Unified cache behavior in persistent mode (disk-backed, cross-session reuse).
- [ ] Update design docs:
  - [ ] `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` – note deprecation of the old separate ContentCache engine.
  - [ ] `PERSISTENTCACHE_DESIGN.md` – describe the unified cache model and policies.
  - [ ] Any other cache-related docs that still talk about two distinct implementations.

---

## 4. Notes and Gotchas

- The original ContentCache bugfix that introduced `ContentKey(width, height, contentHash64)` **must remain intact**. The current PersistentCache implementation already uses `ContentKey` and width/height for in-memory keying; the unified engine should preserve this.
- When removing `ContentCache`, be careful to keep any targeted refresh and debug/logging hooks that are still useful for diagnostics (e.g. mapping cache IDs back to last referenced rects, tiling logs, etc.).
- Tiling logic (`TilingAnalysis`, `TilingIntegration`) is already agnostic to the specific cache backend; it should require minimal or no changes.

---

## 5. Current Status

- [x] Verified that PersistentCache (`GlobalClientPersistentCache`) already uses `ContentKey(width, height, hash64)` for in-memory keying and that the width/height bugfix is fully applied.
- [x] Created this tracking document.
- [x] Viewer-side removal of `ContentCache` for the C++ viewer is complete at the `DecodeManager` layer: all legacy `CachedRect` entry points now delegate to the unified `GlobalClientPersistentCache` engine, with disk I/O gated by the `PersistentCache` parameter.
- [x] Server-side ContentCache removal is implemented for `EncodeManager` and `ServerCore`: the `rfb` library builds on macOS with only the 64-bit PersistentCache path active. `tryContentCacheLookup`/`insertIntoContentCache` are no-op stubs, `writeSubRect` only calls `tryPersistentCacheLookup`, tiling diagnostics now use `Server::persistentCacheMinRectSize`, and all server-side `ContentCache*` configuration parameters have been removed.
- [ ] Tests and higher-level docs still need to be updated to reflect the unified cache engine and to validate targeted refresh and capability behavior.

## 6. Next Steps for Future Work

- Audit `SConnection` and `VNCSConnectionST` to ensure all cache-related paths (especially targeted refresh and `RequestCachedData` handling) operate purely on 64-bit IDs and rect mappings, with no assumptions about a server-side `ContentCache` store.
- Decide final semantics for `pseudoEncodingContentCache` vs `pseudoEncodingPersistentCache` under the unified engine and adjust negotiation/handlers accordingly.
- Run and update the e2e cache tests (`test_cpp_contentcache.py`, `test_cpp_persistentcache.py`, and any others touching cache behavior) to match the new unified protocol, and add coverage for ephemeral vs persistent viewer policies.
- Sweep the codebase for any remaining `ContentCache` references in comments or dead code and either remove them or clearly annotate them as legacy notes.
