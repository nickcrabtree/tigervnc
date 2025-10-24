# PersistentCache Protocol: Design and Implementation Guide

**Author**: TigerVNC Team  
**Date**: 2025-10-24  
**Status**: Design Document  
**Related**: CONTENTCACHE_DESIGN_IMPLEMENTATION.md

## Executive Summary

This document specifies **PersistentCache**, a new RFB protocol extension that enables persistent, hash-based client-side caching. Unlike the existing **ContentCache** protocol (server-assigned IDs), PersistentCache uses content hashes as stable keys, allowing cache entries to survive client restarts and work across different VNC servers.

## Protocol Relationship and Negotiation

### Two Distinct Protocols

| Protocol | Pseudo-Encoding | Key Type | Persistence | Negotiation |
|----------|----------------|----------|-------------|-------------|
| **ContentCache** (existing) | `-320` | Server-assigned ID | Session-only | Already deployed |
| **PersistentCache** (new) | `-321` | Content hash | Cross-session | This document |

### Negotiation Rules

**Client Behavior:**
- Include `-321` in `SetEncodings` to indicate PersistentCache support
- Include `-320` for backward compatibility with ContentCache-only servers
- Example: `[..., encodingH264, encodingZRLE, -321, -320]`

**Server Behavior:**
- If client sends `-321` and server supports it: **use PersistentCache**
- Else if client sends `-320` and server supports it: **use ContentCache**
- Else: **no caching**

**Priority:** When both are available, PersistentCache is preferred.

### Compatibility Matrix

| Client Support | Server Support | Result |
|---------------|----------------|---------|
| Both | Both | **PersistentCache** ✓ |
| Both | ContentCache only | ContentCache |
| Both | PersistentCache only | PersistentCache |
| PersistentCache only | Both | PersistentCache |
| ContentCache only | Both | ContentCache |
| ContentCache only | PersistentCache only | No caching |
| PersistentCache only | ContentCache only | No caching |

## Protocol Constants

### Pseudo-Encodings
```cpp
// In common/rfb/encodings.h

// Existing ContentCache
const int pseudoEncodingContentCache = -320;

// New PersistentCache
const int pseudoEncodingPersistentCache = -321;
```

### Encoding Types
```cpp
// PersistentCache rectangle encodings
const int encodingPersistentCachedRect = 102;      // Reference by hash
const int encodingPersistentCachedRectInit = 103;  // Full data + hash
```

### Message Types
```cpp
// Client-to-server
const int msgTypePersistentCacheQuery = 254;     // Request missing data
const int msgTypePersistentCacheHashList = 253;  // Advertise known hashes (optional)
```

## Wire Format Specifications

### encodingPersistentCachedRect (Server → Client)

**Purpose:** Reference cached content by hash without resending pixels.

**Format:**
```
┌─────────────────────────────────────┐
│ Standard RFB Rectangle Header       │
├─────────────────────────────────────┤
│ x: uint16                           │
│ y: uint16                           │
│ width: uint16                       │
│ height: uint16                      │
│ encoding: int32 = 102               │
├─────────────────────────────────────┤
│ Payload                             │
├─────────────────────────────────────┤
│ hashLen: uint8                      │
│ hashBytes[hashLen]                  │
│ flags: uint16 (reserved, must be 0) │
└─────────────────────────────────────┘
```

**Semantics:**
- Client looks up `hashBytes` in cache
- On hit: blit cached pixels to framebuffer
- On miss: queue `msgTypePersistentCacheQuery` for this hash

### encodingPersistentCachedRectInit (Server → Client)

**Purpose:** Send full rectangle data plus hash for caching.

**Format:**
```
┌─────────────────────────────────────┐
│ Standard RFB Rectangle Header       │
├─────────────────────────────────────┤
│ x: uint16                           │
│ y: uint16                           │
│ width: uint16                       │
│ height: uint16                      │
│ encoding: int32 = 103               │
├─────────────────────────────────────┤
│ Payload                             │
├─────────────────────────────────────┤
│ hashLen: uint8                      │
│ hashBytes[hashLen]                  │
│ innerEncoding: int32                │
│   (Tight, ZRLE, H.264, etc.)        │
│ payloadLen: uint32                  │
│ payloadBytes[payloadLen]            │
└─────────────────────────────────────┘
```

**Semantics:**
- Client decodes `payloadBytes` using `innerEncoding`
- Stores decoded pixels in cache indexed by `hashBytes`
- Blits to framebuffer

### msgTypePersistentCacheQuery (Client → Server)

**Purpose:** Request initialization data for missing hashes.

**Format:**
```
┌─────────────────────────────────────┐
│ type: uint8 = 254                   │
│ count: uint16                       │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: uint8                    │
│   hashBytes[hashLen]                │
└─────────────────────────────────────┘
```

**Semantics:**
- Server responds with `encodingPersistentCachedRectInit` for requested hashes
- Server may coalesce, rate-limit, or batch responses

### msgTypePersistentCacheHashList (Client → Server, Optional)

**Purpose:** Proactively advertise known hashes to reduce misses.

**Format (Simple List):**
```
┌─────────────────────────────────────┐
│ type: uint8 = 253                   │
│ sequenceId: uint32                  │
│ totalChunks: uint16                 │
│ chunkIndex: uint16                  │
│ count: uint16                       │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: uint8                    │
│   hashBytes[hashLen]                │
└─────────────────────────────────────┘
```

**Semantics:**
- Server records client's hash presence
- Increases confidence for sending `encodingPersistentCachedRect`
- Chunking allows large hash lists without blocking

## Protocol Flows

### Initial Connection with PersistentCache

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Client loads persistent cache from disk                  │
│    ~/.cache/tigervnc/persistentcache.dat                   │
│    (contains 50,000 hashes from previous sessions)          │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 2. Client connects to Server A                             │
│    Client → Server: SetEncodings                            │
│      [..., Tight, ZRLE, -321, -320]                        │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 3. Server supports PersistentCache                          │
│    Server enables PersistentCache (ignores -320)            │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ 4. (Optional) Client → Server: HashList                     │
│    Sends chunks of 1,000 hashes each                        │
│    Server builds lookup table of client's known hashes      │
└─────────────────────────────────────────────────────────────┘
```

### Cache Hit Flow

```
Server has updated region: 800×600 at (100, 200)
    ↓
Server computes hash of pixel data: 0xABCD1234...
    ↓
Server checks: does client have this hash?
    (either from HashList or previous Init)
    ↓
YES → Server → Client: encodingPersistentCachedRect
              x=100, y=200, w=800, h=600
              hash=0xABCD1234...
    ↓
Client looks up 0xABCD1234 in cache → HIT!
    ↓
Client blits cached pixels to framebuffer
    ↓
✅ Zero decode cost, 20 bytes transferred vs ~50KB
```

### Cache Miss Flow

```
Server → Client: encodingPersistentCachedRect
                hash=0xDEADBEEF...
    ↓
Client looks up 0xDEADBEEF → MISS!
    ↓
Client queues request (batches multiple misses)
    ↓
Client → Server: msgTypePersistentCacheQuery
                 count=1
                 hash=0xDEADBEEF...
    ↓
Server → Client: encodingPersistentCachedRectInit
                 hash=0xDEADBEEF...
                 innerEncoding=Tight
                 payload=[compressed data]
    ↓
Client decodes payload
    ↓
Client stores in cache: 0xDEADBEEF → pixels
    ↓
Client blits to framebuffer
    ↓
Cache now contains this hash for future hits
```

### Cross-Server Cache Hit

```
Client disconnects from Server A
    ↓
Client saves cache to disk (preserves all hashes)
    ↓
[Later: Client restart or new session]
    ↓
Client loads cache from disk
    ↓
Client connects to Server B (different machine!)
    ↓
Server B sends: encodingPersistentCachedRect
               hash=0xABCD1234... (same Gmail window)
    ↓
Client: "I have this hash from Server A!"
    ↓
✅ Cross-server cache hit!
```

## Data Structures

### GlobalClientPersistentCache (Client-Side)

```cpp
// In common/rfb/GlobalClientPersistentCache.h

class GlobalClientPersistentCache {
public:
    struct CachedPixels {
        std::vector<uint8_t> pixels;  // Decoded pixel data
        PixelFormat format;           // Pixel format
        uint16_t width;               // Rectangle width
        uint16_t height;              // Rectangle height
        uint16_t stridePixels;        // Stride in pixels
        uint32_t lastAccessTime;      // For LRU/ARC eviction
        
        size_t byteSize() const {
            return pixels.size();
        }
    };
    
    GlobalClientPersistentCache(size_t maxSizeMB = 2048);
    ~GlobalClientPersistentCache();
    
    // Lifecycle
    bool loadFromDisk();
    bool saveToDisk();
    
    // Protocol operations
    bool has(const std::vector<uint8_t>& hash) const;
    const CachedPixels* get(const std::vector<uint8_t>& hash);
    void insert(const std::vector<uint8_t>& hash, 
               const uint8_t* pixels,
               const PixelFormat& pf,
               uint16_t width, uint16_t height,
               uint16_t stridePixels);
    
    // Optional: Get all known hashes for HashList
    std::vector<std::vector<uint8_t>> getAllHashes() const;
    
    // Statistics
    struct Stats {
        size_t totalEntries;
        size_t totalBytes;
        uint64_t cacheHits;
        uint64_t cacheMisses;
    };
    Stats getStats() const;
    
private:
    // Using vector<uint8_t> for hash to support variable lengths
    std::unordered_map<std::vector<uint8_t>, 
                      CachedPixels,
                      HashVectorHasher> cache_;
    
    // ARC algorithm for eviction
    // ... (see ARC_ALGORITHM.md for details)
    
    size_t maxCacheSize_;
    size_t currentSize_;
    std::string cacheFilePath_;
};
```

### Server-Side State Tracking

```cpp
// In common/rfb/EncodeManager.h

class EncodeManager {
private:
    // Track which hashes the client has advertised
    std::unordered_set<std::vector<uint8_t>, HashVectorHasher> clientKnownHashes_;
    
    // Pending queries from client
    std::queue<std::vector<uint8_t>> pendingQueries_;
    
public:
    void handleHashList(const std::vector<std::vector<uint8_t>>& hashes);
    void handleCacheQuery(const std::vector<std::vector<uint8_t>>& hashes);
    
    void writeRectWithPersistentCache(const core::Rect& r, 
                                     const PixelBuffer* pb);
};
```

## Hashing Algorithm

### Requirements

1. **Stable across platforms:** Same bytes → same hash
2. **Stable across builds:** No reliance on pointer addresses or non-deterministic data
3. **Fast:** Must not bottleneck encoding pipeline
4. **Low collision rate:** Minimize false positives

### Recommended: SHA-256 Truncated to 128 bits

```cpp
// In common/rfb/ContentHash.h

class ContentHash {
public:
    static std::vector<uint8_t> compute(const uint8_t* data, 
                                       size_t len) {
        // Use SHA-256, truncate to 16 bytes
        std::vector<uint8_t> hash(16);
        
        SHA256_CTX ctx;
        SHA256_Init(&ctx);
        SHA256_Update(&ctx, data, len);
        
        uint8_t full_hash[32];
        SHA256_Final(full_hash, &ctx);
        
        // Take first 16 bytes
        memcpy(hash.data(), full_hash, 16);
        return hash;
    }
};
```

### Hash Input Domain

**CRITICAL:** Hash must be computed over decoded pixel bytes in row-major order.

```cpp
// Compute hash for a rectangle
std::vector<uint8_t> computeRectHash(const PixelBuffer* pb, 
                                     const core::Rect& r) {
    int stride;
    const uint8_t* pixels = pb->getBuffer(r, &stride);
    
    int bytesPerPixel = pb->getPF().bpp / 8;
    size_t rowBytes = r.width() * bytesPerPixel;
    size_t strideBytes = stride * bytesPerPixel;  // CRITICAL: multiply!
    
    // Hash row-major pixel data
    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    
    for (int y = 0; y < r.height(); y++) {
        const uint8_t* row = pixels + (y * strideBytes);
        SHA256_Update(&ctx, row, rowBytes);
    }
    
    uint8_t full_hash[32];
    SHA256_Final(full_hash, &ctx);
    
    std::vector<uint8_t> hash(16);
    memcpy(hash.data(), full_hash, 16);
    return hash;
}
```

**Note:** This matches the fix from Oct 7 2025 that corrected the stride-in-pixels bug.

## Implementation Phases

### Phase 1: Protocol Foundation (2-3 days)

**Goal:** Add constants and feature flags without breaking existing code.

**Tasks:**
1. Add `pseudoEncodingPersistentCache = -321` to `common/rfb/encodings.h`
2. Add `encodingPersistentCachedRect = 102` and `encodingPersistentCachedRectInit = 103`
3. Add message type constants
4. Add server parameter: `BoolParameter persistentCache("PersistentCache", ..., false)`
5. Add client parameter: `BoolParameter persistentCache("PersistentCache", ..., false)`

**Testing:** Compile successfully, no behavior changes.

### Phase 2: Client Cache Storage (3-4 days)

**Goal:** Implement `GlobalClientPersistentCache` with in-memory storage only.

**Tasks:**
1. Create `common/rfb/GlobalClientPersistentCache.h` and `.cxx`
2. Implement hash-indexed storage
3. Implement ARC eviction algorithm
4. Add unit tests

**Testing:** `ctest --test-dir build -R PersistentCache`

### Phase 3: Client Protocol Messages (2-3 days)

**Goal:** Implement client-side message reading/writing.

**Tasks:**
1. Extend `CMsgReader::readRect()` for new encoding types
2. Add `CMsgWriter::writeCacheQuery()` and `writeHashList()`
3. Update `CMsgHandler` interface with new virtual methods

**Testing:** Wire format validation, mock server tests.

### Phase 4: Client Integration (3-4 days)

**Goal:** Wire cache into DecodeManager, handle protocol messages.

**Tasks:**
1. Add `GlobalClientPersistentCache*` to `DecodeManager`
2. Implement handlers for new encodings
3. Implement query batching and debouncing

**Testing:** Integration tests with mock server.

### Phase 5: Server Protocol Messages (2-3 days)

**Goal:** Implement server-side message reading/writing.

**Tasks:**
1. Extend `SMsgReader` for client messages
2. Add `SMsgWriter` methods for new encodings
3. Update `SMsgHandler` interface

**Testing:** Mock client tests.

### Phase 6: Server Integration (4-5 days)

**Goal:** Integrate hash-based encoding into EncodeManager.

**Tasks:**
1. Add hash computation to EncodeManager
2. Track client's known hashes
3. Implement sending logic (reference vs init)
4. Add backward compatibility checks

**Testing:** End-to-end integration tests.

### Phase 7: Disk Persistence (5-7 days)

**Goal:** Implement cache file I/O.

**Tasks:**
1. Design cache file format
2. Implement save/load
3. Add integrity checks
4. Handle corruption gracefully

**Testing:** Restart tests, corruption recovery tests.

## Configuration

### Client Parameters

```cpp
// In vncviewer/parameters.cxx

BoolParameter persistentCache("PersistentCache",
    "Enable PersistentCache protocol",
    true);

IntParameter persistentCacheSize("PersistentCacheSize",
    "Cache size in MB",
    2048);

StringParameter persistentCachePath("PersistentCachePath",
    "Cache file path (default: ~/.cache/tigervnc/persistentcache.dat)",
    "");

BoolParameter persistentCachePreferOverContentCache(
    "PersistentCachePreferOverContentCache",
    "Prefer PersistentCache when both are available",
    true);
```

### Server Parameters

```cpp
// In common/rfb/ServerCore.h

BoolParameter persistentCache("PersistentCache",
    "Enable PersistentCache protocol",
    true);

IntParameter persistentCacheMinRectSize("PersistentCacheMinRectSize",
    "Minimum rectangle size (pixels) to consider for caching",
    4096);
```

## File Format (Cache Persistence)

### Cache File: `~/.cache/tigervnc/persistentcache.dat`

```
┌────────────────────────────────────────┐
│ Header (64 bytes)                      │
├────────────────────────────────────────┤
│ magic:       uint32 = 0x50435643       │  "PCVC" (PersistentCache VNC)
│ version:     uint32 = 1                │
│ totalEntries: uint64                   │
│ totalBytes:  uint64                    │
│ created:     uint64 (unix timestamp)   │
│ lastAccess:  uint64 (unix timestamp)   │
│ reserved:    uint8[24]                 │
└────────────────────────────────────────┘
                  ↓
┌────────────────────────────────────────┐
│ Entry Records (variable)               │
├────────────────────────────────────────┤
│ For each entry:                        │
│                                        │
│ ┌────────────────────────────────────┐ │
│ │ hashLen:      uint8                │ │
│ │ hash:         uint8[hashLen]       │ │
│ │ width:        uint16               │ │
│ │ height:       uint16               │ │
│ │ stridePixels: uint16               │ │
│ │ pixelFormat:  (24 bytes)           │ │
│ │ lastAccess:   uint32               │ │
│ │ pixelDataLen: uint32               │ │
│ │ pixelData:    uint8[pixelDataLen]  │ │
│ └────────────────────────────────────┘ │
└────────────────────────────────────────┘
                  ↓
┌────────────────────────────────────────┐
│ Checksum (32 bytes)                    │
├────────────────────────────────────────┤
│ SHA-256 of all above data              │
└────────────────────────────────────────┘
```

## Testing Strategy

### Unit Tests

```cpp
TEST(PersistentCache, BasicStoreAndRetrieve) {
    GlobalClientPersistentCache cache(10);  // 10 MB
    
    std::vector<uint8_t> hash = {0xAB, 0xCD, ...};
    uint8_t pixels[256] = { /* test data */ };
    PixelFormat pf(...);
    
    cache.insert(hash, pixels, pf, 16, 16, 16);
    
    ASSERT_TRUE(cache.has(hash));
    const auto* entry = cache.get(hash);
    ASSERT_NE(entry, nullptr);
    EXPECT_EQ(entry->width, 16);
}

TEST(PersistentCache, Eviction) {
    GlobalClientPersistentCache cache(1);  // 1 MB - triggers eviction
    
    // Fill beyond capacity
    for (int i = 0; i < 100; i++) {
        std::vector<uint8_t> hash = makeTestHash(i);
        uint8_t pixels[16384];  // 16 KB each
        cache.insert(hash, pixels, pf, 64, 64, 64);
    }
    
    auto stats = cache.getStats();
    EXPECT_LT(stats.totalBytes, 1024 * 1024);  // Under 1 MB
}
```

### Integration Tests

```bash
# Test negotiation
1. Start server with PersistentCache enabled
2. Connect client with both -321 and -320
3. Verify PersistentCache is selected
4. Send test rectangles, verify cache hits

# Test cross-server
1. Connect to Server A, fill cache
2. Disconnect
3. Connect to Server B with identical desktop
4. Verify cache hits from Server A's data

# Test persistence
1. Connect, fill cache
2. Disconnect and exit client
3. Restart client
4. Reconnect
5. Verify immediate cache hits
```

## Backward Compatibility

### Old Client + New Server

- Client sends only `-320`
- Server enables ContentCache
- PersistentCache code path never executed
- **Result:** Works perfectly with ContentCache

### New Client + Old Server

- Client sends `-321` and `-320`
- Server doesn't recognize `-321`, uses `-320`
- Client falls back to ContentCache
- **Result:** Works perfectly with ContentCache

### Both Support Both

- Client sends `-321` and `-320`
- Server prefers `-321`
- **Result:** PersistentCache enabled

## Known Issues and Pitfalls

### 1. Stride Must Be in Bytes When Hashing

**Problem:** If API returns stride in pixels, must multiply by `bytesPerPixel` before hashing.

**Solution:**
```cpp
int stride;  // In pixels!
const uint8_t* pixels = pb->getBuffer(r, &stride);
int bytesPerPixel = pb->getPF().bpp / 8;
size_t strideBytes = stride * bytesPerPixel;  // Convert!
```

**Reference:** Oct 7 2025 fix in ContentCache.

### 2. Hash Collisions

**Probability:** With 128-bit hashes and 100,000 entries: ~0.0001% chance.

**Mitigation:** Use full SHA-256 (256 bits) if security is critical.

### 3. Pixel Format Changes

**Issue:** Same visual content with different pixel formats produces different hashes.

**Solution:** Client may need multiple cache entries for same visual content. This is by design.

## Performance Characteristics

### Network Savings

- **Cache hit:** 20 bytes (hash reference) vs 50 KB (typical compressed rectangle)
- **Savings:** 99.96% bandwidth reduction
- **Latency:** Zero decode time on hit (memory blit only)

### Memory Usage

- **2 GB cache:** ~100,000 entries at 16 KB average
- **Overhead:** ~50 bytes per entry for metadata
- **Total:** ~2.05 GB

### Startup/Shutdown Time

- **Load 100,000 entries:** ~2 seconds (SSD)
- **Save 100,000 entries:** ~3 seconds (SSD)
- **Optimization:** Consider lazy loading in future

## Security Considerations

### Hash Collisions

**Risk:** Malicious server could craft content to collide with legitimate hashes.

**Mitigation:** SHA-256 truncated to 128 bits provides adequate collision resistance for content identification (not cryptographic security).

### Cache Poisoning

**Risk:** Malicious server sends incorrect data with legitimate hash.

**Mitigation:**
- Client validates rectangle dimensions match
- TLS/SSH tunnel for server authentication
- Cache isolated per-user (file permissions)

### Disk Space

**Risk:** Cache grows without bound.

**Mitigation:**
- Hard size limit (default 2 GB)
- ARC eviction
- Configurable max age

## References

- **ContentCache Design:** CONTENTCACHE_DESIGN_IMPLEMENTATION.md
- **ARC Algorithm:** ARC_ALGORITHM.md  
- **RFB Protocol:** RFC 6143
- **SHA-256:** FIPS 180-4

## Changelog

- **2025-10-24:** Initial PersistentCache protocol design, distinct from ContentCache
