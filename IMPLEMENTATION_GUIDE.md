# Content Cache Implementation Quick Start

## What You Have Now

I've created two files for you:

1. **`DESIGN_CONTENT_CACHE.md`** - Complete design document with:
   - Architecture overview
   - Implementation phases
   - Performance estimates
   - Testing strategy
   - Configuration examples

2. **`common/rfb/ContentCache.h`** - Proof-of-concept header file with:
   - ContentCache class definition
   - API for cache operations
   - Statistics tracking
   - LRU eviction logic

## Quick Start: Minimal Viable Implementation

If you want to get something working quickly, here's a simplified path:

### Step 1: Implement Basic ContentCache (1-2 days)

Create `common/rfb/ContentCache.cxx`:

```cpp
#include <rfb/ContentCache.h>
#include <time.h>

// Simple xxHash implementation (or use external library)
static uint64_t simpleHash(const uint8_t* data, size_t len) {
    uint64_t hash = 0xcbf29ce484222325ULL;
    for (size_t i = 0; i < len; i++) {
        hash ^= data[i];
        hash *= 0x100000001b3ULL;
    }
    return hash;
}

uint64_t rfb::computeContentHash(const uint8_t* data, size_t len) {
    return simpleHash(data, len);
}

// ... implement ContentCache methods ...
```

### Step 2: Add Server Parameters (30 minutes)

In `common/rfb/ServerCore.h`:
```cpp
static core::IntParameter contentCacheSize;
static core::IntParameter contentCacheMaxAge;
```

In `common/rfb/ServerCore.cxx`:
```cpp
core::IntParameter rfb::Server::contentCacheSize
("ContentCacheSize",
 "Size of historical content cache in MB (0=disabled)",
 100, 0, 10000);

core::IntParameter rfb::Server::contentCacheMaxAge
("ContentCacheMaxAge",
 "Maximum age of cached content in seconds",
 300, 10, 3600);
```

### Step 3: Integrate with EncodeManager (2-3 days)

**Option A: Server-Only (Simpler, No Protocol Changes)**

Modify `EncodeManager::writeSubRect()` to avoid re-encoding:

```cpp
void EncodeManager::writeSubRect(const core::Rect& rect, const PixelBuffer* pb)
{
    // Only cache if large enough
    if (rect.area() < MIN_CACHE_SIZE)
        goto normal_encode;
        
    // Compute hash
    int stride;
    const uint8_t* data = pb->getBuffer(rect, &stride);
    uint64_t hash = computeContentHash(data, 
        rect.width() * rect.height() * pb->getPF().bpp/8);
    
    // Check if we've encoded this exact content recently
    auto cached = contentCache.findContent(hash);
    if (cached) {
        // Content unchanged - use CopyRect to last known position
        // This works because client already has this data somewhere
        writeCopyRect(rect, (*cached)->lastBounds);
        contentCache.touchEntry(hash);
        return;
    }
    
normal_encode:
    // Normal encoding path
    // ... existing code ...
    
    // Add to cache after encoding
    if (rect.area() >= MIN_CACHE_SIZE) {
        contentCache.insertContent(hash, rect);
    }
}
```

**Benefit**: No protocol changes needed! Uses existing CopyRect.

**Limitation**: Client must not have overwritten the old location.

**Option B: Full Protocol Extension (Better, More Complex)**

Implement new CacheRect encoding as described in the design doc.

### Step 4: Test It (1 day)

Create a test program:

```bash
# Terminal 1: Start server with caching
./build/unix/x0vncserver -display :0 \
    -ContentCacheSize=100 \
    -ContentCacheMaxAge=300

# Terminal 2: Connect and test
./build/vncviewer localhost

# Test scenario:
# 1. Open terminal window
# 2. Switch to browser
# 3. Switch back to terminal
# 4. Watch logs for cache hits
```

Look for log messages like:
```
ContentCache: Hit rate: 45% (1234 hits, 1543 misses)
ContentCache: Saved 23MB bandwidth this session
```

## Recommended Approach

**For Initial Prototype (1 week):**

1. ✅ Implement ContentCache class with basic hash function
2. ✅ Add server configuration parameters  
3. ✅ Integrate with EncodeManager (server-only, use CopyRect)
4. ✅ Add logging/statistics
5. ✅ Test with real workloads

**For Production (Additional 2-3 weeks):**

6. Implement proper CacheRect protocol extension
7. Add client-side decoder
8. Implement proper cache synchronization
9. Add comprehensive tests
10. Performance tuning and optimization

## Key Files to Modify

### Must Modify:
- `common/rfb/ContentCache.cxx` (new) - Core cache implementation
- `common/rfb/EncodeManager.cxx` - Integration with encoding
- `common/rfb/ServerCore.h/cxx` - Configuration parameters
- `common/rfb/CMakeLists.txt` - Add new files to build

### May Modify:
- `common/rfb/encodings.h` - If adding new encoding number
- `common/rfb/VNCSConnectionST.h/cxx` - Per-client cache instance
- `vncviewer/CConn.cxx` - Client-side support (if full protocol)

## Build Commands

```bash
# Configure with your options
cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug

# Build
cmake --build build -- -j$(sysctl -n hw.ncpu)

# Test
./build/tests/unit/contentcache  # After adding unit tests
```

## Debugging Tips

### Enable Verbose Logging

```bash
# Set log level for ContentCache
./x0vncserver -Log=*:stderr:100,ContentCache:stderr:100
```

### Monitor Cache Performance

Add logging in `ContentCache::findContent()`:

```cpp
auto cached = contentCache.findContent(hash);
if (cached) {
    vlog.debug("Cache HIT: hash=%016llx, saved %zu bytes", 
               hash, dataSize);
    stats_.cacheHits++;
} else {
    vlog.debug("Cache MISS: hash=%016llx", hash);
    stats_.cacheMisses++;
}
```

### Visualize Cache State

Create a simple status dump:

```cpp
void ContentCache::dumpStats() {
    vlog.info("=== Content Cache Stats ===");
    vlog.info("  Entries: %zu", cache_.size());
    vlog.info("  Memory: %.1f MB", currentCacheSize_ / (1024.0*1024.0));
    vlog.info("  Hit rate: %.1f%%", 
              100.0 * stats_.cacheHits / 
              (stats_.cacheHits + stats_.cacheMisses));
    vlog.info("  Avg hits per entry: %.1f", 
              (double)stats_.cacheHits / cache_.size());
}
```

## Testing Scenarios

### High Cache Hit Rate (Window Switching)
```
1. Open 3 applications (terminal, browser, editor)
2. Cycle through them multiple times
3. Expected: >60% cache hit rate
```

### Medium Cache Hit Rate (Document Editing)
```
1. Open text editor
2. Type, scroll, undo/redo
3. Expected: 20-30% cache hit rate
```

### Low Cache Hit Rate (Video Playback)
```
1. Play video
2. Expected: <5% cache hit rate (every frame different)
3. Cache should not grow (filter out video content)
```

## Performance Targets

- **Hash computation**: < 1ms per 64×64 block
- **Cache lookup**: < 100μs
- **Memory usage**: Configurable, default 100MB
- **Bandwidth savings**: 20-40% typical, 90%+ for window switching

## Common Pitfalls

1. **Hash collisions**: Use 64-bit hash, consider verification
2. **Memory leaks**: Ensure proper LRU eviction
3. **Stale data**: Clear cache on resolution/format changes
4. **Performance**: Don't hash more than necessary
5. **Threading**: Cache may need locks if accessed from multiple threads

## Next Steps

After basic implementation works:

1. **Measure**: Collect real-world statistics
2. **Tune**: Adjust cache size, block size, hash function
3. **Optimize**: Profile hot paths, optimize data structures
4. **Extend**: Add compressed cache, persistent cache, etc.

## Getting Help

- Check `DESIGN_CONTENT_CACHE.md` for detailed architecture
- Look at existing `CopyRectDecoder` for protocol examples
- Review `ComparingUpdateTracker` for similar caching patterns
- TigerVNC mailing list: tigervnc-devel@googlegroups.com

## Success Criteria

Your implementation is working when:

- ✅ Cache hits are logged correctly
- ✅ Bandwidth usage decreases for repeated content
- ✅ No visual artifacts (verify hash accuracy)
- ✅ Memory usage stays within configured limits
- ✅ Performance overhead < 5% CPU

Good luck! This is an exciting enhancement that could significantly improve TigerVNC's efficiency.
