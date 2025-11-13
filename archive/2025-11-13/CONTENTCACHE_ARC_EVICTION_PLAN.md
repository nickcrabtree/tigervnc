# ContentCache ARC Eviction Implementation Plan

**Date**: 2025-11-04  
**Status**: Planning  
**Goal**: Implement proper ARC cache management on both client and server, with eviction notifications to keep per-client cache ID tracking accurate.

---

## Current State

### What Works ✅
- Server maintains per-connection `knownCacheIds_` set (VNCSConnectionST.h:220)
- Server inserts into cache after encoding and queues CachedRectInit
- Client receives CachedRect/CachedRectInit messages
- Client stores decoded pixels in `pixelCache_` map
- Protocol achieves 84%+ hit rates in testing
- Server-side ARC cache manages content hashes with eviction

### What's Missing ❌
- Client-side pixel storage uses simple map, no ARC eviction
- No client→server eviction notifications
- Server doesn't know when client drops cache entries
- No byte size tracking in statistics
- ARC statistics on client show 0% (not tracking pixel cache)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        SERVER                                │
├─────────────────────────────────────────────────────────────┤
│ ContentCache (server-side)                                   │
│   - cache_ (hash → CacheEntry) [ARC-managed]                │
│   - Tracks content hashes                                    │
│   - Evicts based on ARC algorithm                           │
│                                                              │
│ Per-Connection State (VNCSConnectionST)                     │
│   - knownCacheIds_ (set<uint64_t>)                          │
│   - Tracks which IDs *this* client has                      │
│   - Updated when:                                            │
│     • CachedRectInit sent → add ID                          │
│     • Client eviction notification → remove IDs             │
│     • Connection close → clear all                          │
└─────────────────────────────────────────────────────────────┘
                           │
                           │ RFB Protocol
                           │ ┌─ CachedRect (reference)
                           │ ├─ CachedRectInit (full data + ID)
                           │ └─ CacheEviction (client→server)
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                        CLIENT                                │
├─────────────────────────────────────────────────────────────┤
│ ContentCache (client-side)                                   │
│   - pixelCache_ (cacheId → CachedPixels) [needs ARC]       │
│   - Stores decoded pixel data                               │
│   - Should evict based on ARC algorithm                     │
│   - On eviction → notify server                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Implementation Phases

### Phase 1: Foundation - Byte Size Tracking
**Complexity**: Low  
**Risk**: None  
**Value**: Immediate visibility into cache memory usage

#### Tasks
- [ ] **1.1** Add `getTotalBytes()` method to ContentCache
  - Calculate from `cache_` map (server) and `pixelCache_` (client)
  - Return size in bytes
- [ ] **1.2** Update DecodeManager statistics output
  - Add "Hash cache size" for server
  - Add "Pixel cache size" for client
  - Format as MB with percentage of max
- [ ] **1.3** Update EncodeManager statistics output
  - Show server cache byte usage
- [ ] **1.4** Test and verify reporting
  - Run manual test
  - Verify sizes are reasonable
  - **Commit**: "Add byte size tracking to ContentCache statistics"

**Files to modify**:
- `common/rfb/ContentCache.h` (add getTotalBytes method)
- `common/rfb/ContentCache.cxx` (implement getTotalBytes)
- `common/rfb/DecodeManager.cxx` (update logStats)
- `common/rfb/EncodeManager.cxx` (update logStats)

---

### Phase 2: Protocol Extension - Eviction Messages
**Complexity**: Medium  
**Risk**: Low (additive change)  
**Value**: Required for proper cache synchronization

#### Tasks
- [ ] **2.1** Define new encoding constant
  - Add `encodingCacheEviction = 104` to encodings.h
  - Document in header comment
- [ ] **2.2** Add CMsgWriter::writeCacheEviction()
  - Takes vector of cache IDs to evict
  - Format: count (U32) + IDs (U64[])
- [ ] **2.3** Add SMsgReader::readCacheEviction()
  - Parse eviction message
  - Call handler on connection
- [ ] **2.4** Add SConnection::handleCacheEviction() virtual method
  - Takes vector of cache IDs
  - Default implementation logs warning
- [ ] **2.5** Implement VNCSConnectionST::handleCacheEviction()
  - Remove IDs from `knownCacheIds_`
  - Log eviction count
- [ ] **2.6** Test protocol with dummy messages
  - Manually send eviction from client
  - Verify server receives and processes
  - **Commit**: "Add ContentCache eviction protocol messages"

**Files to modify**:
- `common/rfb/encodings.h`
- `common/rfb/CMsgWriter.h/cxx`
- `common/rfb/SMsgReader.h/cxx`
- `common/rfb/SConnection.h/cxx`
- `common/rfb/VNCSConnectionST.h/cxx`

---

### Phase 3: Client-Side ARC Integration
**Complexity**: High  
**Risk**: Medium (changes core data structure)  
**Value**: Proper cache eviction and LRU/LFU management

#### 3A: Refactor pixel storage to use ARC

- [ ] **3.1** Create PixelCacheEntry struct in ContentCache.h
  ```cpp
  struct PixelCacheEntry {
    uint64_t cacheId;
    std::vector<uint8_t> pixels;
    PixelFormat format;
    int width, height, stridePixels;
    size_t bytes;  // Total byte size
  };
  ```
- [ ] **3.2** Change pixelCache_ to ARC-managed structure
  ```cpp
  // Old: std::unordered_map<uint64_t, CachedPixels> pixelCache_;
  // New: ARC managed with cacheId as key, PixelCacheEntry as value
  ```
- [ ] **3.3** Update storeDecodedPixels() to use ARC
  - Call `arc_.insert(cacheId, entry)` with byte size
  - Let ARC manage capacity
- [ ] **3.4** Update getDecodedPixels() to use ARC
  - Call `arc_.hit(cacheId)` on cache hit
  - Call `arc_.miss(cacheId)` on cache miss
  - Return pixels from ARC entry

#### 3B: Implement eviction callback

- [ ] **3.5** Add eviction callback to ARC template
  - Modify `common/rfb/ARC.h` to support eviction callback
  - Callback signature: `void onEvict(K key, V value)`
- [ ] **3.6** Register eviction callback in ContentCache
  - Pass callback to ARC constructor
  - Callback should queue IDs for batch notification
- [ ] **3.7** Add `pendingEvictions_` vector to ContentCache
  - Store IDs that need server notification
  - Flush on next update or when batch size reached

#### 3C: Wire up notifications

- [ ] **3.8** Add flushEvictionNotifications() method
  - Check if pendingEvictions_ is non-empty
  - Call CMsgWriter::writeCacheEviction()
  - Clear pendingEvictions_
- [ ] **3.9** Call flushEvictionNotifications() from DecodeManager
  - After each frame update completes
  - Before idle timeout
- [ ] **3.10** Test client-side ARC eviction
  - Reduce cache size to force evictions
  - Verify eviction notifications sent
  - Verify server receives and updates knownCacheIds_
  - **Commit**: "Implement client-side ARC with eviction notifications"

**Files to modify**:
- `common/rfb/ContentCache.h/cxx`
- `common/rfb/ARC.h` (add eviction callback support)
- `common/rfb/DecodeManager.h/cxx`
- `common/rfb/CConnection.h/cxx`

---

### Phase 4: Server-Side Enhancements
**Complexity**: Medium  
**Risk**: Low  
**Value**: Accurate tracking across reconnections

#### Tasks
- [ ] **4.1** Add byte size tracking to server cache insertions
  - Calculate bytes when inserting into cache_
  - Pass to ARC for capacity management
- [ ] **4.2** Verify knownCacheIds_ cleared on disconnect
  - Check VNCSConnectionST destructor
  - Ensure cleanup happens
- [ ] **4.3** Add periodic logging of knownCacheIds_ size
  - Every 100 updates or hourly
  - Shows how many IDs each client has
- [ ] **4.4** Test server with multiple concurrent clients
  - Connect 3 viewers simultaneously
  - Verify each has separate knownCacheIds_
  - Disconnect one, verify others unaffected
  - **Commit**: "Enhance server-side cache ID tracking"

**Files to modify**:
- `common/rfb/EncodeManager.cxx`
- `common/rfb/VNCSConnectionST.cxx`

---

### Phase 5: Testing & Validation
**Complexity**: Medium  
**Risk**: None  
**Value**: Ensure correctness

#### Tasks
- [ ] **5.1** Update test_contentcache_hits.sh
  - Check byte sizes reported
  - Verify eviction notifications in logs
  - Test with reduced cache size to force evictions
- [ ] **5.2** Add unit test for ARC eviction callback
  - Test in tests/unit/
  - Verify callback called on eviction
  - Verify correct IDs passed
- [ ] **5.3** Add multi-viewer test script
  - Start server
  - Connect 2 viewers
  - Generate different content patterns
  - Verify separate cache ID tracking
- [ ] **5.4** Test reconnection scenarios
  - Connect, disconnect, reconnect
  - Verify cache state resets properly
  - Verify no stale cache references
- [ ] **5.5** Performance testing
  - Measure overhead of eviction notifications
  - Verify < 1% CPU impact
  - **Commit**: "Add comprehensive ContentCache testing"

**Files to create**:
- `tests/unit/test_arc_eviction.cxx`
- `scripts/test_contentcache_multiviewer.sh`

---

### Phase 6: Documentation
**Complexity**: Low  
**Risk**: None  
**Value**: Essential for maintenance

#### Tasks
- [ ] **6.1** Update CONTENTCACHE_DESIGN_IMPLEMENTATION.md
  - Add eviction protocol section
  - Document per-client tracking
  - Add sequence diagrams
- [ ] **6.2** Update ARC_ALGORITHM.md
  - Document eviction callback mechanism
  - Add client-side usage examples
- [ ] **6.3** Add protocol specification
  - Document CacheEviction message format
  - Add to RFB protocol extensions doc
- [ ] **6.4** Update WARP.md if needed
  - Add any new test commands
  - **Commit**: "Document ContentCache eviction protocol"

**Files to modify**:
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`
- `ARC_ALGORITHM.md`
- Create: `docs/RFB_CACHE_PROTOCOL.md`

---

## Progress Checklist

### Phase 1: Byte Size Tracking
- [x] 1.1 Add getTotalBytes() method
- [x] 1.2 Update DecodeManager statistics
- [x] 1.3 Update EncodeManager statistics
- [x] 1.4 Test and commit

### Phase 2: Protocol Extension
- [x] 2.1 Define encoding constant
- [x] 2.2 Add CMsgWriter::writeCacheEviction()
- [x] 2.3 Add SMsgReader::readCacheEviction()
- [x] 2.4 Add SConnection::handleCacheEviction()
- [x] 2.5 Implement VNCSConnectionST handler
- [x] 2.6 Test and commit

### Phase 3: Client ARC Integration
- [x] 3.1 Create PixelCacheEntry struct
- [x] 3.2 Change pixelCache_ to ARC
- [x] 3.3 Update storeDecodedPixels()
- [x] 3.4 Update getDecodedPixels()
- [x] 3.5 Add ARC eviction callback
- [x] 3.6 Register callback in ContentCache
- [x] 3.7 Add pendingEvictions_ vector
- [x] 3.8 Add flushEvictionNotifications()
- [x] 3.9 Wire up flush calls
- [x] 3.10 Test and commit

### Phase 4: Server Enhancements
- [x] 4.1 Add byte size to server insertions
- [x] 4.2 Verify cleanup on disconnect
- [x] 4.3 Add periodic logging
- [x] 4.4 Test and commit

### Phase 5: Testing
- [x] 5.1 Update test script
- [ ] 5.2 Add unit test
- [ ] 5.3 Add multi-viewer test
- [ ] 5.4 Test reconnection
- [ ] 5.5 Performance test and commit

### Phase 6: Documentation
- [ ] 6.1 Update design doc
- [ ] 6.2 Update ARC doc
- [ ] 6.3 Add protocol spec
- [ ] 6.4 Commit documentation

---

## Risk Assessment

### High Risk Items
- **Phase 3.2**: Changing pixelCache_ data structure
  - **Mitigation**: Thorough testing, keep old code commented for rollback
  
### Medium Risk Items
- **Phase 3.5**: Modifying ARC template for callbacks
  - **Mitigation**: ARC is well-tested, callbacks are common pattern

### Low Risk Items
- All other phases are additive or isolated changes

---

## Estimated Timeline

- **Phase 1**: 2 hours (straightforward calculation)
- **Phase 2**: 4 hours (protocol boilerplate)
- **Phase 3**: 8 hours (complex refactoring)
- **Phase 4**: 3 hours (cleanup and verification)
- **Phase 5**: 6 hours (comprehensive testing)
- **Phase 6**: 3 hours (documentation)

**Total**: ~26 hours of focused work

---

## Success Criteria

- [x] ContentCache protocol works (84% hit rate achieved)
- [ ] Client and server both use ARC for cache management
- [ ] Eviction notifications sent reliably
- [ ] Server accurately tracks per-client cache IDs
- [ ] Multi-viewer scenario works correctly
- [ ] Byte sizes reported accurately
- [ ] ARC statistics show real hit/miss rates
- [ ] Memory usage stays within configured limits
- [ ] Performance overhead < 1%
- [ ] All tests pass
- [ ] Documentation complete

---

## Notes

- Each phase should be committed separately for easy rollback
- Test after each commit before proceeding
- Keep WARP.md safety rules in mind when testing
- Production servers (:1, :2, :3) must never be affected
- Use test displays (:998, :999) for development

---

## References

- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` - Current design
- `ARC_ALGORITHM.md` - ARC cache algorithm details
- `common/rfb/ContentCache.h/cxx` - Implementation
- `common/rfb/VNCSConnectionST.h` - Per-connection tracking
