# Content-Addressable Historical Cache Design

## Problem Statement

Currently, TigerVNC only compares the current framebuffer state with the immediately previous state. If content that was displayed minutes ago reappears (e.g., switching back to a previous window or document), it must be re-encoded and re-transmitted even though the client already has that exact pixel data.

## Proposed Solution

Implement a **content-addressable cache** that stores historical framebuffer chunks indexed by their content hash. When encoding an update, check if that content was previously transmitted, and if so, send a reference to it instead of re-encoding.

## Architecture Overview

### 1. Content Cache Structure

```cpp
// New class in common/rfb/ContentCache.h
class ContentCache {
private:
    struct CacheEntry {
        uint64_t contentHash;      // Hash of pixel data
        core::Rect bounds;         // Where this was last seen
        uint32_t lastSeenTime;     // Timestamp (for LRU eviction)
        uint32_t clientId;         // Which client(s) have this
        std::vector<uint8_t> data; // Optional: keep data for verification
    };
    
    // Hash -> Entry mapping
    std::unordered_map<uint64_t, std::vector<CacheEntry>> cache;
    
    // LRU tracking
    std::list<uint64_t> lruList;
    
    size_t maxCacheSize;           // Configurable limit
    size_t currentCacheSize;
    
public:
    // Check if content exists in cache
    std::optional<CacheEntry> findContent(const uint8_t* data, size_t len);
    
    // Add new content to cache
    void insertContent(uint64_t hash, const core::Rect& bounds, 
                      const uint8_t* data, size_t len);
    
    // Prune old entries (LRU eviction)
    void pruneCache();
    
    // Mark entry as recently used
    void touchEntry(uint64_t hash);
};
```

### 2. New Encoding: CacheRect

Similar to CopyRect, but references cached content by hash:

```
Message format:
- uint64_t contentHash    // Which cached content to use
- uint16_t destX, destY    // Where to place it
- uint16_t width, height   // Size (for verification)
```

Encoding number: Could use pseudo-encoding or real encoding number.

### 3. Integration Points

#### A. Server-Side: EncodeManager

Modify `EncodeManager::writeSubRect()`:

```cpp
void EncodeManager::writeSubRect(const core::Rect& rect, const PixelBuffer* pb)
{
    // Get pixel data for this rect
    const uint8_t* data = pb->getBuffer(rect, &stride);
    
    // Compute hash
    uint64_t hash = computeHash(data, rect.width() * rect.height() * bytesPerPixel);
    
    // Check cache
    auto cached = contentCache.findContent(hash);
    if (cached && client->supportsCacheRect()) {
        // Send CacheRect encoding
        writeCacheRect(rect, hash);
        contentCache.touchEntry(hash);
        return;
    }
    
    // Normal encoding path
    // ... existing code ...
    
    // Add to cache after encoding
    contentCache.insertContent(hash, rect, data, dataLen);
}
```

#### B. Client-Side: Decoder

```cpp
class CacheRectDecoder : public Decoder {
private:
    std::unordered_map<uint64_t, CachedContent> clientCache;
    
public:
    void decodeRect(const core::Rect& r, const uint8_t* buffer,
                   size_t buflen, const ServerParams& server,
                   ModifiablePixelBuffer* pb) override
    {
        rdr::MemInStream is(buffer, buflen);
        uint64_t hash = is.readU64();
        
        auto cached = clientCache.find(hash);
        if (cached == clientCache.end()) {
            // Cache miss - should not happen, log error
            // Fall back to requesting full update
            return;
        }
        
        // Copy from cache to framebuffer
        pb->imageRect(r, cached->second.data.data(), cached->second.stride);
    }
};
```

### 4. Cache Management

#### Configurable Parameters

Add new server parameters:

```cpp
// In ServerCore.h/cxx
static core::IntParameter contentCacheSize
("ContentCacheSize",
 "Size of historical content cache in MB (0=disabled)",
 100, 0, 10000);  // Default 100MB

static core::IntParameter contentCacheMinChunkSize
("ContentCacheMinChunkSize", 
 "Minimum chunk size to cache (in pixels)",
 64*64, 256, 1000000);

static core::IntParameter contentCacheMaxAge
("ContentCacheMaxAge",
 "Maximum age of cached content in seconds",
 300, 10, 3600);  // Default 5 minutes
```

#### LRU Eviction

```cpp
void ContentCache::pruneCache() {
    uint32_t now = getCurrentTime();
    
    // Remove expired entries
    for (auto& [hash, entries] : cache) {
        entries.erase(
            std::remove_if(entries.begin(), entries.end(),
                [now](const CacheEntry& e) {
                    return (now - e.lastSeenTime) > maxAge;
                }),
            entries.end()
        );
    }
    
    // LRU eviction if over size limit
    while (currentCacheSize > maxCacheSize && !lruList.empty()) {
        uint64_t hash = lruList.back();
        lruList.pop_back();
        evictEntry(hash);
    }
}
```

### 5. Hash Function

Use a fast hash function optimized for image data:

```cpp
uint64_t computeHash(const uint8_t* data, size_t len) {
    // Option 1: xxHash (very fast, good distribution)
    return XXH64(data, len, 0);
    
    // Option 2: CityHash
    // return CityHash64((const char*)data, len);
    
    // Option 3: Simple but fast for images - sample pixels
    // For large rects, only hash every Nth pixel to trade accuracy for speed
}
```

## Implementation Phases

### Phase 1: Basic Infrastructure (Week 1-2)
- [ ] Implement ContentCache class
- [ ] Add hash computation function
- [ ] Add configuration parameters
- [ ] Write unit tests for ContentCache

### Phase 2: Server-Side Integration (Week 3-4)
- [ ] Modify EncodeManager to check cache before encoding
- [ ] Implement CacheRect encoder
- [ ] Add cache statistics logging
- [ ] Test with simple scenarios

### Phase 3: Protocol Extension (Week 5)
- [ ] Define CacheRect message format
- [ ] Add pseudo-encoding for capability negotiation
- [ ] Update protocol documentation

### Phase 4: Client-Side Support (Week 6-7)
- [ ] Implement CacheRectDecoder
- [ ] Maintain client-side cache mirror
- [ ] Handle cache misses gracefully
- [ ] Test client/server interaction

### Phase 5: Optimization (Week 8)
- [ ] Tune hash function for performance
- [ ] Optimize cache lookup data structures
- [ ] Add adaptive chunking (vary rect sizes)
- [ ] Profile and optimize hot paths

### Phase 6: Testing & Refinement (Week 9-10)
- [ ] Test with various workloads
- [ ] Measure bandwidth savings
- [ ] Test cache eviction under memory pressure
- [ ] Handle edge cases (disconnects, resizes)

## Design Considerations

### Chunking Strategy

**Option A: Fixed Block Size (Recommended)**
- Use existing 64×64 or 128×128 blocks
- Simpler implementation
- Better cache hit rates for partial matches

**Option B: Adaptive Rectangles**
- Let EncodeManager's existing rect splitting handle it
- More flexible but complex
- May have lower hit rates

### Hash Collisions

- Use 64-bit hash (collision probability ~1 in 10^19)
- Optional: Keep full data for verification on collision
- Or: Use cryptographic hash (SHA-256) for zero collisions

### Per-Client vs. Shared Cache

**Per-Client Cache (Recommended for Phase 1)**
- Simpler: each client tracks what they have
- No synchronization issues
- More memory usage

**Shared Cache (Future Enhancement)**
- All clients share cache entries
- Requires tracking which clients have which entries
- Better memory efficiency

### Protocol Backward Compatibility

- Use pseudo-encoding for capability advertisement
- Server only uses CacheRect if client advertises support
- Falls back to normal encoding for old clients

## Performance Estimates

### Memory Usage
- 100MB cache = ~1300 uncompressed 1920×1080 frames
- Or ~83,000 64×64 chunks
- Per-entry overhead: ~100 bytes

### Bandwidth Savings (Estimated)
Scenario: User switches between 3 applications
- Without cache: Re-transmit ~8MB per switch
- With cache: Send ~10KB of CacheRect messages
- **Savings: 99.9%** for cache hits

Typical usage (30% cache hit rate):
- **Expected savings: 20-40% bandwidth reduction**

### CPU Overhead
- Hash computation: ~50-100 MB/s (single core)
- For 60 FPS @ 1080p with 50% change rate: ~500MB/s
- Hash cost: 1-2% CPU overhead
- **Net benefit: Significant (saves encoding cost)**

## Risks & Mitigations

### Risk 1: Hash Collisions
**Mitigation**: Use 64-bit hash + optional data verification

### Risk 2: Memory Pressure
**Mitigation**: Strict LRU eviction + configurable limits

### Risk 3: Client/Server Cache Desync
**Mitigation**: 
- Version numbers per entry
- Cache flush on resolution change
- Periodic cache validation

### Risk 4: Added Latency
**Mitigation**: 
- Fast hash function (< 1ms for typical rect)
- Async cache lookups
- Fall back to normal encoding if lookup too slow

## Testing Strategy

### Unit Tests
- ContentCache: insert, lookup, eviction
- Hash function: collision rate, performance
- Per-client cache tracking

### Integration Tests
- Full encode/decode cycle with cache
- Cache hit/miss scenarios
- Memory limits and eviction

### Performance Tests
- Benchmark hash computation speed
- Measure bandwidth savings in real workloads
- Profile CPU overhead

### Scenario Tests
- Window switching (high hit rate expected)
- Document scrolling (medium hit rate)
- Video playback (low hit rate, should not cache)

## Configuration Examples

### High Memory System (Development Workstation)
```bash
vncserver -ContentCacheSize=500 \
          -ContentCacheMaxAge=600 \
          -CompareFB=1
```

### Low Memory System (Raspberry Pi)
```bash
vncserver -ContentCacheSize=20 \
          -ContentCacheMaxAge=120 \
          -CompareFB=2
```

### Disable Caching
```bash
vncserver -ContentCacheSize=0
```

## Future Enhancements

1. **Compressed Cache Entries**: Store JPEG/PNG compressed data
2. **Persistent Cache**: Save cache to disk between sessions
3. **Multi-Client Synchronization**: Shared cache across clients
4. **Smart Prefetch**: Predict which cached content will be needed
5. **Partial Rect Matching**: Match sub-regions of cached content
6. **Adaptive Hash Functions**: Choose hash based on content type

## References

- [RFB Protocol Specification](https://github.com/rfbproto/rfbproto/blob/master/rfbproto.rst)
- [xxHash - Fast Hash Algorithm](https://github.com/Cyan4973/xxHash)
- VNC CopyRect encoding (existing implementation)
- TigerVNC EncodeManager architecture

## Open Questions

1. Should we hash raw pixels or encoded data?
   - **Recommendation**: Raw pixels (more cache hits)

2. What's the optimal cache entry size?
   - **Recommendation**: Start with 64×64, tune based on testing

3. Should client maintain full cache or just indices?
   - **Recommendation**: Full cache (simpler protocol)

4. How to handle pixel format changes?
   - **Recommendation**: Flush cache on format change

5. Should cache be per-connection or per-server?
   - **Recommendation**: Per-connection initially, shared later
