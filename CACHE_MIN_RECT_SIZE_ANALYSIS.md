# ContentCache/PersistentCache Minimum Rectangle Size Analysis

**Date:** 2025-11-13  
**Current Threshold:** 4096 pixels (equivalent to 64×64)  
**Question:** Is 4096 pixels the right minimum threshold?

## Executive Summary

The current **4096 pixel minimum is likely too high** for ContentCache and PersistentCache. A threshold of **2048 pixels** (45×45 or 32×64) would be more appropriate based on:

1. **Protocol overhead analysis** showing break-even at ~1600-2000 pixels
2. **Real-world UI element sizes** (icons, buttons, widgets)  
3. **Different characteristics** from CopyRect (which likely inspired the 4096 value)
4. **Performance impact** is minimal due to ARC cache and hash table efficiency

**Recommendation:** Lower thresholds to **2048 pixels** for both protocols, making it a configurable parameter.

## Current Implementation

```cpp
// ServerCore.cxx
core::IntParameter rfb::Server::contentCacheMinRectSize
("ContentCacheMinRectSize",
 "Minimum rectangle size (pixels) to consider for caching",
 4096, 0, INT_MAX);

core::IntParameter rfb::Server::persistentCacheMinRectSize
("PersistentCacheMinRectSize",
 "Minimum rectangle size (pixels) to consider for persistent caching",
 4096, 0, INT_MAX);
```

Both protocols use **4096 pixels** as the default minimum.

## Protocol Overhead Analysis

### 1. ContentCache Protocol

**CachedRect** (reference to cached content):
```
Rectangle header: 12 bytes (x, y, w, h, encoding=-320)
Cache ID: 8 bytes (uint64_t)
Total: 20 bytes
```

**CachedRectInit** (initial send with cache ID):
```
Rectangle header: 12 bytes
Cache ID: 8 bytes
Encoding: 4 bytes
Pixel data: varies (same as normal encoding)
Total: 24 bytes + pixel_data
```

**Break-even calculation:**
- **Reference cost:** 20 bytes (fixed)
- **Full encoding cost:** 12 + compressed_pixels

For 32bpp (4 bytes/pixel) with Tight encoding:
- Uncompressed: pixels × 4 bytes
- Typical compression: 4:1 to 40:1 depending on content
- Average compression: ~10:1 for mixed content

**Best case (solid colors, 40:1 compression):**
- 1600 pixels: ~160 bytes compressed → **20 bytes reference saves 140 bytes (87% reduction)**
- 2048 pixels: ~205 bytes compressed → **20 bytes reference saves 185 bytes (90% reduction)**
- 4096 pixels: ~410 bytes compressed → **20 bytes reference saves 390 bytes (95% reduction)**

**Typical case (mixed content, 10:1 compression):**
- 1600 pixels: 640 bytes compressed → **20 bytes saves 620 bytes (97% reduction)**
- 2048 pixels: 820 bytes compressed → **20 bytes saves 800 bytes (98% reduction)**
- 4096 pixels: 1638 bytes compressed → **20 bytes saves 1618 bytes (99% reduction)**

**Worst case (JPEG photos, 2:1 compression):**
- 1600 pixels: 3200 bytes compressed → **20 bytes saves 3180 bytes (99% reduction)**
- 2048 pixels: 4096 bytes compressed → **20 bytes saves 4076 bytes (99.5% reduction)**
- 4096 pixels: 8192 bytes compressed → **20 bytes saves 8172 bytes (99.7% reduction)**

**Break-even point:** ~500-800 pixels (where 20 byte reference costs more than compressed data)
- Only occurs with extremely high compression (solid colors)
- Even at 800 pixels, savings are significant

### 2. PersistentCache Protocol

**PersistentCachedRect** (reference):
```
Rectangle header: 12 bytes
Hash length: 1 byte
Hash: 32 bytes (SHA-256)
Flags: 2 bytes
Total: 47 bytes
```

**PersistentCachedRectInit** (initial send):
```
Rectangle header: 12 bytes
Hash length: 1 byte
Hash: 32 bytes
Encoding: 4 bytes
Pixel data: varies
Total: 49 bytes + pixel_data
```

**Break-even calculation:**
- **Reference cost:** 47 bytes (2.35× larger than ContentCache)
- **Requires more data savings** to justify overhead

For 32bpp with Tight encoding:
- **Best case** (40:1): Break-even at ~1880 pixels
- **Typical case** (10:1): Break-even at ~470 pixels  
- **Worst case** (2:1): Break-even at ~94 pixels

**PersistentCache has higher overhead but still worthwhile at 2048+ pixels.**

## Comparison with CopyRect

**CopyRect** (the likely origin of 4096):
```
Rectangle header: 12 bytes
Source X: 2 bytes
Source Y: 2 bytes
Total: 16 bytes
```

**Key differences:**

| Aspect | CopyRect | ContentCache | PersistentCache |
|--------|----------|--------------|-----------------|
| **Overhead** | 16 bytes | 20 bytes | 47 bytes |
| **Lookup cost** | O(1) screen position | O(1) hash table | O(1) hash table |
| **Memory per entry** | 0 bytes (no cache) | ~40 bytes (key+metadata) | ~72 bytes (hash+metadata) |
| **Cache capacity** | N/A (screen-only) | 2GB default | 2GB default |
| **Reuse scope** | Current screen | Session | Cross-session |

**CopyRect constraints:**
- References **visible screen content** only
- Used during **window drags** (large regions)
- No cache memory overhead
- Threshold of 2048-4096 makes sense

**ContentCache/PersistentCache advantages:**
- References **historical content** (not limited to screen)
- Used for **repeated UI elements** (smaller, varied sizes)
- Large cache capacity (2GB) minimizes eviction pressure
- ARC algorithm keeps frequently-used small items hot

## Real-World UI Element Sizes

Common repeated elements that benefit from caching:

| Element | Typical Size | Pixels | Cached at 4096? | Cached at 2048? |
|---------|--------------|--------|-----------------|-----------------|
| **Small icons** | 48×48 | 2,304 | ❌ No | ✅ Yes |
| **Medium icons** | 64×64 | 4,096 | ✅ Yes (exact) | ✅ Yes |
| **Large icons** | 128×128 | 16,384 | ✅ Yes | ✅ Yes |
| **Toolbar buttons** | 32×32 | 1,024 | ❌ No | ❌ No |
| **Small widgets** | 40×40 | 1,600 | ❌ No | ❌ No |
| **Logo badges** | 50×50 | 2,500 | ❌ No | ✅ Yes |
| **Window title bar** | 200×24 | 4,800 | ✅ Yes | ✅ Yes |
| **Menu items** | 150×20 | 3,000 | ❌ No | ✅ Yes |
| **Status icons** | 56×56 | 3,136 | ❌ No | ✅ Yes |
| **Tab headers** | 100×28 | 2,800 | ❌ No | ✅ Yes |

**At 4096:** Only catches medium+ icons and title bars  
**At 2048:** Catches small icons, logos, status icons, menus, tabs

**Modern UI trends:**
- Many applications use **48×48 to 64×64 icons**
- Material Design icons: **24×24, 48×48** common
- Retina/HiDPI: Logical 48×48 = Physical 96×96 (9,216 pixels) ✅
- But subdivision can split these below threshold

## Rectangle Subdivision Impact

The encoder subdivides large rectangles:
- **`SubRectMaxArea = 65536`** (256×256 max)
- **`SubRectMaxWidth = 2048`** (max width)

**Example: 128×128 icon (16,384 pixels)**
- Original: 16,384 pixels ✅ above 4096
- After subdivision: May split into smaller pieces
- Some fragments might be **below 4096 threshold**

**Example from test logs:**
- 48×48 logo (2,304 pixels) subdivided into:
  - 50×21 = 1,050 pixels ❌
  - 51×21 = 1,071 pixels ❌
  - 51×38 = 1,938 pixels ❌

**All fragments rejected at 4096, but would pass at 1024-2048.**

## Performance Considerations

### Memory Overhead

**Per cache entry cost:**
- ContentCache: ~40 bytes (ContentKey + CacheEntry metadata)
- PersistentCache: ~72 bytes (32-byte hash + metadata)

**At 2048 pixel threshold:**
- Assuming 50% more cache entries than 4096 threshold
- 2GB cache with 2048 avg rect size: ~1M entries max
- Memory overhead: 40-72 MB for metadata
- **Impact: <4% of cache size** – negligible

### Hash Computation Cost

**ContentHash (SHA-256) performance:**
- ~500-1000 MB/s on modern CPUs
- 2048 pixels × 4 bytes = 8 KB
- **Hash time: ~8-16 μs** per rect
- Encoding time: 100-1000 μs per rect
- **Hash is 1-10% of encoding cost** – minimal

### Cache Lookup Cost

**Hash table lookup: O(1)**
- `std::unordered_map` with good hash function
- **Lookup time: ~50-200 ns**
- Negligible compared to encoding

### ARC Algorithm Efficiency

**Adaptive Replacement Cache handles mixed sizes well:**
- T1 (recent, once): Prevents small item churn
- T2 (frequent): Promotes repeated small items
- Ghost lists: Learns access patterns
- **Works efficiently even with many small entries**

## Cost-Benefit at Different Thresholds

| Threshold | UI Coverage | Bandwidth Savings | Memory Overhead | Recommendation |
|-----------|-------------|-------------------|-----------------|----------------|
| **1024** | Excellent (catches 32×32 buttons) | 95-99% per hit | Moderate (doubles entries) | Too low - catches noise |
| **2048** | Very Good (catches 48×48 icons) | 97-99% per hit | Low (50% more entries) | ✅ **Recommended** |
| **4096** | Good (64×64+ only) | 99% per hit | Minimal | ❌ Too conservative |
| **8192** | Poor (128×128+ only) | 99.5% per hit | Very Low | Too high - misses common UI |

## Historical Context

The 4096 value appears to have **no documented rationale**. Likely origins:

1. **Copied from CopyRect heuristics** (which had different constraints)
2. **Conservative estimate** from early development
3. **64×64 = 4096** is a "round number" in powers of 2
4. **Pre-ARC implementation** when cache efficiency was less predictable

## Recommendations

### 1. Lower Default Thresholds (High Priority)

```cpp
// Recommended new defaults
core::IntParameter rfb::Server::contentCacheMinRectSize
("ContentCacheMinRectSize",
 "Minimum rectangle size (pixels) to consider for caching",
 2048, 0, INT_MAX);  // Changed from 4096

core::IntParameter rfb::Server::persistentCacheMinRectSize
("PersistentCacheMinRectSize",
 "Minimum rectangle size (pixels) to consider for persistent caching",
 2048, 0, INT_MAX);  // Changed from 4096
```

**Rationale:**
- Captures 48×48 icons (2,304 pixels after margin)
- Still excludes tiny 32×32 buttons (1,024 pixels)
- Minimal performance impact (<4% memory overhead)
- Significantly better UI element coverage

### 2. Consider Dynamic Thresholds (Future Enhancement)

**Adaptive threshold based on:**
- Cache hit rate (lower threshold if hit rate is high)
- Available memory (raise threshold if cache is full)
- Rectangle size distribution (exclude outliers)

### 3. Add Threshold to Test Documentation

Update `tests/e2e/` documentation to specify:
- Test content must be **≥2048 pixels** (not 4096)
- Recommended test sizes: 64×64, 96×96, 128×128
- Explain why smaller content won't exercise cache

### 4. Document in WARP.md

Add to `/home/nickc/code/tigervnc/WARP.md`:
```markdown
## ContentCache/PersistentCache Thresholds

Minimum rectangle size: **2048 pixels** (default)
- Captures: 48×48 icons, UI widgets, logos
- Excludes: Small buttons (<45×45), trivial fragments
- Configurable via `ContentCacheMinRectSize` / `PersistentCacheMinRectSize`
```

## Migration Plan

1. **Change defaults** in `common/rfb/ServerCore.cxx`
2. **Update tests** to use 64×64+ test images
3. **Re-run benchmarks** to verify performance impact
4. **Document** in CONTENTCACHE_DESIGN_IMPLEMENTATION.md
5. **Commit** with clear rationale in commit message

## Conclusion

The **4096 pixel threshold is too conservative** for ContentCache and PersistentCache. Lowering to **2048 pixels** will:

✅ Cover common UI elements (48×48 icons, status badges, menu items)  
✅ Provide 97-99% bandwidth savings per hit  
✅ Minimal performance overhead (<4% memory, <10% CPU for hashing)  
✅ Better utilize the 2GB cache capacity  
✅ Improve cache hit rates in real-world applications  

The protocols are designed for **historical content reuse**, not just **screen-to-screen copying** like CopyRect. Different use cases justify different thresholds.
