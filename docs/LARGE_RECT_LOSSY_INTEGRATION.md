# Large Rectangle Caching + Lossy Hash Integration

## Problem Identified

The large rectangle optimization code (bordered regions, bounding box caching, seed mechanism) has the **same hash mismatch issue** as regular cache lookups.

## Current Seed Mechanism Flow

**Server side** (EncodeManager.cxx:1225-1265):
```cpp
// 1. Encode individual subrects with lossy encoding (JPEG)
writeSubRect(rect, pb);  // Sends JPEG data

// 2. After all rects sent, seed the bounding box
uint64_t canonical_hash = hash(lossless_pixels);
writer->writeCachedRectSeed(bbox, canonical_hash);
```

**Client side** (DecodeManager.cxx:1089-1173):
```cpp
// 3. Receives seed message with canonical_hash
void seedCachedRect(rect, canonical_hash, pb) {
    // Client has lossy decoded pixels in framebuffer
    uint64_t client_hash = hash(decoded_lossy_pixels);
    
    if (client_hash != canonical_hash) {
        // REJECTED! Hash mismatch (line 1142-1150)
        vlog.info("seedCachedRect skipped: hash mismatch");
        persistentCache->invalidateByContentId(canonical_hash);
        return;  // Seed fails!
    }
    
    // Would store here if hashes matched
    persistentCache->insert(canonical_hash, pixels, ...);
}
```

**Result**: Seed is rejected for lossy encodings, cache stays empty.

## Three Affected Code Paths

### 1. Bordered Region Lookup (EncodeManager.cxx:1054-1106)
```cpp
// Compute canonical hash of bordered content region
uint64_t contentId = hash(lossless_pixels);

// Check if viewer has it
if (conn->knowsPersistentId(contentId)) {
    // Send reference - but viewer might not have canonical hash!
    writer->writePersistentCachedRect(rect, contentId);
}
```

**Problem**: Viewer has lossy hash `L`, server checks for canonical hash `C`.

### 2. Bounding Box Lookup (EncodeManager.cxx:1112-1162)
```cpp
// Check if bounding box matches cached content
uint64_t bboxId = hash(lossless_pixels);

if (conn->knowsPersistentId(bboxId)) {
    // Send reference - but viewer might not have canonical hash!
    writer->writePersistentCachedRect(bbox, bboxId);
}
```

**Problem**: Same as bordered regions.

### 3. Seed After Encoding (EncodeManager.cxx:1225-1265)
```cpp
// After encoding with JPEG, seed with canonical hash
uint64_t bboxId = hash(lossless_pixels);
writer->writeCachedRectSeed(bbox, bboxId);
```

**Problem**: Client will compute lossy hash and reject the seed.

## Solution: Integrated Lossy Hash Support

### Required Changes

#### 1. Track Encoding Type During writeUpdate

```cpp
class EncodeManager {
    // Track encoding used for the current update
    EncoderType currentEncodingType;
    bool currentEncodingIsLossy;
    
    void writeUpdate() {
        // Determine if current encoding is lossy
        currentEncodingIsLossy = isLossyEncoding(currentEncodingType);
        
        // ... rest of update logic
    }
};
```

#### 2. Enhanced Bordered Region Lookup

```cpp
// Compute canonical hash
uint64_t canonical_hash = ContentHash::compute(pb, contentRect);

// Check if viewer has canonical (best quality)
if (viewerHasCanonical(canonical_hash)) {
    writer->writePersistentCachedRect(contentRect, canonical_hash);
    return;
}

// If lossy encoding, compute and check lossy hash
if (currentEncodingIsLossy) {
    uint64_t lossy_hash = computeLossyHash(contentRect, currentEncodingType);
    
    if (viewerHasLossy(lossy_hash)) {
        writer->writePersistentCachedRect(contentRect, lossy_hash);
        return;
    }
}

// Viewer doesn't have either - seed after encoding
```

#### 3. Enhanced Bounding Box Logic

Same pattern as bordered regions:
1. Check canonical hash first
2. If not found and lossy encoding, compute and check lossy hash
3. Fall through to normal encoding + seed

#### 4. Seed with Correct Hash

```cpp
// After encoding all subrects
if (shouldSeedBbox) {
    uint64_t hashToSeed;
    
    if (currentEncodingIsLossy) {
        // Client will have lossy decoded pixels - seed with lossy hash
        hashToSeed = computeLossyHashOfClientFramebuffer(bbox);
    } else {
        // Lossless - client has exact pixels
        hashToSeed = canonical_hash;
    }
    
    writer->writeCachedRectSeed(bbox, hashToSeed);
    
    // Track both hashes on server
    if (currentEncodingIsLossy) {
        lossyHashCache[canonical_hash] = hashToSeed;
    }
}
```

### Helper Function: computeLossyHashOfClientFramebuffer

**Challenge**: After encoding subrects, we need to know what the client's framebuffer looks like.

**Option A - Server-Side Decode** (accurate but expensive):
```cpp
uint64_t computeLossyHashOfClientFramebuffer(Rect bbox) {
    // Create temp buffer simulating client framebuffer
    ManagedPixelBuffer clientFB(bbox.width(), bbox.height(), clientPF);
    
    // Re-encode and decode each subrect that was sent
    for (auto& subrect : encodedSubrects) {
        // 1. Re-encode with same parameters
        vector<uint8_t> encoded = encoder->encode(subrect);
        
        // 2. Decode to temp buffer
        decoder->decode(encoded, &clientFB, subrect);
    }
    
    // 3. Compute hash of simulated client framebuffer
    return ContentHash::compute(&clientFB, bbox);
}
```

**Option B - Progressive Hash** (efficient but complex):
```cpp
// During encoding, accumulate hash as we go
class ProgressiveHashTracker {
    HashContext ctx;
    Rect bbox;
    
    void onSubrectEncoded(Rect subrect, vector<uint8_t> encoded) {
        // Decode immediately
        ManagedPixelBuffer decoded = decode(encoded);
        
        // Update hash with this subrect's contribution
        ctx.update(decoded, subrect);
    }
    
    uint64_t finalize() {
        return ctx.getHash();
    }
};
```

**Recommendation**: Start with Option A (server-side decode) for correctness, optimize later if needed.

## Cross-Session Interaction

The dual-hash system must work with large rectangle caching:

```cpp
// Server checks BOTH hashes before sending reference
bool canUseBorderedRegion(Rect contentRect) {
    uint64_t canonical = hash(lossless_pixels);
    
    // Check if viewer has lossless version (best quality)
    if (viewerConfirmedCache.count(canonical)) {
        return true;  // Use canonical hash
    }
    
    // Check if viewer has lossy version from current encoding
    if (currentEncodingIsLossy) {
        uint64_t lossy = lossyHashCache[canonical];
        if (viewerConfirmedCache.count(lossy)) {
            return true;  // Use lossy hash
        }
    }
    
    // Viewer doesn't have either
    return false;
}
```

## Implementation Order

### Phase 1: Fix Seed Mechanism
1. Track encoding type during writeUpdate
2. Compute lossy hash for seed messages when needed
3. Update seed to use correct hash

**Impact**: Seeds will start working for lossy content

### Phase 2: Enhance Lookups
4. Update bordered region lookup to check both hashes
5. Update bounding box lookup to check both hashes
6. Integrate with viewer confirmation tracking

**Impact**: Large rectangle optimization will work across sessions

### Phase 3: Optimize
7. Cache decoded rects to avoid re-decoding
8. Implement progressive hash accumulation
9. Add performance metrics

## Expected Results

After integration:
- Seed messages will succeed for lossy content
- Large rectangle optimization will have 60%+ hit rates
- Cross-session cache persistence will work correctly
- Visual corruption eliminated

## Testing Strategy

1. **Unit test**: Seed with lossy encoding
   - Encode rect with JPEG
   - Compute expected lossy hash
   - Verify seed succeeds

2. **E2E test**: Large bordered content
   - Display image with border (e.g., browser window)
   - Verify bordered region cached
   - Close and reopen → verify cache hit

3. **E2E test**: Bounding box across sessions
   - Display tiled content
   - Stop server, restart with different JPEG quality
   - Verify graceful fallback (send full data with new hash)

## Performance Considerations

**Cost per large rect**:
- Option A: ~5-10ms for JPEG decode (320x240 rect)
- Option B: Incremental overhead during encoding (~1ms per subrect)

**Benefit**:
- Single 47-byte reference instead of 50KB+ of JPEG data
- ~1000x bandwidth savings for repeated content
- No client-side decode needed

**Trade-off**: Worth it for cache-eligible content (area >= 2048 pixels).
