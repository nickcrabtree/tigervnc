# Cache-Based Rectangle Protocol Extension

## Overview

This document describes the TigerVNC cache-based rectangle protocol extension that allows servers and clients to maintain synchronized content caches, dramatically reducing bandwidth for repeated content.

## Protocol Extension

### Capability Negotiation

**Pseudo-encoding for capability announcement:**
```cpp
pseudoEncodingContentCache = -320
```

- Client includes this in its supported encodings list
- Server checks client.supportsEncoding(pseudoEncodingContentCache)
- If supported, server can send cache-based rectangles

### New Encoding Types

**1. CachedRect (Reference to cached content)**
```cpp
encodingCachedRect = 100
```

**2. CachedRectInit (First transmission with cache ID assignment)**
```cpp
encodingCachedRectInit = 101
```

## Message Formats

### CachedRect (Reference-only)

Used when server knows client has the content cached.

```
Rectangle header:
  x-position:     U16
  y-position:     U16
  width:          U16
  height:         U16
  encoding-type:  S32 (encodingCachedRect = 100)

CachedRect body:
  cache-id:       U64  (Which cache entry to use)
```

**Total size:** 12 bytes (rect header) + 8 bytes (cache ID) = **20 bytes**

Compare to:
- Raw RGB888 at 1920x1080 = 6,220,800 bytes
- CachedRect = 20 bytes
- **Compression ratio: 311,040:1**

### CachedRectInit (Initial transmission)

Used when sending content for the first time or re-synchronizing.

```
Rectangle header:
  x-position:     U16
  y-position:     U16
  width:          U16
  height:         U16
  encoding-type:  S32 (encodingCachedRectInit = 101)

CachedRectInit body:
  cache-id:          U64  (ID to assign this content)
  actual-encoding:   S32  (How pixels are encoded: Tight, ZRLE, etc.)
  pixels:            []   (Encoded pixel data)
```

**Flow:**
1. Server sends CachedRectInit with cache ID and encoded pixels
2. Client decodes pixels and stores them under cache ID
3. Future references use CachedRect with same cache ID

### Cache Control Messages

**Cache Clear**
```
Sent as a special rectangle:
  x-position:     0
  y-position:     0
  width:          0
  height:         0
  encoding-type:  S32 (encodingCachedRect = 100)
  cache-id:       U64 (0 = clear all, other = clear specific ID)
```

## Protocol Flow

### Scenario 1: New Content

```
┌────────┐                                           ┌────────┐
│ Server │                                           │ Client │
└────────┘                                           └────────┘
    │                                                     │
    │  Check: Is content in cache?                       │
    │  → NO (new content)                                │
    │                                                     │
    │  Assign cache ID: 12345                            │
    │                                                     │
    │  CachedRectInit(id=12345, Tight encoded pixels)    │
    │────────────────────────────────────────────────────>│
    │                                                     │
    │                         Client decodes and stores  │
    │                         in cache under ID 12345    │
    │                                                     │
```

### Scenario 2: Repeated Content (Cache Hit)

```
┌────────┐                                           ┌────────┐
│ Server │                                           │ Client │
└────────┘                                           └────────┘
    │                                                     │
    │  Check: Is content in cache?                       │
    │  → YES (cache ID 12345)                            │
    │                                                     │
    │  CachedRect(id=12345, target=(100,200))            │
    │────────────────────────────────────────────────────>│
    │                                                     │
    │                         Client looks up ID 12345   │
    │                         Draws cached content at    │
    │                         position (100, 200)        │
    │                                                     │
```

### Scenario 3: Cache Miss (Recovery)

```
┌────────┐                                           ┌────────┐
│ Server │                                           │ Client │
└────────┘                                           └────────┘
    │                                                     │
    │  CachedRect(id=67890, target=(500,500))            │
    │────────────────────────────────────────────────────>│
    │                                                     │
    │                         Lookup ID 67890: NOT FOUND │
    │                         (evicted from client cache)│
    │                                                     │
    │              Request Refresh (FBU request)         │
    │<────────────────────────────────────────────────────│
    │                                                     │
    │  CachedRectInit(id=67890, pixels)                  │
    │────────────────────────────────────────────────────>│
    │                                                     │
    │                         Store and display          │
    │                                                     │
```

## Cache Management

### Server-Side Cache

**ContentCache with Cache ID Management:**
```cpp
class ContentCache {
    // Existing: hash -> CacheEntry
    std::unordered_map<uint64_t, CacheEntry> cache_;
    
    // NEW: Cache ID management
    std::atomic<uint64_t> nextCacheId_;
    std::unordered_map<uint64_t, uint64_t> hashToCacheId_;  // hash -> ID
    std::unordered_map<uint64_t, uint64_t> cacheIdToHash_;  // ID -> hash
    
    uint64_t assignCacheId(uint64_t hash, const Rect& bounds);
    CacheEntry* findByCacheId(uint64_t cacheId);
    CacheEntry* findByHash(uint64_t hash, uint64_t* outCacheId);
};
```

**Server Algorithm:**
1. Compute content hash
2. Check if hash exists: `findByHash(hash, &cacheId)`
3. If found AND client supports cache protocol:
   - Send `CachedRect(cacheId, targetRect)`
4. If not found:
   - Assign new cache ID: `assignCacheId(hash, bounds)`
   - Send `CachedRectInit(cacheId, encoding, pixels)`

### Client-Side Cache

**ContentCache for Decoded Content:**
```cpp
class ContentCache {
    // Store decoded pixel data by cache ID
    struct CachedPixels {
        uint64_t cacheId;
        std::vector<uint8_t> pixels;
        PixelFormat format;
        int width;
        int height;
        int stride;
        uint32_t lastUsedTime;
    };
    
    std::unordered_map<uint64_t, CachedPixels> pixelCache_;
    
    void storeDecoded(uint64_t cacheId, const uint8_t* pixels,
                     const PixelFormat& pf, const Rect& bounds);
    const CachedPixels* getDecoded(uint64_t cacheId);
};
```

**Client Algorithm:**
1. Receive `CachedRect(id, target)`:
   - Lookup `getDecoded(id)`
   - If found: Blit pixels to target position
   - If not found: Send refresh request
   
2. Receive `CachedRectInit(id, encoding, pixels)`:
   - Decode pixels using specified encoding
   - Store: `storeDecoded(id, pixels, pf, bounds)`
   - Display decoded content

## Cache Synchronization

### Initial Connection

```
1. Client sends capabilities including pseudoEncodingContentCache
2. Server creates empty cache state for this client
3. All content sent as CachedRectInit initially
4. Cache IDs accumulate during session
```

### Cache Eviction Handling

**Server evicts entry:**
- Remove from hashToCacheId mapping
- Future identical content gets NEW cache ID
- Old cache ID becomes invalid
- Client may still have old ID cached (harmless)

**Client evicts entry:**
- Remove from pixelCache_
- If server sends CachedRect with evicted ID:
  - Client sends refresh request
  - Server sends CachedRectInit with same ID (re-sync)

### Cache Clearing

**Desktop resize:**
```
Server: contentCache->clear();
Server sends: CachedRect(id=0) [special "clear all" message]
Client: pixelCache_.clear();
```

## Performance Characteristics

### Best Case (Repeated Content)

**Scenario:** User switches between 5 application windows repeatedly

Without cache protocol:
- Each switch re-encodes entire window: ~500KB per window
- 10 switches = 5MB transmitted

With cache protocol:
- First view: 500KB (CachedRectInit)
- Each subsequent view: 20 bytes (CachedRect)
- 5 windows + 10 switches = 2.5MB + 200 bytes ≈ **2.5MB total**
- **Savings: 50%**

### Worst Case (All Unique Content)

**Scenario:** Video playback (every frame unique)

Without cache protocol:
- Each frame: 100KB (Tight encoding)

With cache protocol:
- Each frame: 100KB + 8 bytes overhead (CachedRectInit)
- **Overhead: 0.008%** (negligible)

### Memory Usage

**Server:**
- Cache stores only hashes + metadata: ~100 bytes per entry
- 10,000 entries = ~1MB
- No pixel data stored

**Client:**
- Stores decoded pixels: size depends on resolution
- 1920x1080 RGBA = 8MB per cached rect
- ARC eviction keeps memory bounded
- Default: 256MB cache = ~32 full-screen images

## Compatibility

### Fallback Behavior

**Legacy clients (no cache support):**
- Server detects: `!client.supportsEncoding(pseudoEncodingContentCache)`
- Server uses existing ContentCache for CopyRect optimization only
- No CachedRect/CachedRectInit sent
- Full backward compatibility ✓

**Modern clients:**
- Negotiate cache protocol
- Benefit from historical content caching
- Can interoperate with legacy servers (graceful degradation)

## Implementation Priority

1. ✅ Server-side ContentCache (DONE - basic version)
2. **Protocol constants** (encodings.h)
3. **Server cache ID management** (ContentCache enhancements)
4. **Message writers/readers** (SMsgWriter, CMsgReader)
5. **Encoder** (CachedRectEncoder)
6. **Client-side ContentCache** (DecodeManager integration)
7. **Decoder** (CachedRectDecoder)
8. **Cache sync protocol** (refresh requests, cache clear)
9. **Testing & tuning**

## Configuration Parameters

**Server:**
- `EnableContentCache` (bool) - Enable server-side cache
- `ContentCacheSize` (MB) - Server cache size
- `ContentCacheMaxAge` (seconds) - Age-based eviction
- `ContentCacheMinRectSize` (pixels) - Minimum size to cache

**Client (NEW):**
- `EnableContentCache` (bool) - Enable client-side cache
- `ClientContentCacheSize` (MB) - Client cache size (default: 256MB)
- `ClientCacheMaxAge` (seconds) - Client cache age limit

## Future Enhancements

1. **Compression:** Compress cached pixel data on client (LZ4)
2. **Persistence:** Save cache to disk between sessions
3. **Pre-caching:** Server predicts likely content and pre-sends
4. **Statistics:** Detailed bandwidth savings reports
5. **Smart eviction:** ML-based prediction of reuse likelihood
