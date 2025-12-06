# Lossy Cache Bug Fix - December 6, 2025

## Problem Summary

The lossy hash reporting protocol (message 247) is fully implemented but **cache lookups fail** for lossy content due to three critical bugs in the client-side DecodeManager.

## Root Cause Analysis

### The Cache Key Structure
```cpp
struct CacheKey {
    uint16_t width;       // Rectangle width
    uint16_t height;      // Rectangle height
    uint64_t contentHash; // 64-bit ID (canonical OR lossy)
};
```

### The Lossy Content Flow

#### Server Side (Working Correctly ✅)
1. Computes canonical hash from framebuffer: `canonicalId = hash(pixels)`
2. Sends `PersistentCachedRectInit(rect, canonicalId, encoding=ZRLE)`
3. Encodes pixels with lossy compression (JPEG/ZRLE)
4. Sends compressed data to client

#### Client Side (BROKEN ❌)
1. Receives `PersistentCachedRectInit` with `canonicalId`
2. Decodes compressed data → lossy pixels
3. Computes hash of decoded pixels: `lossyId = hash(decoded_pixels)`
4. Detects `lossyId != canonicalId` (expected for lossy)
5. **BUG #1**: Stores in cache with key `(width, height, canonicalId)` ← Wrong!
6. Reports to server: `PersistentCacheHashReport(canonicalId, lossyId)`
7. Server stores mapping: `lossyHashCache_[canonicalId] = lossyId`

#### Next Occurrence (Cache Lookup FAILS ❌)
1. Server computes `canonicalId` for same content
2. Server checks: `knowsPersistentId(canonicalId)` → false
3. Server checks: `hasLossyHash(canonicalId, lossyId)` → true! (mapping exists)
4. Server checks: `knowsPersistentId(lossyId)` → true! (client has it)
5. Server sends `PersistentCachedRect(rect, lossyId)` ← Using lossy ID
6. **BUG #2**: Client looks up with key `(width, height, lossyId)` ← Not found!
7. Client stored it as `(width, height, canonicalId)` but server referenced `lossyId`
8. **Result**: Cache miss, client requests full data again

### Bug #3: Seed Rejection

When server uses `CachedRectSeed` for large rectangles:
1. Server sends `CachedRectSeed(rect, canonicalId)`
2. Server encodes rect with lossy compression
3. Server sends encoded data
4. Client decodes → lossy pixels in framebuffer
5. Client receives seed: `seedCachedRect(rect, canonicalId, framebuffer)`
6. Client computes hash of framebuffer: `lossyId = hash(framebuffer)`
7. **BUG #3**: Detects `lossyId != canonicalId`, **rejects seed entirely**
8. No cache entry stored, future lookups always miss

## The Three Bugs

### Bug #1: storePersistentCachedRect stores with wrong ID
**File**: `common/rfb/DecodeManager.cxx`  
**Lines**: 1042-1086

```cpp
// Current (WRONG):
bool hashMatch = (hashId == cacheId);
if (!hashMatch) {
    // Reports lossy hash ✅
    conn->writer()->writePersistentCacheHashReport(cacheId, hashId);
}
// Stores with CANONICAL ID only ❌
persistentCache->insert(cacheId, diskKey, pixels, ...);
```

**Should be**: Store with **BOTH** canonical and lossy IDs so either lookup works.

### Bug #2: handlePersistentCachedRect validates incorrectly
**File**: `common/rfb/DecodeManager.cxx`  
**Lines**: 912-972

```cpp
// Current (WRONG):
CacheKey key((uint16_t)r.width(), (uint16_t)r.height(), (uint64_t)cacheId);
const CachedPixels* cached = persistentCache->getByKey(key);

// Validates cached pixels against cacheId
uint64_t hashId = hash(cached->pixels);
if (hashId != cacheId) {  // ❌ Fails for lossy content!
    invalidate();
    return;
}
```

**Should be**: Accept cached pixels if hash matches **either** the lookup ID or a known alias.

### Bug #3: seedCachedRect rejects lossy seeds
**File**: `common/rfb/DecodeManager.cxx`  
**Lines**: 1140-1160

```cpp
// Current (WRONG):
uint64_t hashId = hash(framebuffer);
if (hashId != cacheId) {
    vlog.info("seedCachedRect skipped: hash mismatch");
    invalidate(cacheId);  // ❌ Destroys cache entry!
    return;  // ❌ Doesn't store anything!
}
```

**Should be**: Store pixels, report lossy hash (same as `storePersistentCachedRect`).

## The Fix Strategy

### Option A: Dual Storage (Simple)
Store lossy content under **both** canonical and lossy IDs:

```cpp
if (!hashMatch) {
    // Store under canonical ID
    persistentCache->insert(canonicalId, diskKey, pixels, ...);
    // ALSO store under lossy ID
    persistentCache->insert(lossyId, lossyDiskKey, pixels, ...);
    // Report mapping
    conn->writer()->writePersistentCacheHashReport(canonicalId, lossyId);
}
```

**Pros**: Simple, no lookup changes needed  
**Cons**: 2x memory usage for lossy content

### Option B: Alias Mapping (Memory-Efficient) ✅ RECOMMENDED
Store once, track aliases:

```cpp
if (!hashMatch) {
    // Store under lossy ID (actual content hash)
    persistentCache->insert(lossyId, lossyDiskKey, pixels, ...);
    // Register canonical ID as alias
    persistentCache->registerAlias(canonicalId, lossyId);
    // Report mapping
    conn->writer()->writePersistentCacheHashReport(canonicalId, lossyId);
}
```

**Lookup**: Check primary ID, then check if it's an alias.

**Pros**: Single copy in memory, clean design  
**Cons**: Requires alias tracking infrastructure

### Option C: Store Under Lossy ID Only (SIMPLEST) ✅ IMPLEMENT THIS
The server already tracks canonical→lossy mappings and sends the correct ID. Just store under the ID that matches the actual pixels:

```cpp
// Store under whichever ID matches the actual pixel content
uint64_t storageId = hashMatch ? cacheId : hashId;

persistentCache->insert(storageId, diskKey, pixels, ...);

if (!hashMatch) {
    // Report so server learns the mapping
    conn->writer()->writePersistentCacheHashReport(cacheId, hashId);
}
```

**Validation**: Accept if cached pixels match the lookup ID (already computed).

## Implementation Plan

### Step 1: Fix `storePersistentCachedRect`
Store under the ID that matches actual pixel content:

```cpp
void DecodeManager::storePersistentCachedRect(const core::Rect& r,
                                             uint64_t cacheId,
                                             int encoding,
                                             ModifiablePixelBuffer* pb) {
    // ... existing code ...
    
    uint64_t hashId = hash(pixels);
    bool hashMatch = (hashId == cacheId);
    
    if (!hashMatch) {
        // Report lossy hash
        conn->writer()->writePersistentCacheHashReport(cacheId, hashId);
    }
    
    // KEY FIX: Store under the ID that matches actual pixels
    uint64_t storageId = hashMatch ? cacheId : hashId;
    
    persistentCache->insert(storageId, diskKey, pixels, pb->getPF(),
                          r.width(), r.height(), stridePixels,
                          isLossless);
}
```

### Step 2: Fix `seedCachedRect`
Apply same logic as `storePersistentCachedRect`:

```cpp
void DecodeManager::seedCachedRect(const core::Rect& r,
                                   uint64_t cacheId,
                                   ModifiablePixelBuffer* pb) {
    // ... existing code ...
    
    uint64_t hashId = hash(framebuffer);
    bool hashMatch = (hashId == cacheId);
    
    if (!hashMatch) {
        // Report lossy hash (same as storePersistentCachedRect)
        conn->writer()->writePersistentCacheHashReport(cacheId, hashId);
        vlog.info("seedCachedRect: detected lossy content, storing under lossy ID");
    }
    
    // KEY FIX: Store under the ID that matches actual pixels
    uint64_t storageId = hashMatch ? cacheId : hashId;
    
    persistentCache->insert(storageId, diskKey, pixels, pb->getPF(),
                          r.width(), r.height(), stridePixels,
                          isLossless);
}
```

### Step 3: Fix `handlePersistentCachedRect`
Validation is already correct! Since we now store under the correct ID, the existing check works:

```cpp
// This is CORRECT - no changes needed
if (hashId != cacheId) {
    // If we stored correctly, this won't happen
    invalidate();
    return;
}
```

## Expected Results After Fix

### Lossy Content Flow (Fixed)
1. Client receives `PersistentCachedRectInit(rect, canonicalId=100)`
2. Decodes → computes `lossyId=200`
3. Stores in cache with key `(width, height, 200)` ← Fixed!
4. Reports `PersistentCacheHashReport(100, 200)` to server
5. Server stores `lossyHashCache_[100] = 200`

### Next Occurrence (Cache Hit!)
1. Server computes `canonicalId=100`
2. Server checks mapping → finds `lossyId=200`
3. Server checks `knowsPersistentId(200)` → true!
4. Server sends `PersistentCachedRect(rect, 200)`
5. Client looks up `(width, height, 200)` → **FOUND!** ✅
6. Client validates: `hash(cached) == 200` → **MATCH!** ✅
7. Client blits cached pixels
8. **Result: CACHE HIT, bandwidth saved!**

### Seed Flow (Fixed)
1. Server sends `CachedRectSeed(rect, canonicalId=100)`
2. Server encodes with lossy → client decodes
3. Client receives seed: `seedCachedRect(rect, 100, framebuffer)`
4. Client computes `lossyId=200` from framebuffer
5. Reports `PersistentCacheHashReport(100, 200)`
6. Stores with key `(width, height, 200)` ← Fixed!
7. Future lookups for `lossyId=200` succeed

## Test Validation

### Tests That Should Now Pass
1. **test_cpp_contentcache.py** - Should get >20% hit rate
2. **test_cpp_cache_eviction.py** - Should get >20% hit rate  
3. **test_large_rect_cache_strategy.py** - Should get >15% hit rate
4. **test_toggle_pictures.py** - Should get 3± hits per toggle
5. **test_libreoffice_slides.py** - Should get 2± hits per transition
6. **test_lossy_lossless_parity.py** - Lossy should work like lossless
7. **test_seed_mechanism.py** - Seeds should not be skipped

### Tests That Should Still Pass
- **test_cpp_persistentcache.py** - Already passing (26% hit rate)
- **test_cpp_no_caches.py** - No cache behavior unchanged
- All visual corruption tests should pass (pixels still correct)

## Memory and Performance Impact

### Memory
- **No change**: Each cache entry stored once under correct ID
- Lossy content: stored under `lossyId` (matches pixel content)
- Lossless content: stored under `canonicalId` (matches pixel content)

### CPU
- **No change**: Hash already computed in all paths
- Just using computed hash for storage instead of ignoring it

### Network
- **Massive improvement**: Cache hits now work for lossy content
- Bandwidth savings: 50KB-500KB per hit for large rectangles

## Rollback Plan

If this causes issues:
1. Revert changes to `DecodeManager.cxx`
2. Re-run tests to confirm old behavior restored
3. No data corruption possible (cache is memory-only until explicitly persisted)

## Files to Modify

1. `common/rfb/DecodeManager.cxx`
   - Fix `storePersistentCachedRect()` (line ~1084)
   - Fix `seedCachedRect()` (lines ~1140-1183)
   - Validation in `handlePersistentCachedRect()` already correct

## Summary

The lossy hash reporting protocol is **100% correct** on the server side and in the protocol layer. The bugs are purely in the **client-side cache storage logic** that:

1. Stores lossy content under canonical ID (wrong)
2. Rejects seed requests for lossy content (wrong)
3. Validates with the wrong expectation (consequence of #1)

The fix is simple: **Store content under the ID that matches the actual pixels**, whether canonical (lossless) or lossy (computed after decode).
