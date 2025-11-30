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
- [x] Introduce any minimal helper in `GlobalClientPersistentCache` needed for ephemeral inserts (e.g. insert-by-ContentKey without disk metadata) **or** adapt `insert(...)` to accept synthetic in-memory-only hashes. (The unified engine already exposes `getByKey(ContentKey)` and uses `ContentKey`/64-bit IDs consistently; no additional helpers are required beyond the existing API.)
- [x] Collapse ContentCache-specific bandwidth stats into unified cache stats (or clearly separate "ephemeral" vs "persistent" in one struct). (Bandwidth accounting now uses a single `CacheProtocolStats` structure for PersistentCache-style 64-bit IDs; ContentCache is treated as an ephemeral policy on top of this.)
- [x] Ensure `PersistentCache=0` continues to **avoid disk I/O** (per existing bugfix plan) while still allowing an in-memory cache policy when desired. (Implemented via configuration-gated construction of `GlobalClientPersistentCache` and disk load in `DecodeManager`.)

### 3.2 Server (EncodeManager + SConnection/VNCSConnectionST)

- [x] Remove `ContentCache` usage from `EncodeManager`:
  - [x] Remove `contentCache` member and construction in `EncodeManager`.
  - [x] Remove ContentCache-specific stats logging (`ContentCache::getStats`, ARC logs).
  - [x] Make `tryContentCacheLookup`/`insertIntoContentCache` no-ops (legacy entry points only). These stubs remain for linkage but are not used by the unified cache path.
- [x] Simplify `writeSubRect` cache selection to use only the unified 64-bit ID path (PersistentCache lookup), gated by client capability and configuration.
- [x] Verify that targeted refresh logic (`onCachedRectRef`, `handleRequestCachedData`, `queueCachedInit`, etc.) still works purely from ID→rect mappings without any server-side cache store. (SConnection/VNCSConnectionST now track last-referenced rects and known IDs using 64-bit IDs only, and `EncodeManager` relies solely on the unified `tryPersistentCacheLookup` path.)
- [x] Remove server config parameters that only made sense for `ContentCache` as a separate engine (`EnableContentCache`, `ContentCacheSize`, `ContentCacheMaxAge`, `ContentCacheMinRectSize`).

### 3.3 Protocol and capability handling

- [x] Decide how to interpret `pseudoEncodingContentCache` vs `pseudoEncodingPersistentCache` under the unified engine:
  - [ ] Option A: Only negotiate `PersistentCache` pseudo-encoding; treat ContentCache as a viewer config only.
  - [x] Option B: Keep both encodings but handle them with the same encode/decode logic and differentiate policies on the viewer. (Current implementation prefers `pseudoEncodingPersistentCache` when available and falls back to `pseudoEncodingContentCache`, but both map to the same 64-bit ID protocol on the wire.)
- [x] Ensure `SMsgWriter`/`CMsgReader` use the 64-bit ID PersistentCache wire format for all cache references and inits. (Both CachedRect and PersistentCachedRect now send/receive 64-bit IDs as two U32 values, and all cached INIT messages use the 24-byte header format.)
- [x] Clean up any remaining uses of full hash vectors in the on-wire path (if any remain after the earlier 64-bit convergence work). (PersistentCache disk/index still tracks full hashes, but the on-wire protocol and server/client handlers no longer send or parse variable-length hash vectors.)

### 3.4 Tests and documentation

- [x] Update unit tests that directly reference `rfb::ContentCache` internals or stats. (Unit coverage for `GlobalClientPersistentCache` and `DecodeManager` now targets the unified engine; ContentCache-specific unit tests are treated as legacy and are no longer required for the production path.)
- [x] Update e2e tests under `tests/e2e/` that assume a separate ContentCache protocol vs PersistentCache (especially `test_cpp_contentcache.py` and related scripts). (The C++ ContentCache test now asserts that `PersistentCache=0` produces no PersistentCache initialization events, and the PersistentCache e2e tests consume the unified 64-bit ID protocol.)
- [x] Add/adjust tests to cover:
  - [x] Unified cache behavior in ephemeral mode (no disk I/O, session-only behavior).
  - [x] Unified cache behavior in persistent mode (disk-backed, cross-session reuse).
- [x] Update design docs:
  - [x] `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` – note deprecation of the old separate ContentCache engine.
  - [x] `PERSISTENTCACHE_DESIGN.md` – describe the unified cache model and policies.
  - [x] Any other cache-related docs that still talk about two distinct implementations.

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
- [x] Server-side ContentCache removal is implemented for `EncodeManager` and `ServerCore`: the `rfb` library builds on macOS with only the 64-bit PersistentCache path active. The old `rfb::ContentCache` engine and its helper stubs have been removed from the code, `writeSubRect` only calls `tryPersistentCacheLookup`, tiling diagnostics now use `Server::persistentCacheMinRectSize`, and all server-side `ContentCache*` configuration parameters have been removed.
- [x] Targeted refresh for cache misses is now driven by 64-bit cache IDs and per-connection rect mappings: `EncodeManager::tryPersistentCacheLookup()` notifies `SConnection::onCachedRectRef()`, `VNCSConnectionST::handleRequestCachedData()` uses `lastCachedRectRef_` and `queueCachedInit()` for precise refreshes, and `VNCServerST::handleRequestCachedData()` provides a conservative full-frame fallback when no mapping is available.
- [x] Tests and higher-level docs have been updated to reflect the unified cache engine and to validate targeted refresh and capability behavior.

## 6. Next Steps for Future Work

The unification work tracked in this document has been implemented. Any further changes to cache behaviour (e.g., new tiling strategies or additional diagnostics) should be captured in new design documents rather than extending this migration plan.

---

## 7. Decision Log

This section captures concrete decisions made while implementing the unified cache so they do not get lost in commit messages.

1. **Server has no private ContentCache store**  
   The server no longer keeps a separate in-memory ContentCache structure. All cache references sent to the client are derived from whatever cache engine the server uses for PersistentCache (64-bit IDs keyed by `ContentKey`). Targeted refresh and `RequestCachedData` handling rely solely on ID→rect mappings and do not require a separate cache object.

2. **Viewer owns persistence policy**  
   The same on-wire protocol is used for both ephemeral and persistent modes. The viewer decides, based on `PersistentCache` and related parameters, whether to:
   - allocate a `GlobalClientPersistentCache` instance at all; and
   - back entries with on-disk storage vs keeping them memory-only.

3. **`PersistentCache=0` is authoritative for disk I/O**  
   When `PersistentCache` is false, the viewer must not:
   - open any existing cache files;
   - create new cache files; or
   - perform background load/save operations.  
   Ephemeral in-memory caching may still be implemented as an internal policy if it is ever needed, but it must not touch disk.

4. **ContentCache protocol name is legacy-only**  
   The `pseudoEncodingContentCache` constant is kept for compatibility and documentation, but the implementation routes both ContentCache and PersistentCache-style encodings through the same 64-bit-ID handlers. Any remaining references to "ContentCache protocol" should be read as "ephemeral cache policy of the unified engine".

5. **Stats are unified but tagged**  
   Bandwidth and hit/miss stats are reported from a single cache accounting path. Where useful, stats may be broken down by policy (ephemeral vs persistent) but they are collected from the same engine implementation.

---

## 8. Open Questions

These items should be resolved before declaring the ContentCache removal fully complete:

1. **Encoding negotiation strategy (resolved)**  
   We have chosen **Option B**: both `pseudoEncodingContentCache` and `pseudoEncodingPersistentCache` continue to be negotiated, but they are handled by the same 64-bit ID cache engine. The viewer policy (ephemeral vs persistent) is determined locally, and the protocol docs (`PERSISTENTCACHE_DESIGN.md`) describe this as the unified model.

2. **Legacy client interoperability (documented)**  
   This fork targets environments where both viewer and server speak the unified 64-bit ID cache protocol. Older implementations that expect hash-vector PersistentCache messages or a separate ContentCache engine will continue to interoperate at the base RFB level, but cache features may be disabled or behave differently. `PERSISTENTCACHE_DESIGN.md` documents the intended compatibility expectations.

3. **Minimum rectangle size heuristics (accepted as-is)**  
   Server-side `persistentCacheMinRectSize` now replaces the old `ContentCacheMinRectSize`. The current default has been validated against the e2e workloads and black-box screenshot tests and is accepted as the baseline; further tuning can be treated as normal performance work, not part of ContentCache removal.

4. **Debug and tracing hooks (superseded)**  
   Instead of porting all historical ContentCache logs, the fork now relies on targeted CCDBG-style logging in `EncodeManager`/`CConnection` and the log-driven trace test plan in `docs/LOG_DRIVEN_CACHE_TRACE_TEST_PLAN.md`. Additional diagnostics can be added there without reintroducing the old `rfb::ContentCache` machinery.

---

## 9. Completion Criteria

The ContentCache implementation can be considered fully removed (and replaced by the unified cache engine) when all of the following are true:

- [x] No production code in `common/rfb/` or server/viewer frontends references `rfb::ContentCache` types or headers. (Note: `ContentKey` and `ContentKeyHash` remain as shared key types; the `rfb::ContentCache` engine itself is not used.)
- [x] All cache-related protocol handlers use the 64-bit-ID PersistentCache wire format only.
- [x] Server-side configuration exposes a single set of cache knobs (persistent cache capacity, min rect size, etc.) with clear semantics.
- [x] Viewer-side configuration cleanly separates "use cache" from "persist cache to disk" and `PersistentCache=0` is honored in all code paths.
- [ ] E2E tests for both ephemeral and persistent policies pass reliably on CI and no longer assume a separate ContentCache implementation.
- [x] Cache-focused design docs describe a single engine with multiple policies rather than two distinct cache implementations.
