# ContentCache: Design and Implementation Guide

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Protocol Design](#protocol-design)
4. [Server Implementation](#server-implementation)
5. [Client Implementation](#client-implementation)
6. [Build System](#build-system)
7. [Configuration](#configuration)
8. [Performance Characteristics](#performance-characteristics)
9. [Debugging and Troubleshooting](#debugging-and-troubleshooting)
10. [Known Issues and Bug Fixes](#known-issues-and-bug-fixes)

## Overview

### Problem Statement

Traditional VNC implementations only compare the current framebuffer with the immediately previous state. When content that was displayed minutes ago reappears (e.g., switching back to a previous window, scrolling back in a document), it must be re-encoded and re-transmitted even though the client already received that exact pixel data.

### Solution

The **ContentCache** system implements a content-addressable historical cache that:

1. **Hashes pixel content** to create unique identifiers
2. **Tracks historical content** seen over a configurable time window
3. **References cached content** by sending small cache IDs instead of re-encoding
4. **Adapts to access patterns** using the ARC (Adaptive Replacement Cache) algorithm
5. **Synchronizes client-side cache** using a two-message protocol

### Benefits

- **97-99% bandwidth reduction** for repeated content (20 bytes vs full encoding)
- **Zero CPU decode cost** for cache hits (memory blit vs decompression)
- **Intelligent eviction** via ARC balances recency and frequency
- **Backward compatible** with standard VNC clients (capability negotiation)

### Implementation Status

**🎉 Rust Implementation Complete**: As of October 2024, a full ContentCache implementation is available in the Rust VNC viewer:
- **Location**: `rust-vnc-viewer/` directory  
- **Status**: Production-ready with 97-99% bandwidth reduction  
- **Documentation**: See `rust-vnc-viewer/PHASE8A_CONTENTCACHE_COMPLETE.md` for complete implementation details
- **Usage**: `njcvncviewer-rs` binary with ContentCache protocol support

## Architecture

### High-Level Flow

```
Server Side                           Client Side
-----------                           -----------
1. Detect changed region
2. Extract pixel data
3. Compute content hash         
4. Check ContentCache
   ├─ Hit: Send CachedRect ────────> Lookup cache ID
   │   (20 bytes)                     Blit pixels to framebuffer
   │
   └─ Miss: Send CachedRectInit ───> Decode pixels
       (full encoding + cache ID)     Store in cache with ID
       Insert into cache
```

### Core Components

#### Server Side (`common/rfb/`)

- **`ContentCache`** (`ContentCache.h/cxx`): Core cache data structure with ARC algorithm
- **`EncodeManager`** (`EncodeManager.h/cxx`): Integration point for cache lookups and insertions
- **`SMsgWriter`** (`SMsgWriter.h/cxx`): Protocol message serialization
- **`ServerCore`** (`ServerCore.h/cxx`): Configuration parameters

#### Client Side (`common/rfb/`)

- **`DecodeManager`** (`DecodeManager.h/cxx`): Cache lookup and pixel blitting
- **`CMsgReader`** (`CMsgReader.cxx`): Protocol message deserialization
- **`CMsgHandler`** (`CMsgHandler.h`): Handler interface for cache messages
- **`CConnection`** (`CConnection.h/cxx`): Capability negotiation and forwarding

#### Protocol Constants (`common/rfb/encodings.h`)

```cpp
// Encoding types for protocol messages
const int encodingCachedRect     = 0xFFFFFE00;  // Cache reference
const int encodingCachedRectInit = 0xFFFFFE01;  // Cache initialization

// Capability pseudo-encoding for negotiation
const int pseudoEncodingContentCache = 0xFFFFFE10;
```

## Protocol Design

### Message Format

#### 1. CachedRect (Cache Hit)

Sent when server finds content in cache that client should already have.

```
Byte order: Network (big-endian)

+--------------+-----------------+
| Field        | Type            |
+--------------+-----------------+
| x            | u16             |  Rectangle position
| y            | u16             |
| width        | u16             |  Rectangle dimensions  
| height       | u16             |
| encoding     | s32 = 0xFFFFFE00|  CachedRect encoding type
| cacheId      | u64             |  Cache identifier
+--------------+-----------------+
Total: 20 bytes
```

**Client action**: Look up `cacheId` in local cache, blit pixels to `(x, y, width, height)`.

#### 2. CachedRectInit (Cache Miss Recovery)

Sent when server knows content but client might not have it cached yet.

```
+--------------+-----------------+
| Field        | Type            |
+--------------+-----------------+
| x            | u16             |
| y            | u16             |
| width        | u16             |
| height       | u16             |
| encoding     | s32 = 0xFFFFFE01|  CachedRectInit encoding type
| cacheId      | u64             |  Cache identifier to store under
| actualEnc    | s32             |  Real encoding (Tight, ZRLE, etc.)
| pixelData    | variable        |  Encoded pixel data
+--------------+-----------------+
Total: 20 + sizeof(pixelData)
```

**Client action**: 
1. Decode `pixelData` using `actualEnc` decoder
2. Render to framebuffer at `(x, y, width, height)`
3. Store decoded pixels in cache with `cacheId`

### Capability Negotiation

The client advertises support by including `pseudoEncodingContentCache` (0xFFFFFE10) in its encoding list during connection setup. The server checks for this capability before sending any cache protocol messages.

```cpp
// Client side (CConnection::updateEncodings)
encodings.push_back(pseudoEncodingContentCache);

// Server side (EncodeManager)
if (conn->client.supportsEncoding(pseudoEncodingContentCache)) {
    // Safe to use ContentCache protocol
}
```

## Server Implementation

### ContentCache Class

Located in `common/rfb/ContentCache.h` and `ContentCache.cxx`.

#### Data Structures

```cpp
class ContentCache {
public:
    struct CacheEntry {
        uint64_t contentHash;         // FNV-1a hash of pixel data
        uint64_t cacheId;             // Protocol identifier (1, 2, 3, ...)
        core::Rect lastBounds;        // Last known screen position
        uint32_t lastSeenTime;        // Timestamp for age-based eviction
        uint32_t hitCount;            // Access count for statistics
        size_t dataSize;              // Byte size for memory accounting
        std::vector<uint8_t> data;    // Optional: actual pixel data
    };

private:
    // Main storage: hash -> cache entry
    std::unordered_map<uint64_t, CacheEntry> cache_;
    
    // Bidirectional mappings for protocol
    std::unordered_map<uint64_t, uint64_t> hashToCacheId_;  // hash -> ID
    std::unordered_map<uint64_t, uint64_t> cacheIdToHash_;  // ID -> hash
    
    // ARC algorithm structures
    std::list<uint64_t> t1_;  // Recently used once (recency)
    std::list<uint64_t> t2_;  // Frequently used (frequency)
    std::list<uint64_t> b1_;  // Ghost: evicted from T1
    std::list<uint64_t> b2_;  // Ghost: evicted from T2
    
    std::unordered_map<uint64_t, ListInfo> listMap_;  // Track list membership
    size_t p_;                 // Adaptive target size for T1
    
    // Client-side: decoded pixel storage
    std::unordered_map<uint64_t, CachedPixels> pixelCache_;
};
```

#### Core Operations

**Insert Content** (`insertContent`):
1. Compute content hash from pixel data
2. Check if hash already in cache (update if exists)
3. Check ghost lists B1/B2 for adaptive behavior
4. Apply ARC replacement policy to make room
5. Assign monotonic cache ID (starts at 1)
6. Insert into cache and appropriate ARC list (T1 or T2)
7. Register hash ↔ cache ID mappings

**Find Content** (`findContent` / `findByHash`):
1. Look up hash in main cache
2. If found, increment hit count and update timestamp
3. Apply ARC promotion: T1 → T2 on second access
4. Return cache entry with cache ID

**ARC Replacement** (`replace`):
- If cache full and new item needed:
  - Evict from T1 if `|T1| > p` (target), else evict from T2
  - Evicted items become ghosts (moved to B1 or B2)
  - Ghost lists guide adaptive parameter `p`:
    - Hit in B1 → increase `p` (favor recency)
    - Hit in B2 → decrease `p` (favor frequency)

### Hash Function

Uses **FNV-1a** (Fowler-Noll-Vo) hash for speed and good distribution:

```cpp
uint64_t computeContentHash(const uint8_t* data, size_t len) {
    const uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
    const uint64_t FNV_PRIME  = 0x100000001b3ULL;
    
    uint64_t hash = FNV_OFFSET;
    for (size_t i = 0; i < len; i++) {
        hash ^= data[i];
        hash *= FNV_PRIME;
    }
    return hash;
}
```

**Critical Bug Fix (Oct 7 2025)**: Stride is in **pixels**, not bytes!

```cpp
// WRONG - only hashes partial data
size_t dataLen = rect.height() * stride;

// CORRECT - multiply by bytesPerPixel
size_t dataLen = rect.height() * stride * bytesPerPixel;
hash = computeContentHash(buffer, dataLen);
```

This bug caused frequent hash collisions and visual corruption before being fixed.

For rectangles > 512×512 pixels, a **sampled hash** is used (hash every 4th pixel in each dimension) for performance:

```cpp
uint64_t computeSampledHash(const uint8_t* data, size_t width, size_t height,
                            size_t stride, size_t bytesPerPixel, size_t sampleRate) {
    uint64_t hash = FNV_OFFSET;
    for (size_t y = 0; y < height; y += sampleRate) {
        const uint8_t* row = data + (y * stride * bytesPerPixel);
        for (size_t x = 0; x < width; x += sampleRate) {
            const uint8_t* pixel = row + (x * bytesPerPixel);
            for (size_t b = 0; b < bytesPerPixel; b++) {
                hash ^= pixel[b];
                hash *= FNV_PRIME;
            }
        }
    }
    return hash;
}
```

### EncodeManager Integration

The `EncodeManager` class orchestrates rectangle encoding with cache integration.

#### Cache Lookup (`tryContentCacheLookup`)

Called **before** encoding each rectangle:

```cpp
bool EncodeManager::tryContentCacheLookup(const core::Rect& rect, const PixelBuffer* pb) {
    if (contentCache == nullptr) return false;
    if (rect.area() < Server::contentCacheMinRectSize) return false;
    if (!conn->client.supportsEncoding(pseudoEncodingContentCache)) return false;
    
    // Get pixel data
    const uint8_t* buffer;
    int stride;
    buffer = pb->getBuffer(rect, &stride);
    
    // Compute hash (sampled for large rects)
    uint64_t hash;
    size_t bytesPerPixel = pb->getPF().bpp / 8;
    if (rect.area() > 262144) {  // 512×512
        hash = computeSampledHash(buffer, rect.width(), rect.height(),
                                  stride, bytesPerPixel, 4);
    } else {
        size_t dataLen = rect.height() * stride * bytesPerPixel;
        hash = computeContentHash(buffer, dataLen);
    }
    
    // Look up in cache
    uint64_t cacheId = 0;
    ContentCache::CacheEntry* entry = contentCache->findByHash(hash, &cacheId);
    
    if (entry && cacheId != 0) {
        // Cache hit! Send CachedRect message
        conn->writer()->writeCachedRect(rect, cacheId);
        contentCache->touchEntry(hash);  // Update LRU
        return true;
    }
    
    return false;  // Cache miss, use normal encoding
}
```

#### Cache Initialization (`writeSubRect` with CachedRectInit)

For cache misses, send `CachedRectInit` instead of plain encoding:

```cpp
void EncodeManager::writeSubRect(const core::Rect& rect, const PixelBuffer* pb) {
    // Try cache lookup first
    if (tryContentCacheLookup(rect, pb))
        return;  // Cache hit handled
    
    // Determine encoder type (Solid, Indexed, FullColour, etc.)
    EncoderType type = analyseRect(rect, pb);
    
    // If ContentCache enabled, wrap encoding in CachedRectInit
    bool usedCachedInit = false;
    if (contentCache && client.supportsContentCache && 
        rect.area() >= minRectSize) {
        
        // Compute hash
        const uint8_t* buffer;
        int stride;
        buffer = pb->getBuffer(rect, &stride);
        size_t bytesPerPixel = pb->getPF().bpp / 8;
        size_t dataLen = rect.height() * stride * bytesPerPixel;
        uint64_t hash = computeContentHash(buffer, dataLen);
        
        // Insert and get cache ID
        uint64_t cacheId = contentCache->insertContent(hash, rect, nullptr, 0, false);
        
        // Send CachedRectInit header
        Encoder* encoder = encoders[activeEncoders[type]];
        conn->writer()->writeCachedRectInit(rect, cacheId, encoder->encoding);
        
        // Encode pixel payload
        encoder->writeRect(preparePixelBuffer(rect, pb), palette);
        conn->writer()->endRect();
        
        usedCachedInit = true;
    }
    
    if (!usedCachedInit) {
        // Fallback: normal encoding + cache insertion
        Encoder* encoder = startRect(rect, type);
        encoder->writeRect(preparePixelBuffer(rect, pb), palette);
        endRect();
        insertIntoContentCache(rect, pb);
    }
}
```

## Client Implementation

### DecodeManager Integration

The `DecodeManager` handles cache operations on the client side.

#### Initialization

```cpp
DecodeManager::DecodeManager(CConnection* conn)
  : conn(conn), contentCache(nullptr) {
    // Initialize client-side cache (256MB default, 5 min max age)
    contentCache = new ContentCache(256, 300);
}
```

#### Handle CachedRect (Cache Hit)

```cpp
void DecodeManager::handleCachedRect(const core::Rect& r, uint64_t cacheId,
                                     ModifiablePixelBuffer* pb) {
    // Look up decoded pixels by cache ID
    const ContentCache::CachedPixels* cached = contentCache->getDecodedPixels(cacheId);
    
    if (!cached) {
        vlog.error("CachedRect cache miss for ID %llu", cacheId);
        // Could request full refresh here
        return;
    }
    
    // Verify dimensions match
    if (cached->width != r.width() || cached->height != r.height()) {
        vlog.error("CachedRect size mismatch");
        return;
    }
    
    // Blit cached pixels to framebuffer
    pb->imageRect(cached->format, r, cached->pixels.data(), cached->stride);
}
```

#### Store CachedRectInit (Cache Population)

```cpp
void DecodeManager::storeCachedRect(const core::Rect& r, uint64_t cacheId,
                                    ModifiablePixelBuffer* pb) {
    // Extract pixels from framebuffer (after decoding)
    int stride;
    const uint8_t* buffer = pb->getBuffer(r, &stride);
    
    // Store in cache with cache ID
    size_t bytesPerPixel = pb->getPF().bpp / 8;
    contentCache->storeDecodedPixels(
        cacheId, buffer, pb->getPF(),
        r.width(), r.height(), stride
    );
}
```

### CMsgReader Protocol Parsing

Located in `common/rfb/CMsgReader.cxx`:

```cpp
void CMsgReader::readRect(const core::Rect& r, int encoding) {
    switch (encoding) {
    case encodingCachedRect: {
        // Read cache ID
        uint64_t cacheId = is->readU64();
        handler->handleCachedRect(r, cacheId);
        break;
    }
    
    case encodingCachedRectInit: {
        // Read cache ID and actual encoding
        uint64_t cacheId = is->readU64();
        int actualEncoding = is->readS32();
        
        // Decode using actual encoder
        readRect(r, actualEncoding);
        
        // Store decoded pixels in cache
        handler->storeCachedRect(r, cacheId);
        break;
    }
    
    // ... other encodings ...
    }
}
```

## Build System

### Building Server (Xvnc)

The TigerVNC build system has **two separate build processes**:

1. **CMake build** (for libraries, vncviewer, utilities)
2. **Autotools build** (for Xvnc server - integrates with Xorg)

#### CMake Build (Libraries and Viewer)

```bash
# From tigervnc root
cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build build -j$(nproc)

# This builds:
# - build/common/rfb/libvnc.a (RFB protocol library with ContentCache)
# - build/vncviewer/vncviewer (VNC client)
# - build/unix/vncpasswd/vncpasswd
# - build/unix/x0vncserver/x0vncserver
```

#### Xvnc Server Build (Integrated with Xorg)

The Xvnc server requires building against patched Xorg source:

```bash
# 1. Extract Xorg server source (handled by unix/xserver/configure)
cd build/unix/xserver

# 2. Configure with patches applied
./configure --prefix=/usr/local --with-pic --without-dtrace \
    --disable-static --disable-dri --enable-dri2 --enable-dri3 \
    --enable-glx --disable-xorg --disable-xnest --disable-xvfb \
    --disable-dmx --disable-xwin --disable-xephyr --disable-kdrive

# 3. Build Xvnc
make -j$(nproc)

# Result: build/unix/xserver/hw/vnc/Xvnc
```

**Important**: The CMake build creates `libvnc.la` (with ContentCache), which is linked into Xvnc during the Xorg build.

#### Binary Location Confusion

Multiple binaries/symlinks exist:

- **Source of truth**: `build/unix/xserver/hw/vnc/Xvnc` (actual built binary)
- **Symlink (may be stale)**: `build/unix/vncserver/Xtigervnc`
- **System binary**: `/usr/bin/Xtigervnc` (system installation, not your build)

**Fix stale symlink**:
```bash
ln -sf /home/nickc/code/tigervnc/build/unix/xserver/hw/vnc/Xvnc \
       /home/nickc/code/tigervnc/build/unix/vncserver/Xtigervnc
```

### Rebuilding After Code Changes

#### For RFB Protocol Changes (ContentCache)

```bash
# 1. Build common libraries with CMake
cd /home/nickc/code/tigervnc/build
cmake --build . -j$(nproc)

# 2. Rebuild Xvnc server to pick up new libvnc.la
cd /home/nickc/code/tigervnc/build/unix/xserver
make -j$(nproc)

# 3. Verify binary timestamp
ls -lh hw/vnc/Xvnc
```

#### For Client Changes

```bash
# Rebuild viewer only
cd /home/nickc/code/tigervnc/build
cmake --build . --target vncviewer -j$(nproc)
```

### Testing Your Build

```bash
# Check which binary is actually running
ps aux | grep Xtigervnc

# Verify binary used is your build, not system
readlink -f /home/nickc/code/tigervnc/build/unix/vncserver/Xtigervnc

# Check build timestamp
ls -lh /home/nickc/code/tigervnc/build/unix/xserver/hw/vnc/Xvnc
```

## Configuration

### Server Parameters

Add to `~/.vnc/config` or pass on command line:

```bash
# Enable ContentCache
ContentCache=1

# Cache size in MB (default: 2048 MB)
ContentCacheSize=2048

# Maximum age for cached entries in seconds (default: 300 = 5 minutes)
ContentCacheMaxAge=300

# Minimum rectangle size to cache in pixels (default: 4096 = 64x64)
ContentCacheMinRectSize=4096
```

### Server Code (`ServerCore.cxx`)

```cpp
BoolParameter Server::enableContentCache
("ContentCache",
 "Enable content-addressable historical cache for repeated content",
 true);

IntParameter Server::contentCacheSize
("ContentCacheSize",
 "Size of content cache in MB",
 2048, 0, 10000);

IntParameter Server::contentCacheMaxAge
("ContentCacheMaxAge",
 "Maximum age of cached content in seconds",
 300, 10, 3600);

IntParameter Server::contentCacheMinRectSize
("ContentCacheMinRectSize",
 "Minimum rectangle size to cache (in pixels)",
 4096, 1024, 1000000);
```

### Client Configuration

Currently hard-coded in `DecodeManager` constructor:

```cpp
contentCache = new ContentCache(
    256,   // 256 MB cache size
    300    // 5 minute max age
);
```

Could be exposed as viewer parameters in future.

## Performance Characteristics

### Bandwidth Savings

For repeated content:

- **Traditional encoding**: ~30-70% compression (Tight/ZRLE)
- **CachedRect**: 20 bytes (rectangle header + cache ID)
- **Savings**: 97-99% reduction for cache hits

Example (64×64 tile at 32bpp):
- Uncompressed: 16,384 bytes
- Tight encoding: ~5,000 bytes (70% compression)
- CachedRect: 20 bytes (99.6% reduction vs Tight)

### CPU Savings

- **Encode**: Server doesn't re-compress cached content
- **Decode**: Client zero-cost blit vs decompression (Tight/ZRLE decoding eliminated)
- **Memory**: Cache overhead ~16KB per cached 64×64 tile

### Cache Statistics

Logged hourly by server:

```
ContentCache: === ARC Cache Statistics ===
ContentCache: Hit rate: 23.5% (1234 hits, 4032 misses, 5266 total)
ContentCache: Memory: 156MB / 2048MB (7.6% used), 10245 entries, 23 evictions
ContentCache: ARC balance: T1=8245 (80.5%, target 45.2%), T2=2000 (19.5%)
ContentCache: Ghost lists: B1=145, B2=67 (adaptation hints)
```

Interpretation:
- **Hit rate**: Percentage of rectangles served from cache
- **T1 vs T2**: Recency (T1) vs frequency (T2) balance
- **Ghost lists**: Guide adaptive behavior (higher B1 → favor recency)

## Debugging and Troubleshooting

### Enable Verbose Logging

```bash
# Server side
Xtigervnc :2 -Log *:stderr:100

# Look for ContentCache messages
tail -f ~/.vnc/quartz:2.log | grep -i contentcache
```

### Common Issues

#### 1. Cache Not Used (0% Hit Rate)

**Symptoms**: Statistics show all misses, no hits.

**Causes**:
- Client doesn't support ContentCache (not sending `pseudoEncodingContentCache`)
- Rectangle size below `ContentCacheMinRectSize` threshold
- Content changing every frame (no repetition)

**Diagnosis**:
```bash
# Check client capability negotiation
grep "supportsEncoding.*ContentCache" ~/.vnc/quartz:2.log

# Check rectangle sizes
grep "EncodeManager.*writeSubRect" ~/.vnc/quartz:2.log
```

#### 2. Visual Corruption

**Symptoms**: Wrong pixels displayed, "trails" or stale content.

**Likely Causes**:
- **Hash collision** (extremely rare with 64-bit FNV-1a)
- **Stride bug** (FIXED Oct 7 2025): Hash computed on wrong byte length
- **Client/server cache desync**: Client missing cache entry

**Diagnosis**:
```bash
# Check for cache misses on client
grep "CachedRect cache miss" client.log

# Verify hash calculation includes all pixel data
# (stride * height * bytesPerPixel)
```

#### 3. High Memory Usage

**Symptoms**: Server/client RSS growing beyond cache size limit.

**Causes**:
- Too many small entries (overhead per entry)
- ARC not evicting (ghost lists too large)
- Memory leak in cache code

**Diagnosis**:
```bash
# Check cache statistics
grep "Memory:" ~/.vnc/quartz:2.log | tail -1

# Monitor process RSS
ps aux | grep Xtigervnc
```

### Instrumentation Points

Add debug logging:

```cpp
// In EncodeManager::tryContentCacheLookup
vlog.debug("ContentCache lookup: rect [%d,%d-%d,%d] hash=%016llx %s",
           rect.tl.x, rect.tl.y, rect.br.x, rect.br.y,
           (unsigned long long)hash,
           (entry ? "HIT" : "MISS"));

// In DecodeManager::handleCachedRect
vlog.debug("CachedRect: cacheId=%llu found=%s",
           (unsigned long long)cacheId,
           (cached ? "yes" : "NO"));
```

## Known Issues and Bug Fixes

### Bug: Incorrect Hash Calculation (CRITICAL - Fixed Oct 7 2025)

**Commit**: `456e7c6d` - "Fix ContentCache hash calculation: stride is in pixels not bytes"

**Problem**: 
In `EncodeManager.cxx` lines 1258 and 1326, hash calculation was:

```cpp
size_t dataLen = rect.height() * stride;  // WRONG!
```

`PixelBuffer::getBuffer()` returns stride in **pixels**, not bytes. This caused only a fraction of the pixel data to be hashed.

**Impact**:
- Frequent hash collisions (different rectangles got same hash)
- Wrong cache lookups (client received wrong cached content)
- Severe visual corruption

**Fix**:
```cpp
size_t dataLen = rect.height() * stride * bytesPerPixel;  // CORRECT
```

Always multiply stride by `bytesPerPixel` to get byte length.

**Affected Code Paths**:
- `tryContentCacheLookup()` - line 1258
- `insertIntoContentCache()` - line 1326
- `writeSubRect()` CachedRectInit path - line 980 (was already correct!)

### Other Considerations

#### Hash Collision Probability

With 64-bit FNV-1a hashes:
- Birthday paradox: ~50% collision chance after 2^32 unique entries (~4 billion)
- For typical VNC usage with thousands of cached rectangles: negligible risk

If collisions become an issue:
- Switch to 128-bit hash (MurmurHash3, xxHash)
- Add content verification (compare actual pixels on hash match)

#### ARC Tuning

Default parameters work well for typical desktop usage, but consider:

- **Heavy scrolling**: Increase cache size, favor recency (monitor T1/T2 ratio)
- **Window switching**: Default balanced mode is optimal
- **Video playback**: ContentCache not beneficial (disable or use H.264 encoding)

#### CopyRect Interaction

The traditional CopyRect encoding has been **disabled when ContentCache is active** to avoid conflicts. CopyRect references currently-visible pixels, while ContentCache references historical content. Mixing them caused "window trail" corruption when dragging up/left.

See `EncodeManager::tryContentCacheLookup()`:

```cpp
// We do not fall back to CopyRect because the cache tracks historical positions,
// and CopyRect must only reference currently-visible content. Falling back here
// can copy stale content and cause window trails (especially when dragging up/left).
if (!conn->client.supportsEncoding(pseudoEncodingContentCache))
    return false;  // Don't use CopyRect either
```

---

## Summary

The ContentCache system provides substantial bandwidth and CPU savings for repeated content by maintaining a content-addressable historical cache synchronized between server and client. The ARC eviction algorithm adapts to access patterns, and the two-message protocol ensures cache consistency.

Key implementation details:
- **64-bit FNV-1a hashing** of pixel data (mind the stride units!)
- **ARC algorithm** balances recency and frequency
- **Protocol messages**: CachedRect (20 bytes) and CachedRectInit (20 + encoding)
- **Dual build system**: CMake for libraries, Autotools for Xvnc
- **Capability negotiation**: Client advertises support, server opts in

For questions or issues, check the VNC logs and cache statistics, and ensure both server and client are using builds with ContentCache support enabled.
