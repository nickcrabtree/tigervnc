# Lossy vs Lossless Cache Behavior

**Date**: 2025-12-12  
**Status**: IMPLEMENTED

## Overview

The TigerVNC cache system (both ContentCache and PersistentCache) supports caching both lossless and lossy content, with automatic quality preference to ensure the best visual fidelity.

## Content Hash IDs

### Canonical ID (Lossless)
- Computed from the **server's framebuffer** before encoding
- Represents the "true" pixel values without any compression artifacts
- Used for lossless encodings (Raw, ZRLE without JPEG subencodings)

### Lossy ID (Compressed)
- Computed from the **decoded pixels at the client** after lossy compression
- Represents the actual pixels the client has (with compression artifacts)
- Different from canonical ID due to JPEG/lossy compression
- Used for lossy encodings (Tight with JPEG, TightJPEG)

## Storage Behavior

### Client-Side Storage

The client stores content under the ID that matches the **actual decoded pixels**:

```cpp
uint64_t hashId = hash(decoded_pixels);
bool hashMatch = (hashId == cacheId);  // Compare with server's canonical ID

if (hashMatch) {
    // Lossless: decoded pixels match server's canonical hash
    storageId = cacheId;  // Store under canonical ID
} else {
    // Lossy: compression artifacts caused hash mismatch
    storageId = hashId;   // Store under lossy ID
    // Report mapping to server
    conn->writer()->writePersistentCacheHashReport(cacheId, hashId);
}

persistentCache->insert(storageId, ...);
```

**Key principle**: Store content under the ID that the client can compute from its own pixels.

### Server-Side Tracking

When the server receives a lossy hash report, it must update its tracking:

```cpp
void handlePersistentCacheHashReport(uint64_t canonicalId, uint64_t lossyId) {
    // CRITICAL: Remove canonical ID from known set
    // Client stored under lossy ID, cannot look up by canonical ID
    knownPersistentIds_.erase(canonicalId);
    
    // Add lossy ID to known set
    markPersistentIdKnown(lossyId);
    
    // Store mapping for fallback
    cacheLossyHash(canonicalId, lossyId);
}
```

**Why remove canonical ID?** Because the client doesn't actually have content stored under that ID. The "known" set must reflect what IDs the client can actually look up.

## Quality Preference

### Lookup Order

Server lookups **prefer lossless over lossy** to maximize visual quality:

```cpp
// 1. Check canonical ID first (lossless, best quality)
if (conn->knowsPersistentId(canonicalId)) {
    matchedId = canonicalId;
}
// 2. Fall back to lossy ID only if lossless not available
else if (conn->hasLossyHash(canonicalId, lossyId) &&
         conn->knowsPersistentId(lossyId)) {
    matchedId = lossyId;
}
```

This order ensures:
- ✅ Lossless content is always preferred when available
- ✅ Lossy content works as fallback
- ✅ Quality upgrades are possible (lossy → lossless)

## Quality Upgrade Path

### Scenario: Lossy First, Then Lossless

1. **First occurrence (lossy encoding)**:
   - Server sends `PersistentCachedRectInit(canonical=100)` with lossy encoding
   - Client decodes → computes `lossyId=200`, stores under lossy ID 200
   - Client reports `PersistentCacheHashReport(100, 200)`
   - Server removes canonical ID 100, adds lossy ID 200 to known set

2. **Second occurrence (still lossy)**:
   - Server checks: `knowsPersistentId(100)` → FALSE (removed!)
   - Server checks: `hasLossyHash(100, 200)` and `knowsPersistentId(200)` → TRUE
   - Server sends `PersistentCachedRect(200)` with lossy ID
   - Client looks up lossy ID 200 → **HIT** ✅

3. **Later occurrence (lossless encoding)**:
   - Network improves, server now uses lossless encoding
   - Server sends `PersistentCachedRectInit(canonical=100)` with lossless encoding
   - Client decodes → computes hash = 100 (perfect match!)
   - Client stores under canonical ID 100 (NEW ENTRY with better quality)
   - No hash report sent (hash matches)
   - Server marks canonical ID 100 as known

4. **Future occurrences**:
   - Server checks: `knowsPersistentId(100)` → TRUE (lossless version!)
   - Server sends `PersistentCachedRect(100)` with canonical ID
   - Client looks up canonical ID 100 → **HIT with better quality!** ✅

**Result**: Cache automatically upgrades to lossless when available.

## Dual Storage vs Single Storage

### NOT USED: Dual Storage
We do NOT store content under both IDs because:
- ❌ Doubles memory usage for lossy content
- ❌ Wastes cache capacity
- ❌ More complex bookkeeping

### USED: Single Storage with Quality Tracking
We store each quality level separately:
- Lossy content: stored once under lossy ID
- Lossless content: stored once under canonical ID
- Each version is independent, allowing quality upgrades

## Memory and Disk Persistence

### Lossy Content
- Stored in **memory only** (`isLossless=false` flag)
- Never persisted to disk
- Prevents cross-session visual drift from compression artifacts
- Still provides bandwidth savings within the session

### Lossless Content
- Stored in **memory AND disk** (`isLossless=true` flag)
- Persists across sessions
- Safe for long-term reuse (bit-identical)

## Testing

Tests should verify:

1. **Lossy content cache hits work**: Server uses lossy ID, client finds content
2. **Lossless is preferred**: When both exist, canonical ID is used
3. **Quality upgrades**: Lossy → lossless transition works
4. **No incorrect hits**: Client never gets wrong-quality content

### Example Test Flow

```python
# Phase 1: Force lossy encoding (low bandwidth)
# Expect: Lossy cache hits after first occurrence

# Phase 2: Force lossless encoding (high bandwidth)
# Expect: Lossless cache hits, higher quality

# Phase 3: Mix of lossy and lossless
# Expect: Lossless preferred when available
```

## Implementation Files

### Client Side
- `common/rfb/DecodeManager.cxx`:
  - `storePersistentCachedRect()`: Stores under correct ID
  - `seedCachedRect()`: Handles seed messages
  - `handlePersistentCachedRect()`: Validates and blits cached content

### Server Side
- `common/rfb/VNCSConnectionST.cxx`:
  - `handlePersistentCacheHashReport()`: Updates known set correctly
  - `markPersistentIdKnown()`: Tracks what client can look up

- `common/rfb/EncodeManager.cxx`:
  - `tryPersistentCacheLookup()`: Prefers lossless over lossy
  - Bordered and bbox lookups: Same preference order

### Cache Engine
- `common/rfb/GlobalClientPersistentCache.cxx`:
  - `insert()`: Stores content with quality flag
  - `getByKey()`: Fast lookup by CacheKey

## Edge Cases

### What if server sends canonical ID for lossy content?
- Client will miss (lookup fails)
- Client sends `RequestCachedData`
- Server sends full data with `PersistentCachedRectInit`
- This is why server must track lossy mappings correctly!

### What if client has both lossy and lossless versions?
- They're stored as separate entries (different IDs)
- Server prefers canonical ID in lookups
- Lossy version may eventually be evicted from cache

### What if compression quality changes?
- Different JPEG quality levels produce different lossy IDs
- Each quality level is a separate cache entry
- This is acceptable - prevents visual inconsistency

## Future Enhancements

Potential improvements (not currently needed):

1. **Active quality replacement**: When lossless version arrives, actively evict lossy version
2. **Quality-level tracking**: Store JPEG quality with entry, only reuse same quality
3. **Perceptual hashing**: Use pHash to match similar content despite compression differences

Current implementation is sufficient for all test requirements and real-world usage.

## Related Documentation

- `LOSSY_CACHE_BUG_FIX_2025-12-06.md`: Bug fix implementation details
- `docs/CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: Overall cache design
- `docs/PERSISTENTCACHE_DESIGN.md`: PersistentCache protocol
- `CACHE_FIX_SUMMARY_2025-12-04.md`: Lossy caching enablement
