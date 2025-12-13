# Cache Hash Mismatch Analysis
**Date**: 2025-12-04  
**Updated**: 2025-12-05  
**Issue**: Low cache hit rates (0-5.6%) with hash invalidations

## Key Insight

**Root cause**: Lossy encodings (JPEG) produce different pixels than the original, causing hash mismatches between server and client.

**Solution**: Server must compute *both* hashes:
1. **Canonical hash** (lossless pixels) - for server-side deduplication
2. **Lossy hash** (encode→decode→hash) - for client verification

**Critical requirement**: Server must verify with viewer which hash it has before sending reference, because:
- Viewer's cache may be from a different server run
- Different JPEG quality settings produce different lossy hashes
- Server must check if viewer has either canonical OR lossy hash

## Problem Summary

The cache system has a fundamental hash mismatch issue when using lossy encodings:

### Current Flow

**Server side:**
1. Computes hash of **original lossless pixels** → `hashA`
2. Uses `hashA` as cache ID
3. Encodes rect with **lossy compression** (Tight JPEG)
4. Sends cache reference with ID = `hashA`

**Client side:**
1. Receives cache reference with ID = `hashA`
2. Decodes **lossy JPEG data** → slightly different pixels
3. Stores decoded pixels with ID = `hashA`
4. Later receives another reference with ID = `hashA`
5. Computes hash of its stored (lossy decoded) pixels → `hashB`
6. **`hashA ≠ hashB` → HIT INVALIDATED**

## Evidence from Test Logs

### test_cpp_persistentcache.py (26.1% hit rate - partially working)
```
DecodeManager: PersistentCache STORE (lossy): hash mismatch for rect
DecodeManager: PersistentCache STORE (lossy): hash mismatch for rect
DecodeManager: PersistentCache STORE (lossy): hash mismatch for rect
DecodeManager: PersistentCache HIT INVALIDATED: rect [1819,101-1917,122]
```

### test_cpp_contentcache.py (5.6% hit rate - failing)
```
DecodeManager: PersistentCache STORE (lossy): hash mismatch for rect
DecodeManager: PersistentCache HIT INVALIDATED: rect [1819,101-1917,122]
DecodeManager: PersistentCache MISS: rect [673,100-1345,223]
```

**Pattern**: All stored rects show "lossy: hash mismatch", and subsequent lookups are invalidated.

## Why Some Tests Show Higher Hit Rates

The 26.1% hit rate in `test_cpp_persistentcache.py` suggests:
- Some content is encoded losslessly (Raw, ZRLE without palette reduction)
- These lossless encodings allow hash verification to work
- Lossy JPEG-compressed content always fails validation

## Root Cause

The protocol design assumes:
```
server_hash(original_pixels) == client_hash(decoded_pixels)
```

This is only true for **lossless encodings**. For lossy encodings:
```
server_hash(lossless) ≠ client_hash(lossy_decoded)
```

## Proposed Solution: Server-Side Lossy Hash Cache

Implement a two-phase hash system:

### Phase 1: Canonical Hash (for deduplication)
- Compute hash of original lossless pixels → `canonical_hash`
- Use for server-side deduplication
- Determines if content should be cached

### Phase 2: Lossy Hash (for client verification)
**For lossy encodings only:**
1. Encode rect to JPEG
2. **Decode the JPEG back to pixels** (simulate client)
3. Compute hash of decoded pixels → `lossy_hash`
4. Send cache reference with ID = `lossy_hash`
5. Cache mapping: `canonical_hash → lossy_hash`

**For lossless encodings:**
- `lossy_hash = canonical_hash` (no decoding needed)

### Server Flow
```cpp
// Encode the rect
std::vector<uint8_t> encoded_data = encode(rect, encoding);

uint64_t cacheId;
if (is_lossy_encoding(encoding)) {
    // Decode to get what client will see
    PixelBuffer decoded = decode(encoded_data, encoding);
    std::vector<uint8_t> lossy_hash = ContentHash::computeRect(decoded, rect);
    memcpy(&cacheId, lossy_hash.data(), sizeof(uint64_t));
    
    // Store mapping for future lookups
    lossyHashCache[canonical_hash] = cacheId;
} else {
    cacheId = canonical_hash;  // Lossless - use original hash
}

writer->writePersistentCachedRect(rect, cacheId);
```

### Client Flow
No changes needed - client already computes hash of its decoded pixels.

## Cross-Session Problem

**Critical issue identified**: The lossy hash depends on encoding parameters (JPEG quality, encoder version), so:

1. **Session 1**: Server sends content with lossy hash `L1`, viewer stores in PersistentCache
2. **Server restarts** (or different server)
3. **Session 2**: 
   - Server has canonical hash `C`, computes new lossy hash `L2`
   - `L2 ≠ L1` (different JPEG quality or encoder)
   - Viewer has `L1` in cache, but server sends reference with `L2` → **cache miss!**

### Solution: Server Queries Viewer's Cache

Before sending a cache reference, server must verify viewer has the specific hash:

```
1. Server computes canonical hash C
2. Server checks: does viewer have C? (lossless from previous session)
3. If yes → send reference with C
4. If no → compute lossy hash L (encode→decode→hash)
5. Server checks: does viewer have L? (lossy from current encoding)
6. If yes → send reference with L  
7. If no → send full CachedRectInit with L
```

### Protocol Enhancement: Cache Availability Check

**Option 1: Explicit Query Message**
```cpp
// Server → Client
writer->writeCacheQuery(canonical_hash, lossy_hash);

// Client → Server (in response)
writer->writeCacheQueryResponse(has_canonical, has_lossy);

// Server decides what to send based on response
if (has_canonical) {
    writer->writePersistentCachedRect(rect, canonical_hash);
} else if (has_lossy) {
    writer->writePersistentCachedRect(rect, lossy_hash);
} else {
    writer->writePersistentCachedRectInit(rect, lossy_hash, encoding, data);
}
```

**Option 2: Piggyback on Existing Mechanism**

Extend the existing "knowsPersistentId" tracking:
```cpp
// During session init, client sends list of cache IDs it has
// (already happens implicitly via first lookups)

// Server maintains set of IDs viewer confirmed having
std::unordered_set<uint64_t> viewerConfirmedCache;

// Before sending reference:
bool viewerHasCanonical = viewerConfirmedCache.count(canonical_hash);
bool viewerHasLossy = viewerConfirmedCache.count(lossy_hash);

if (viewerHasCanonical) {
    // Use lossless hash (best quality)
    writer->writePersistentCachedRect(rect, canonical_hash);
} else if (viewerHasLossy) {
    // Use lossy hash (current encoding)
    writer->writePersistentCachedRect(rect, lossy_hash);
} else {
    // Send full data with lossy hash
    writer->writePersistentCachedRectInit(rect, lossy_hash, encoding, data);
}
```

**Recommendation**: Option 2 is simpler and reuses existing infrastructure.

### How Viewer Confirms Cache Availability

When server sends `PersistentCachedRect(hash)` and viewer has it:
```cpp
// Client responds implicitly by NOT sending RequestCachedData
// Server marks this hash as confirmed in viewerConfirmedCache
```

When viewer doesn't have it:
```cpp
// Client sends RequestCachedData(hash)
// Server removes hash from viewerConfirmedCache (was stale assumption)
// Server sends PersistentCachedRectInit(hash, data)
```

## Implementation Requirements

### 1. Server-Side Data Structures

```cpp
class SConnection {
  // Map canonical hash → lossy hash (for current encoding parameters)
  std::unordered_map<uint64_t, uint64_t> lossyHashCache;
  
  // Track which IDs viewer has confirmed having in its cache
  std::unordered_set<uint64_t> viewerConfirmedCache;
  
  // Track optimistic assumptions (sent reference, awaiting confirmation)
  std::unordered_set<uint64_t> viewerPendingConfirmation;
};
```

### 2. Server Cache Lookup Logic

```cpp
uint64_t canonical_hash = computeCanonicalHash(rect);
uint64_t lossy_hash = 0;
bool needsLossyHash = isLossyEncoding(encoding);

if (needsLossyHash) {
    // Check if we already computed lossy hash for this content
    auto it = lossyHashCache.find(canonical_hash);
    if (it != lossyHashCache.end()) {
        lossy_hash = it->second;
    } else {
        // First time: encode→decode→hash
        lossy_hash = computeLossyHash(rect, encoding);
        lossyHashCache[canonical_hash] = lossy_hash;
    }
}

// Check which hash viewer has
bool viewerHasCanonical = viewerConfirmedCache.count(canonical_hash) > 0;
bool viewerHasLossy = needsLossyHash && viewerConfirmedCache.count(lossy_hash) > 0;

if (viewerHasCanonical) {
    // Viewer has lossless version - use it (best quality)
    writer->writePersistentCachedRect(rect, canonical_hash);
    // Already confirmed, no need to track
} else if (viewerHasLossy) {
    // Viewer has lossy version from current encoding
    writer->writePersistentCachedRect(rect, lossy_hash);
    // Already confirmed, no need to track
} else {
    // Viewer doesn't have either - send full data
    uint64_t hash_to_send = needsLossyHash ? lossy_hash : canonical_hash;
    writer->writePersistentCachedRectInit(rect, hash_to_send, encoding, data);
    
    // Optimistically assume viewer will cache it
    viewerPendingConfirmation.insert(hash_to_send);
}
```

### 3. Viewer Response Handling

```cpp
// When viewer requests data (cache miss)
void SConnection::handleRequestCachedData(uint64_t cacheId) {
    // Viewer didn't have this ID - remove from confirmed set
    viewerConfirmedCache.erase(cacheId);
    viewerPendingConfirmation.erase(cacheId);
    
    // Send the data
    // ... (existing logic)
}

// After sending a frame update successfully
void SConnection::onFrameUpdateComplete() {
    // Move pending confirmations to confirmed set
    // (viewer didn't send RequestCachedData, so it has these IDs)
    for (uint64_t id : viewerPendingConfirmation) {
        viewerConfirmedCache.insert(id);
    }
    viewerPendingConfirmation.clear();
}
```

### 4. Decode Capability for Lossy Hash

Server needs to decode its own JPEG output:
```cpp
uint64_t computeLossyHash(const Rect& rect, int encoding) {
    // 1. Encode rect to JPEG
    std::vector<uint8_t> encoded = tightEncoder.encodeRect(rect, encoding);
    
    // 2. Decode back to pixels (simulate client)
    ManagedPixelBuffer tempBuf(rect.width(), rect.height(), clientPF);
    tightDecoder.decodeRect(rect, encoded.data(), encoded.size(), &tempBuf);
    
    // 3. Compute hash of decoded pixels
    std::vector<uint8_t> hash = ContentHash::computeRect(&tempBuf, rect);
    
    uint64_t hashId = 0;
    memcpy(&hashId, hash.data(), std::min(hash.size(), sizeof(uint64_t)));
    return hashId;
}
```

### 5. Eviction Handling

When content is evicted from cache:
```cpp
void onCacheEviction(uint64_t canonical_hash) {
    // Remove lossy hash mapping
    auto it = lossyHashCache.find(canonical_hash);
    if (it != lossyHashCache.end()) {
        uint64_t lossy_hash = it->second;
        lossyHashCache.erase(it);
        
        // Also remove from viewer tracking
        viewerConfirmedCache.erase(canonical_hash);
        viewerConfirmedCache.erase(lossy_hash);
    }
}
```

## Performance Considerations

### Cost
- Additional decode pass for lossy rects (JPEG decode)
- Only for cache-eligible rects (default: area >= 2048 pixels)
- Memory: ~8 bytes per lossy cached rect (hash mapping)

### Benefit
- 20-40x cache hit rate improvement (5% → 26%+ observed with partial fixes)
- Bandwidth savings: 95%+ for repeated content
- CPU savings: no decode needed for cache hits

### Optimization
Could make lossy hash computation optional via configuration:
```
LossyHashMode=auto|always|never
  auto:   Enable for Tight JPEG, H.264
  always: Enable for all encodings (testing)
  never:  Disable (current broken behavior)
```

## Alternative Considered: Client-Side Adaptive Tolerance

Rejected because:
- Can't reliably detect "close enough" hashes without false positives
- JPEG quality varies - no single tolerance works
- Lossy content legitimately changes (quality adjustments, artifacts)

## Next Steps

### Phase 1: Basic Lossy Hash Support (Single Session)
1. Add data structures to `SConnection`:
   - `lossyHashCache` (canonical → lossy mapping)
   - `viewerConfirmedCache` (IDs viewer has)
   - `viewerPendingConfirmation` (awaiting confirmation)

2. Implement `computeLossyHash()` in `EncodeManager`:
   - Encode rect with current parameters
   - Decode back to pixels
   - Compute hash of decoded pixels

3. Update cache lookup logic:
   - Check canonical hash first (best quality)
   - Fall back to lossy hash (current encoding)
   - Send full data if neither matches

4. Implement viewer confirmation tracking:
   - Mark IDs as pending after sending
   - Confirm after successful frame update
   - Remove on RequestCachedData (miss)

### Phase 2: Cross-Session Support
5. Persist lossy hash mappings:
   - Option A: Store in PersistentCache metadata
   - Option B: Recompute on server restart (deterministic if encoding params saved)

6. Handle viewer cache from different servers:
   - Server always checks both canonical and lossy
   - Viewer cache may have IDs from different JPEG quality settings
   - System gracefully falls back to full data + new hash

### Phase 3: Testing & Optimization
7. Add unit tests:
   - Lossless encoding (hash match)
   - Lossy encoding (hash computed correctly)
   - Cross-session cache persistence
   - Viewer confirmation tracking

8. Re-run e2e tests:
   - Verify hit rates >60% for repeated content
   - Confirm visual corruption eliminated
   - Check eviction notifications working

9. Performance tuning:
   - Cache decoded rects to avoid re-decoding
   - Limit lossy hash cache size
   - Add configuration options

## Expected Outcomes

- Cache hit rates should increase from 5% to 60%+ for repeated content
- Visual corruption tests should pass (hash mismatches resolved)
- Eviction notifications should start working (cache will actually be used)
