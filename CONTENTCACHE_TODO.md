# ContentCache TODO - Future Optimizations

**Last Updated**: 2025-10-08  
**Status**: Current implementation functional but inefficient for cache miss recovery

---

## Current Implementation Status

### ✅ Working Features
- Client advertises ContentCache support via pseudo-encoding
- Server detects duplicate content and sends `CachedRect` references (20 bytes)
- Client stores decoded pixels with cache IDs
- Cache hits work perfectly - instant blit from memory
- Client detects cache misses and sends `RequestCachedData(cacheId)`
- Server responds by marking entire framebuffer as changed (forces full refresh)
- No corruption - correctness is guaranteed

### ⚠️ Current Limitation
**Cache Miss Recovery is Inefficient**

When client has a cache miss (e.g., first connection, cache eviction):
1. Client sends `RequestCachedData(cacheId)` ✅
2. Server marks **entire framebuffer** as changed 🐌
3. Server re-sends **all content** (not just the missing rect) 🐌
4. Client receives and caches the data ✅

**Impact**: 
- Works correctly (no corruption) ✅
- But wastes bandwidth for large screens 📊
- Example: Missing 64×64 tile triggers re-send of 3840×2160 framebuffer

---

## Proposed Optimization: Targeted Cache Miss Recovery

### Option A: Specific Rectangle Refresh (Recommended)

**Goal**: Only re-send the specific rectangle that was missed

#### Implementation Plan

1. **Server-side: Track Cache Entry Bounds**
   ```cpp
   // In EncodeManager or VNCServerST, maintain access to ContentCache
   ContentCache* getContentCache() { return contentCache; }
   ```

2. **Lookup Rectangle from Cache ID**
   ```cpp
   void VNCServerST::handleRequestCachedData(VNCSConnectionST* client,
                                             uint64_t cacheId)
   {
     // Get access to the EncodeManager's cache
     ContentCache* cache = getContentCache();  // Need to add this accessor
     
     // Look up the cache entry by ID
     ContentCache::CacheEntry* entry = cache->findByCacheId(cacheId);
     
     if (entry) {
       // Mark only the specific rectangle as changed
       core::Region missedRegion(entry->lastBounds);
       client->add_changed(missedRegion);
       
       slog.info("Cache miss for ID %llu - refreshing rect [%d,%d-%d,%d]",
                 cacheId, entry->lastBounds.tl.x, entry->lastBounds.tl.y,
                 entry->lastBounds.br.x, entry->lastBounds.br.y);
     } else {
       // Cache entry not found - fall back to full refresh
       slog.warning("Cache entry %llu not found - full refresh", cacheId);
       core::Region fullRegion({0, 0, pb->width(), pb->height()});
       client->add_changed(fullRegion);
     }
   }
   ```

3. **Required Changes**
   - [ ] Add `getContentCache()` accessor to VNCServerST or EncodeManager
   - [ ] Ensure cache entries persist with `lastBounds` information
   - [ ] Update `handleRequestCachedData()` in VNCServerST.cxx

**Benefits**:
- Only re-sends the specific missing rectangle
- Minimal bandwidth overhead
- Fast recovery from cache misses

**Complexity**: Low - just needs cache accessor plumbing

---

### Option B: Full CachedRectInit Response (Protocol-Complete)

**Goal**: Implement the full protocol spec - send `CachedRectInit` on demand

#### Protocol Design
Per the original ContentCache design:
1. Server stores encoded pixel data with cache entries
2. On `RequestCachedData`, server responds with `CachedRectInit` message
3. `CachedRectInit` contains: `cacheId` + `encoding` + encoded data
4. Client decodes and stores in cache

#### Implementation Plan

1. **Store Encoded Data in Cache**
   ```cpp
   // In EncodeManager::writeSubRect(), after encoding:
   
   // Capture the encoded data stream
   rdr::MemOutStream encodedData;
   encoder->writeRect(ppb, info.palette, &encodedData);
   
   // Store in cache with encoded data
   contentCache->insertContent(hash, rect, 
                               encodedData.data(), 
                               encodedData.length(),
                               true);  // keepData=true
   ```

2. **Send CachedRectInit on Request**
   ```cpp
   void VNCServerST::handleRequestCachedData(VNCSConnectionST* client,
                                             uint64_t cacheId)
   {
     ContentCache* cache = getContentCache();
     ContentCache::CacheEntry* entry = cache->findByCacheId(cacheId);
     
     if (entry && !entry->data.empty()) {
       // Send CachedRectInit with stored encoded data
       client->writer()->writeCachedRectInit(entry->lastBounds, 
                                            cacheId,
                                            entry->encoding,
                                            entry->data.data(),
                                            entry->data.size());
       slog.info("Sent CachedRectInit for cache ID %llu", cacheId);
     } else {
       // Fall back to marking region as changed
       // ... (Option A logic)
     }
   }
   ```

3. **Required Changes**
   - [ ] Extend `CacheEntry` to store encoded data and encoding type
   - [ ] Modify `insertContent()` to capture encoded stream
   - [ ] Implement `writeCachedRectInit()` to send pre-encoded data
   - [ ] Handle memory overhead (encoded data storage)

**Benefits**:
- Protocol-complete implementation
- Matches original design spec
- Potentially faster than re-encoding

**Drawbacks**:
- Increased memory usage (stores both decoded and encoded data)
- More complex implementation
- May not provide significant benefit over Option A

---

## Recommendation

**Start with Option A** (Specific Rectangle Refresh):
- Simple to implement
- Low memory overhead
- Sufficient for most use cases
- Can upgrade to Option B later if needed

**Consider Option B if**:
- Memory is not constrained
- You want protocol spec compliance
- You need to support very high-latency networks where re-encoding delay matters

---

## Related Files

- `common/rfb/VNCServerST.cxx` - Server handler implementation
- `common/rfb/EncodeManager.cxx` - Encoding and cache insertion
- `common/rfb/ContentCache.h/cxx` - Cache data structures
- `common/rfb/SMsgWriter.cxx` - Protocol message writers
- `CONTENTCACHE_PROTOCOL_BUG.md` - Original protocol bug report

---

## Testing Plan

After implementing optimization:

1. **Unit Tests**
   - Test cache miss recovery with known cache ID
   - Test cache miss with unknown cache ID
   - Verify only correct region is marked changed

2. **Integration Tests**
   - Connect client, let cache warm up
   - Force cache eviction on client
   - Verify only missing rects are re-sent (not full screen)
   - Monitor bandwidth usage

3. **Performance Tests**
   - Measure bandwidth before/after optimization
   - Test with various screen sizes
   - Test with multiple simultaneous cache misses

---

## Notes

- Current workaround (full framebuffer refresh) is **safe but slow**
- No data corruption with current implementation ✅
- Optimization can be done incrementally without breaking changes
- Consider making cache entry retention configurable

---

## Version History

- **2025-10-08**: Initial TODO created after implementing cache miss workaround
- **Current**: Full refresh workaround in place, targeted refresh pending
