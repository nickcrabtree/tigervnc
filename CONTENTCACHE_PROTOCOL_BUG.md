# ContentCache Protocol Bug Analysis

## STATUS: RESOLVED ✅

**Fixed in commit `d8aad8a0` (2025-10-08)**: Complete client request protocol implementation

---

## Problem Summary

The vncviewer crashes with a segfault immediately after connecting, specifically when receiving ContentCache messages from the server.

## Root Cause

The ContentCache protocol implementation is **incomplete** - it's missing the client-to-server request mechanism that is fundamental to the design.

### Intended Design Flow

1. Server computes hash of content and checks cache
2. **If in cache**: Server sends `CachedRect` (reference only, ~20 bytes)
3. Client receives `CachedRect`, checks local cache
4. **If client missing data**: **Client sends `RequestCachedData(cacheId)` to server** ← **MISSING**
5. Server receives request, sends `CachedRectInit` (full data + cacheId)
6. Client decodes and stores in cache
7. Future references work because client now has the data

### Current Broken Implementation

1. Server sends `CachedRect` (reference)
2. Client receives, checks cache
3. **Client missing data**: Logs "TODO: Implement refresh request mechanism" and returns
4. Screen area remains unrendered (or crashes if trying to access NULL data)

## Evidence

### 1. Client Code Shows TODO

`common/rfb/DecodeManager.cxx` lines 431-437:

```cpp
if (cached == nullptr) {
    // Cache miss - request refresh from server
    cacheStats.cache_misses++;\n    vlog.debug(\"Cache miss for ID %llu, requesting refresh\",
               (unsigned long long)cacheId);\n    // TODO: Implement refresh request mechanism\n    return;\n}
```

### 2. No Request Message in Protocol

The client message writer (`CMsgWriter.h`) has NO method to send cache requests:
- `writeClientInit()`
- `writeSetPixelFormat()`
- `writeSetEncodings()`
- `writeFramebufferUpdateRequest()`
- etc.

**Missing**: `writeRequestCachedData(uint64_t cacheId)`

### 3. Server Has No Handler

The server message reader (`SMsgReader`) has NO handler for cache requests.

### 4. Crash Location

The segfault occurs in `CMsgReader::readCachedRectInit()` at line 918 when calling `readRect(r, encoding)`. This suggests:
- Server is trying to send full data via `CachedRectInit`
- But something is wrong with how the encoding type or data is being sent/received
- The input stream has a NULL pointer when trying to read the encoded rectangle data

## Temporary Workaround

Until the protocol is fully implemented, the server should:

**Always send `CachedRectInit` (full data) instead of `CachedRect` (reference)**

This ensures the client always receives the pixel data and can render correctly, even though it defeats the bandwidth savings.

### Implementation

In `EncodeManager::tryContentCacheLookup()`:

```cpp
bool EncodeManager::tryContentCacheLookup(const core::Rect& rect, const PixelBuffer* pb) {
    // ... compute hash ...
    
    ContentCache::CacheEntry* entry = contentCache->findByHash(hash, &cacheId);
    
    if (entry && cacheId != 0) {
        // TODO: Once client request mechanism is implemented, send CachedRect here
        // conn->writer()->writeCachedRect(rect, cacheId);
        
        // WORKAROUND: Always return false to force full data transmission
        return false;  // Pretend cache miss
    }
    
    return false;
}
```

Alternatively, **disable ContentCache references entirely** while keeping the cache infrastructure:
- Keep `CachedRectInit` messages (populate cache)
- Never send `CachedRect` messages (no references)
- This maintains server-side caching benefits without the broken protocol

## Full Fix Required

To properly implement the protocol:

### 1. Define Client Message Type

Add to `common/rfb/msgTypes.h`:

```cpp
const int msgTypeRequestCachedData = 254;  // Client → Server
```

### 2. Client Message Writer

Add to `CMsgWriter.h/cxx`:

```cpp
void CMsgWriter::writeRequestCachedData(uint64_t cacheId) {
    startMsg(msgTypeRequestCachedData);
    os->writeU32((uint32_t)(cacheId >> 32));  // High 32 bits
    os->writeU32((uint32_t)(cacheId & 0xFFFFFFFF));  // Low 32 bits
    endMsg();
}
```

### 3. Server Message Reader

Add handler in `SMsgReader.cxx`:

```cpp
case msgTypeRequestCachedData:
    ret = readRequestCachedData();
    break;

bool SMsgReader::readRequestCachedData() {
    if (!is->hasData(8))
        return false;
    
    uint32_t hi = is->readU32();
    uint32_t lo = is->readU32();
    uint64_t cacheId = ((uint64_t)hi << 32) | lo;
    
    handler->handleRequestCachedData(cacheId);
    return true;
}
```

### 4. Server Handler

In server connection (VNCSConnectionST or equivalent):

```cpp
void VNCSConnectionST::handleRequestCachedData(uint64_t cacheId) {
    // Look up content by cache ID
    // Re-encode and send as CachedRectInit
    // (Server must remember rect bounds associated with this cacheId)
}
```

### 5. Update Client Handler

In `DecodeManager::handleCachedRect()`:

```cpp
if (cached == nullptr) {
    cacheStats.cache_misses++;
    vlog.debug(\"Cache miss for ID %llu, requesting from server\",
               (unsigned long long)cacheId);
    
    // Send request to server
    conn->writer()->writeRequestCachedData(cacheId);
    
    // Mark region as pending (need to track this)
    pendingCacheRequests[cacheId] = r;
    return;
}
```

## Documentation Updates Needed

The following documentation files describe the protocol incorrectly and need updating:

1. `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` - Shows protocol without client request
2. `CACHE_PROTOCOL_COMPLETE.md` - Claims protocol is complete (it's not)
3. `CACHE_PROTOCOL_DESIGN.md` - Needs client message specification
4. `CONTENTCACHE_CLIENT_INTEGRATION.md` - Missing request implementation

All docs should clearly state:
- `CachedRect` is a **reference** that may fail if client doesn't have data
- Client **must** send `RequestCachedData` message on cache miss
- Server **must** respond with `CachedRectInit` containing full data

## Next Steps

1. **Immediate**: Disable `CachedRect` messages (always send `CachedRectInit`)
2. **Short term**: Implement client request message and server handler
3. **Medium term**: Add persistent disk cache (as mentioned in design docs)
4. **Long term**: Add adaptive strategies (predict what client likely has cached)
