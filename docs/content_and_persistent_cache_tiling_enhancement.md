# ContentCache & PersistentCache Tiling Enhancement

This guide describes a proposed design to add an **orthogonal tiling analysis layer** that works with both **ContentCache** (session-only) and **PersistentCache** (cross-session).

The goals are:
- Run **pre-decomposition cache checks** for large regions before the existing encoder pipeline splits them into solids/details.
- Use a **tiling + maximal-rectangle pass** to find large cacheable areas that can be served entirely from cache.
- Introduce a **policy for creating large cached rectangles** when we detect slide-like or otherwise repeated full-screen content.
- Keep the design **common between ContentCache and PersistentCache**, so both protocols benefit from the same tiling logic.

This document is intentionally high level and focused on architecture and APIs, not line-by-line code.

## 1. High-Level Concept

Today, the server-side pipeline in `EncodeManager` works roughly as:

1. Compute the changed region.
2. Apply CopyRect where possible.
3. Run `writeSolidRects()` to split off large solid-colour rects.
4. Run `writeRects()` to encode the remaining changed regions with Tight/ZRLE/etc.
5. For each rect, optionally attempt ContentCache or PersistentCache lookups.

This means cache decisions are made *after* the region has been decomposed into many small rectangles.

The proposed enhancement inserts an **orthogonal tiling analysis step** *before* steps 3–4 for large dirty regions:

1. For each sufficiently large dirty region, build a **logical tile grid** (e.g. 64×64 or 128×128 tiles).
2. For each tile, compute its content hash and ask the relevant cache(s) whether the tile is **cacheable** for this client.
3. Build a binary mask over tiles (1 = cacheable, 0 = not).
4. Use a **maximal-rectangle algorithm** on this mask to find large contiguous cacheable regions.
5. For each large cacheable region:
   - Prefer to send **cache-based updates** (CachedRect / PersistentCachedRect) instead of letting the standard pipeline decompose it.
   - Optionally, create **new large cached rectangles** (CachedRectInit / PersistentCachedRectInit) when we detect promising content (e.g. slide-sized regions).
6. For uncovered or non-cacheable tiles, fall back to the existing encoder pipeline.

The tiling logic itself is independent of which cache protocol is active; only the **per-tile lookup and Init/Ref emission** differ between ContentCache and PersistentCache.

## 2. Tiling Model

### 2.1 Tile Grid Definition

- Use a **configurable tile size**, e.g. 64×64 or 128×128 pixels.
  - Expose as server parameters, e.g.:
    - `ContentCacheTileSize` (default 128)
    - `PersistentCacheTileSize` (default 128)
- For a dirty region `R` (a `core::Rect`), define a grid of tiles aligned to `R.tl`:
  - Tile `(i, j)` covers:
    - `x = R.tl.x + i * tileSize`
    - `y = R.tl.y + j * tileSize`
  - Clip final tiles at `R.br` (partial tiles at boundaries are allowed).

### 2.2 Tile Classification

For each tile rect `T` within `R`:

1. Compute a **content hash** using `ContentHash::computeRect(pb, T)`.
2. Depending on which cache protocol is negotiated:
   - **ContentCache path**:
     - Build a `ContentKey(width=T.width(), height=T.height(), hash64)`.
     - Ask server-side ContentCache: `findByKey(key, &cacheId)`.
     - Check `conn->knowsCacheId(cacheId)` to ensure this client has been told about it.
   - **PersistentCache path**:
     - Use the full hash bytes from `ContentHash::computeRect`.
     - Ask `conn->knowsPersistentHash(hash)`.
3. Mark tile as:
   - `CACHE_HIT` if client already knows this content.
   - `CACHE_INIT_CANDIDATE` if it exists server-side but client does not yet know it, or if we want to seed it.
   - `NOT_CACHEABLE` otherwise.

For the 2D mask used by the maximal-rectangle pass, we only need a boolean **cacheable** flag:

- `mask[i][j] = 1` if tile is `CACHE_HIT`.
- `mask[i][j] = 0` otherwise.

`CACHE_INIT_CANDIDATE` tiles still participate in policy for seeding large cached rectangles, but do not count as hits for the current frame.

## 3. Maximal Rectangle Search

Given the binary mask `mask[w][h]` over tiles in region `R`:

- We want to find large axis-aligned rectangles of 1s.
- Classic algorithm: largest rectangle in a binary matrix in O(W×H) using a stack-based histogram approach.
  - For each row `j`, treat `mask[*, j]` as the base of a histogram of consecutive 1s above.
  - Use the standard "largest rectangle in histogram" stack algorithm per row to detect all maximal rectangles.

For our case, we likely want:

- **Thresholds**:
  - Minimum tile area: e.g. `minTiles = 4` (2×2 tiles) or equivalently `minPixelArea`.
  - Possibly a maximum aspect ratio to avoid long skinny rects that are less beneficial.
- **Output rectangles** in framebuffer space:
  - For a tile-rectangle spanning tiles `(i0..i1, j0..j1)`, the pixel rect is:
    - `x1 = R.tl.x + i0 * tileSize`
    - `y1 = R.tl.y + j0 * tileSize`
    - `x2 = R.tl.x + (i1 + 1) * tileSize` (clipped to `R.br.x`)
    - `y2 = R.tl.y + (j1 + 1) * tileSize` (clipped to `R.br.y`)

Implementation notes:

- Put the rectangle-finding algorithm in a **shared utility** in `common/rfb/cache/` so both ContentCache and PersistentCache can use it.
- Keep all tile indices and masks in small fixed-size structures or `std::vector` to avoid heap churn.

## 4. Shared Abstractions

Introduce a small set of shared types for tiling, e.g. in `common/rfb/cache/TilingAnalysis.h`:

- `struct TileRect` — pixel-space rect aligned to the tile grid.
- `enum class TileCacheState { NotCacheable, Hit, InitCandidate };`
- `struct TileInfo { TileRect rect; TileCacheState state; /* optional: hashes */ };`
- `struct MaxRect { TileRect rect; unsigned tilesWide; unsigned tilesHigh; };`

And utilities:

- `class TilingGrid` owning:
  - Tile size
  - Bounding dirty region `R`
  - 2D array of `TileInfo`

- Functions:
  - `build_tiling_grid(R, tileSize, PixelBuffer* pb, CacheQueryInterface& cache, TilingGrid& out)`
  - `find_max_cacheable_rects(const TilingGrid&, std::vector<MaxRect>& out)`

`CacheQueryInterface` is a small abstraction layer with implementations for:

- ContentCache server-side lookup + `knowsCacheId`.
- PersistentCache server-side lookup + `knowsPersistentHash`.

That keeps the tiling logic agnostic to which cache protocol is active.

## 5. Integration Points in EncodeManager

### 5.1 Where to run tiling

In `EncodeManager::writeFramebufferUpdate()` (or equivalent high-level function):

1. Identify "large" dirty regions.
   - For example, any rect with area ≥ `ContentCacheTileSize^2 * 4`.
   - Or, any region representing a full monitor or large window.
2. For each large dirty rect `R_large`:
   - Run `build_tiling_grid()` with the negotiated cache protocol.
   - Run `find_max_cacheable_rects()` to get `MaxRect` candidates.
   - For each `MaxRect` above a configurable threshold (e.g. ≥ 4 tiles):
     - Attempt a **cache-based encoding step** (see below).
   - Subtract all successfully cache-handled rectangles from the dirty region before running `writeSolidRects()` and `writeRects()`.

All existing logic remains intact for non-cacheable regions and for smaller dirty rects.

### 5.2 Emitting cache-based rectangles

Given a `MaxRect` `MR` that is fully marked as `Hit` in the grid:

- **ContentCache path**:
  - For each tile `T` inside `MR`, we already know there is a cache hit with some `cacheId`.
  - Two options:
    1. Emit multiple `CachedRect` messages (one per tile).
    2. If we have a server-side cache entry whose bounds exactly equal `MR.rect`, emit a single large `CachedRect`.

- **PersistentCache path**:
  - Similar logic, but using `PersistentCachedRect` with per-tile hashes.

For a first iteration, option (1) is simpler and still beneficial because **the encoder is no longer decomposing MR into solids + detail rects**; it's explicitly giving cache the chance to cover a large region.

## 6. Policy for Creating Large Cached Rectangles

To truly exploit whole-slide reuse, we need a policy for **creating** large cached rectangles, not just recognising existing hits.

### 6.1 When to seed a large cached rectangle

Heuristics for seeding a "big" cache entry for region `R_big`:

- `R_big` is large (e.g. ≥ quarter-screen, or above `ContentCacheMinRectSize * k`).
- The content is relatively stable across frames (few pixels change).
- The region appears multiple times (e.g. slide transitions or back/forward navigation).

Policy sketch:

1. When tiling detects that **most tiles in `R_big` are INIT candidates** (cacheable server-side but unknown to client), or it is the first time we see such a region:
   - Consider seeding a **full-region CachedRectInit / PersistentCachedRectInit** in addition to normal encoding.
2. For ContentCache:
   - Insert a `ContentKey` for `R_big` in the server-side ContentCache and get a `cacheId`.
   - Emit one `CachedRectInit(R_big, cacheId, encoding)` that carries a full encoding of `R_big`.
3. For PersistentCache:
   - Compute the content hash of `R_big` and insert into the persistent cache.
   - Emit `PersistentCachedRectInit(R_big, hash, encoding)`.

Configurable guards:

- `ContentCacheMaxSeedSize` / `PersistentCacheMaxSeedSize` to cap the size of seeded regions.
- `ContentCacheSeedCoolDown` (seconds or frames) to avoid seeding huge rectangles too frequently.
- Minimum **tile density of hits/INIT candidates** required to consider seeding.

### 6.2 Interaction with normal encoding

On the frame where we seed a large cached rectangle, we have two choices:

1. **Dual path (simple, more bandwidth):**
   - Encode `R_big` with the existing pipeline for correctness.
   - Also emit a large `CachedRectInit` solely for populating the cache for future frames.

2. **Single path (more complex, less bandwidth):**
   - Replace normal encoding of `R_big` with a custom encoder that both:
     - Sends one `CachedRectInit` as the primary encoding.
     - Ensures the viewer's framebuffer matches exactly what the non-cache path would have produced.

For an initial implementation, (1) is safer and easier to reason about. Once behaviour is validated, we can consider evolving toward (2).

## 7. Configuration and Tuning

Suggested new server parameters:

- `ContentCacheTileSize` (int, default 128)
- `PersistentCacheTileSize` (int, default 128)
- `ContentCacheMinTiledRectTiles` (int, default 4) — minimum tiles per max-rect
- `ContentCacheMaxSeedSize` (int, pixels^2 or width×height threshold)
- `ContentCacheSeedCoolDown` (int, frames or seconds)
- Analogous parameters for PersistentCache.

All of these should be read from the existing configuration mechanism so they can be tuned without recompiling.

## 8. Testing Strategy

To validate the tiling enhancement:

1. **Unit tests for tiling & maximal rectangle logic**
   - Synthetic masks with known maximal rectangles.
   - Edge cases: disjoint regions, skinny rects, full 1s, full 0s.

2. **Unit tests for cache query abstraction**
   - Mock `CacheQueryInterface` for ContentCache and PersistentCache to ensure the tiling layer does not depend on cache internals.

3. **E2E tests with static scenes**
   - PowerPoint-like slides with large constant backgrounds + text/logo.
   - Verify that:
     - On first view, seeds are created as expected (Init counts).
     - On subsequent views, large regions are served primarily via CachedRect / PersistentCachedRect.

4. **Performance/regression tests**
   - Verify that CPU overhead of tiling + hashing is acceptable.
   - Measure bandwidth and latency improvements vs. baseline.

## 9. Rollout Plan

1. Implement the **shared tiling and maximal-rectangle utilities** in `common/rfb/cache/`.
2. Integrate a **read-only tiling pass** in `EncodeManager` that does not yet emit different encodings, but logs what it *would* have done. Use this to tune thresholds.
3. Enable **cache-based emission** for ContentCache only, behind a feature flag.
4. Extend the same abstractions to PersistentCache (reusing the same tiling logic).
5. Iterate on heuristics and configuration based on real workloads.

This approach keeps tiling analysis orthogonal to the existing encoders, allows progressive rollout, and gives both ContentCache and PersistentCache a path toward more whole-scene reuse instead of only per-rect optimizations.

## 10. Current Progress

**Implemented:**

- Shared tiling utilities in `common/rfb/cache/`:
  - `TilingAnalysis.h/.cxx` providing `TileCacheState`, `TileInfo`, `MaxRect`, `CacheQueryInterface`, `buildTilingGrid()`, and `findLargestHitRectangle()`.
  - `TilingIntegration.h/.cxx` providing `ContentCacheQuery`, `PersistentCacheQuery`, and `analyzeRegionTilingLogOnly()`.
- Log-only integration in `EncodeManager::doUpdate()`:
  - Controlled by `TIGERVNC_CC_TILING_DEBUG` (and optional `TIGERVNC_CC_TILE_SIZE`).
  - Chooses PersistentCache when enabled and negotiated, otherwise ContentCache.
  - Logs tile hit/init counts and largest HIT rectangle for each update without changing on-wire behaviour.

**Not yet implemented:**

- Actually emitting `CachedRect` / `PersistentCachedRect` rectangles based on tiling results.
- Seeding of large cached rectangles (`CachedRectInit` / `PersistentCachedRectInit`) driven by tiling.
- Configurable server-side parameters (`ContentCacheTileSize`, `ContentCacheMinTiledRectTiles`, etc.).
- Unit tests for `TilingAnalysis` core helpers (`buildTilingGrid`, `findLargestHitRectangle`) in `tests/unit/tiling_analysis.cxx`.
- E2E tests that assert expected tiling behaviour in realistic scenarios (e.g. PowerPoint-like slides).

## 11. Next Steps

Short-term next steps:

1. **Unit tests for tiling core**
   - Add tests under `tests/unit/` for:
     - `buildTilingGrid()` on simple synthetic rectangles.
     - `findLargestHitRectangle()` using hand-crafted masks (single big rect, multiple disjoint rects, no hits, thin strips).
2. **Unit tests for cache query adapters**
   - Mock `SConnection` / `ContentCache` to verify `ContentCacheQuery` and `PersistentCacheQuery` classifications (Hit vs InitCandidate vs NotCacheable) without depending on full server state.
3. **Tuning via log-only runs**
   - Run with `TIGERVNC_CC_TILING_DEBUG=1` against real workloads.
   - Capture logs for representative sessions (PowerPoint, code, terminals) and adjust:
     - Default tile size.
     - `minTiles` threshold.
     - Any additional heuristics needed before enabling real emission.

Medium-term next steps:

4. **Prototype cache-based emission (behind a feature flag)**
   - In `EncodeManager`, add an optional path that, for a selected `MaxRect`:
     - Emits `CachedRect` / `PersistentCachedRect` updates instead of normal `writeRects()` in that region.
     - Leaves behaviour unchanged when the feature flag is off.
5. **Seeding large cached rectangles**
   - Implement an initial seeding policy based on tiling density and region size.
   - Start with the dual-path approach (seed via Init but still encode normally) until confident.
6. **Configuration wiring**
   - Introduce and document server parameters for tile size and thresholds.

## 12. Implementation Checklist

- [x] Design tiling model and maximal-rectangle algorithm (this document).
- [x] Implement shared tiling utilities (`TilingAnalysis` & `TilingIntegration`).
- [x] Integrate log-only tiling analysis into `EncodeManager` behind `TIGERVNC_CC_TILING_DEBUG`.
- [x] Add unit tests for `buildTilingGrid()` and `findLargestHitRectangle()` (see `tests/unit/tiling_analysis.cxx`).
- [ ] Add unit tests for `ContentCacheQuery` and `PersistentCacheQuery` classification.
- [ ] Run tiling diagnostics on real sessions and tune tile size / thresholds.
- [ ] Implement optional cache-based emission path (guarded by a feature flag).
- [ ] Implement initial seeding policy for large cached rectangles.
- [ ] Add configuration parameters and update user-facing documentation.
- [ ] Add focused E2E tests validating bandwidth/latency improvements in static-scene scenarios.
