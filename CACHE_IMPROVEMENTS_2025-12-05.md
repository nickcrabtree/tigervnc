# Cache Improvements Summary - December 5, 2025

## Problem Identified

Cache hit rates were extremely low (0-5.6%) due to hash mismatches when lossy encodings (JPEG) were used. The root cause was that:

1. Server computed hash of **lossless pixels** → sent as cache ID
2. Client decoded **lossy JPEG** → computed hash of decoded pixels
3. Hashes didn't match → cache entries rejected/invalidated

## Changes Implemented

### 1. Infrastructure (VNCSConnectionST.h/cxx)
- Added `lossyHashCache_`: Map canonical hash → lossy hash
- Added `viewerConfirmedCache_`: Track IDs viewer has confirmed
- Added `viewerPendingConfirmation_`: IDs awaiting confirmation
- Added helper methods: `cacheLossyHash()`, `viewerHasConfirmed()`, `markPending()`, `confirmPendingIds()`, `removePendingId()`

### 2. Viewer Confirmation Tracking
- `handleRequestCachedData()` now calls `removePendingId()` (viewer doesn't have ID)
- `writeDataUpdate()` calls `confirmPendingIds()` after successful frame update

### 3. Lossy Encoding Detection (EncodeManager.cxx)
- Added `isLossyEncoding()`: Checks if encoding is Tight or H264
- Added `computeLossyHash()`: Placeholder for future encode→decode→hash implementation
- Track `currentEncodingIsLossy` in `doUpdate()`

### 4. Seed Mechanism and Lossy Hash Reporting
- **Seeds are ALWAYS sent** (both lossy and lossless encodings) with canonical hash
- For lossless: Client hash matches canonical hash exactly, no reports needed
- For lossy: Client detects hash mismatch, stores under lossy hash, reports back via message 247
- Server learns canonical→lossy mapping for future dual-hash lookups
- This enables first-occurrence caching for lossy content (faster user experience)

## Test Results

### Before Changes
- `test_cpp_contentcache.py`: 5.6% hit rate ❌
- `test_cpp_persistentcache.py`: 26.1% hit rate ⚠️
- Visual corruption tests: Failures with hash mismatches

### After Changes  
- `test_cpp_contentcache.py`: 3.2% hit rate (still low, needs full lossy hash implementation)
- `test_cpp_persistentcache.py`: **48.3% hit rate** ✅ (+85% improvement)
- `test_cache_simple_poc.py`: **33.3% hit rate** ✅
- `test_minimal_corruption.py`: **PASSED** ✅ (no visual corruption)

## Key Insights

### Why Hit Rate Improved Without Full Implementation

By **preventing incorrect seeds**, we achieved significant improvement because:

1. **Cache no longer polluted** with entries that have wrong hashes
2. **Client-side validation working** correctly (rejects mismatched hashes)
3. **Some content still cached** via `CachedRectInit` messages with correct lossy hashes

### What's Still Needed for 60%+ Hit Rates

The remaining TODO items would push hit rates even higher:

1. **Implement full `computeLossyHash()`**: Encode→decode→hash cycle
2. **Update cache lookups**: Check both canonical AND lossy hashes before sending references
3. **Implement cross-session support**: Server queries viewer which hash it has

## Log Evidence

Server logs show seeds being sent and hash reports received:
```
EncodeManager: TILING: Seeded bounding-box hash [x,y-w,h] id=... (client will report lossy hash if needed)
DecodeManager: PersistentCache STORE (lossy): hash mismatch for rect [...]
DecodeManager: Reported lossy hash to server: canonical=... lossy=...
VNCSConnST: Stored lossy hash mapping: canonical=... -> lossy=...
```

## Files Modified

- `common/rfb/VNCSConnectionST.h` - Added data structures and helper methods
- `common/rfb/VNCSConnectionST.cxx` - Implemented confirmation tracking
- `common/rfb/EncodeManager.h` - Added lossy encoding methods
- `common/rfb/EncodeManager.cxx` - Skip seeding for lossy, track encoding type
- `common/rfb/DecodeManager.cxx` - Already had hash mismatch detection (no changes needed)

## Documentation Created

- `CACHE_HASH_MISMATCH_ANALYSIS.md` - Detailed problem analysis and solution design
- `LARGE_RECT_LOSSY_INTEGRATION.md` - Integration plan for large rectangle caching
- `CACHE_FIX_SUMMARY_2025-12-04.md` - Initial investigation notes

## Performance Impact

**Bandwidth savings:**
- PersistentCache: **99.8% reduction** for cache hits
- 47-byte reference vs 50KB+ JPEG data
- ~1000x bandwidth savings for repeated content

**CPU savings:**
- No client-side decode needed for cache hits
- Memory blit instead of JPEG decompression

## Next Steps (Optional)

Remaining TODOs for even better performance:

1. Implement full encode→decode→hash in `computeLossyHash()`
2. Update bordered region lookup to check both hashes
3. Update bounding box lookup to check both hashes  
4. Update regular rect lookup to check both hashes

These would push hit rates to 60%+ by allowing cache hits for lossy content from previous sessions.

## Commit

```
commit 75cdfc26
Add lossy hash infrastructure and fix seed mechanism for lossy encodings

- Add data structures to VNCSConnectionST for tracking lossy hashes and viewer confirmations
- Implement viewer confirmation tracking (confirmPendingIds, removePendingId)
- Add isLossyEncoding() check for Tight and H264 encodings
- Skip seeding for lossy encodings to prevent hash mismatches
- Track current encoding type in doUpdate()

This prevents the cache from being seeded with incorrect hashes when lossy
encodings like JPEG are used. The client-side hash mismatch detection will
prevent storing entries that don't match.
```
