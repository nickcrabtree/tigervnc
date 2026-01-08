# Lazy 32-bit Refresh Design

## Problem Statement

When network bandwidth is limited, the TigerVNC viewer's AutoSelect feature switches to 8bpp (rgb332) pixel format to maintain responsiveness. This causes visual quality degradation (purple/color-shifted areas). The current lossless refresh mechanism attempts to improve quality during idle time, but it sends data at the **current negotiated pixel format** (8bpp), which doesn't actually improve color quality.

### Current Behavior

1. Client measures throughput, drops below 256 kbit/s threshold
2. Client sends `SetPixelFormat(8bpp)`
3. Server sends all updates at 8bpp
4. During idle time, server sends "lossless refresh" - but still at 8bpp
5. Client caches the 8bpp data
6. When same content is needed again, cache returns 8bpp data
7. Visual quality remains degraded

### Desired Behavior

1. Client at 8bpp for realtime responsiveness
2. During idle time, server sends **32bpp** data for cache population
3. Client stores 32bpp data in PersistentCache
4. When same content is needed again, client uses cached 32bpp data
5. Visual quality is excellent despite low bandwidth

## Goals

1. **Progressive Quality Enhancement**: Use idle bandwidth to upgrade cached content to full color depth
2. **Cache-First Display**: Prefer high-quality cached content over low-quality fresh content
3. **Bandwidth Efficiency**: Don't waste bandwidth re-sending 8bpp content during lossless refresh
4. **Protocol Compatibility**: Maintain backward compatibility with standard VNC clients

## Architecture Overview

### Current Data Flow

```
Server Framebuffer (32bpp)
    │
    ▼ SetPixelFormat(8bpp)
┌─────────────────────┐
│  Pixel Conversion   │
│  32bpp → 8bpp       │
└─────────────────────┘
    │
    ▼
┌─────────────────────┐
│  Encoder (ZRLE)     │
│  Compresses 8bpp    │
└─────────────────────┘
    │
    ▼ Wire Protocol (8bpp encoded)
┌─────────────────────┐
│  Decoder            │
│  Decompresses       │
└─────────────────────┘
    │
    ▼
┌─────────────────────┐
│  Display Conversion │
│  8bpp → display fmt │
└─────────────────────┘
    │
    ▼
Client Framebuffer (display native, e.g. 32bpp)
```

**Problem**: The 8bpp→display conversion cannot restore the lost color information. The client's framebuffer and cache contain color-degraded data.

### Proposed Data Flow (Lazy 32-bit Refresh)

```
Server Framebuffer (32bpp)
    │
    ├─── Realtime Path (8bpp) ───────────────────┐
    │                                             │
    ▼ Lossless Refresh Path                       │
┌─────────────────────┐                           │
│  NO Pixel Conv.     │                           │
│  Keep 32bpp         │                           │
└─────────────────────┘                           │
    │                                             │
    ▼                                             ▼
┌─────────────────────┐               ┌─────────────────────┐
│  Encoder (ZRLE)     │               │  Encoder (ZRLE)     │
│  Compresses 32bpp   │               │  Compresses 8bpp    │
└─────────────────────┘               └─────────────────────┘
    │                                             │
    ▼ PersistentCachedRectInit                    ▼ Normal rect
    │ (with native_format flag)                   │
    │                                             │
┌─────────────────────┐               ┌─────────────────────┐
│  Client Decoder     │               │  Client Decoder     │
│  Stores 32bpp cache │               │  Display at 8bpp    │
└─────────────────────┘               └─────────────────────┘
    │
    ▼ Future cache HIT
┌─────────────────────┐
│  Display from cache │
│  Full 32bpp quality │
└─────────────────────┘
```

## Protocol Design

### Option A: Extend PersistentCachedRectInit (Recommended)

Add a pixel format descriptor to `PersistentCachedRectInit` messages:

```
Current PersistentCachedRectInit:
  - rect: x, y, width, height (8 bytes)
  - encoding: pseudoEncodingPersistentCachedRectInit (-325)
  - cacheId: uint64 (8 bytes)
  - payloadEncoding: int32 (4 bytes)
  - payload: encoded pixel data

Proposed PersistentCachedRectInit v2:
  - rect: x, y, width, height (8 bytes)
  - encoding: pseudoEncodingPersistentCachedRectInit (-325)
  - cacheId: uint64 (8 bytes)
  - flags: uint8 (1 byte)
      - bit 0: native_format (0 = use connection pixel format, 1 = native format follows)
      - bits 1-7: reserved
  - [if native_format] pixelFormat: PixelFormat (16 bytes)
  - payloadEncoding: int32 (4 bytes)
  - payload: encoded pixel data
```

**Pros**:
- Single message type
- Backward compatible (servers not sending native_format work as before)
- Client can reject/ignore native_format if unsupported

**Cons**:
- Protocol version coordination needed
- 17 extra bytes per native-format message

### Option B: New Message Type

Add a new pseudo-encoding `pseudoEncodingNativeFormatCachedRectInit` (-326):

```
NativeFormatCachedRectInit:
  - rect: x, y, width, height (8 bytes)
  - encoding: pseudoEncodingNativeFormatCachedRectInit (-326)
  - cacheId: uint64 (8 bytes)
  - pixelFormat: PixelFormat (16 bytes)
  - payloadEncoding: int32 (4 bytes)
  - payload: encoded pixel data
```

**Pros**:
- Clean separation, easy feature detection
- Clients that don't advertise -326 won't receive these messages

**Cons**:
- More code paths
- Two cache init message types to handle

### Recommendation

**Option A** is recommended for simplicity. The `flags` byte provides future extensibility.

## Client-Side Changes

### 1. Pixel Format Handling in Decoder

When receiving `PersistentCachedRectInit` with `native_format=1`:

```cpp
void DecodeManager::handlePersistentCachedRectInit(...) {
  PixelFormat dataFormat;
  
  if (flags & NATIVE_FORMAT_FLAG) {
    // Read pixel format from message
    dataFormat = readPixelFormat(reader);
  } else {
    // Use connection pixel format
    dataFormat = conn->client.pf();
  }
  
  // Decode payload using dataFormat
  decoder->decode(payload, dataFormat);
  
  // Convert to framebuffer format if needed
  if (dataFormat != framebuffer->getPF()) {
    convertPixels(decoded, dataFormat, framebuffer->getPF());
  }
  
  // Store in cache at NATIVE quality (32bpp)
  // Cache key includes format info
  persistentCache->store(cacheId, decoded, dataFormat);
}
```

### 2. Cache Storage Strategy

The client cache should store pixels at the **highest quality received**:

```cpp
void PersistentCache::store(uint64_t id, pixels, PixelFormat pf) {
  auto existing = lookup(id);
  
  if (existing && existing->format.bpp >= pf.bpp) {
    // Already have equal or better quality, skip
    return;
  }
  
  // Store new entry (upgrades quality if bpp is higher)
  entries[id] = { pixels, pf, timestamp };
}
```

### 3. Cache Retrieval with Format Conversion

When retrieving cached content for display:

```cpp
bool PersistentCache::retrieve(uint64_t id, PixelBuffer* dest) {
  auto entry = lookup(id);
  if (!entry) return false;
  
  if (entry->format == dest->getPF()) {
    // Direct copy
    memcpy(dest, entry->pixels);
  } else {
    // Convert from cached format to display format
    convertPixels(entry->pixels, entry->format, dest, dest->getPF());
  }
  
  return true;
}
```

## Server-Side Changes

### 1. Lossless Refresh at Native Format

Modify `EncodeManager::writeLosslessRefresh` to send at native depth:

```cpp
void EncodeManager::doLosslessRefreshUpdate(const Region& region, 
                                            const PixelBuffer* pb) {
  // Use native pixel format (server framebuffer format) instead of
  // client's negotiated format
  PixelFormat nativeFormat = pb->getPF();
  
  // Temporarily override for this update
  bool useNativeFormat = (nativeFormat.bpp > conn->client.pf().bpp);
  
  for (each rect in region) {
    if (usePersistentCache && clientSupportsNativeFormatCache) {
      writePersistentCachedRectInit(rect, cacheId, 
                                    useNativeFormat ? NATIVE_FORMAT_FLAG : 0,
                                    useNativeFormat ? nativeFormat : conn->client.pf(),
                                    payload);
    } else {
      // Fall back to standard encoding at client format
      writeRect(rect, pb);
    }
  }
}
```

### 2. Track "Client Needs Upgrade" Regions

Extend `reducedDepthRegion` to prioritize during lossless refresh:

```cpp
void EncodeManager::writeLosslessRefresh(...) {
  // Prioritize reduced-depth regions for native-format upgrade
  Region upgradeRegion = reducedDepthRegion.intersect(pendingRefreshRegion);
  
  if (!upgradeRegion.is_empty()) {
    // Send these at native 32bpp format for cache population
    doLosslessRefreshUpdate(upgradeRegion, pb, /* nativeFormat */ true);
    reducedDepthRegion.assign_subtract(upgradeRegion);
  }
  
  // Then handle remaining lossy regions (JPEG artifacts) at client format
  Region lossyOnlyRegion = pendingRefreshRegion.subtract(upgradeRegion);
  if (!lossyOnlyRegion.is_empty()) {
    doLosslessRefreshUpdate(lossyOnlyRegion, pb, /* nativeFormat */ false);
  }
}
```

### 3. Capability Negotiation

Add pseudo-encoding for feature detection:

```cpp
// In encodings.h
const int pseudoEncodingNativeFormatCache = -327;

// Server checks during setEncodings:
bool clientSupportsNativeFormatCache = 
  client.supportsEncoding(pseudoEncodingNativeFormatCache);
```

## Implementation Phases

### Phase 1: Foundation (Server-Side Tracking)
**Status**: Complete

- [x] Track `reducedDepthRegion` for content sent at < 24 bpp
- [x] Trigger refresh when pixel format upgrades via `handlePixelFormatChange()`
- [x] `lastSentBpp` tracks pixel depth of updates

### Phase 2: Server-Side Native Format Refresh
**Estimated effort**: 1-2 days

- [ ] Modify `writeLosslessRefresh()` to send at native 32bpp format (not client format)
- [ ] Prioritize `reducedDepthRegion` over `lossyRegion` in refresh order
- [ ] Use `preparePixelBuffer()` with native format for refresh encoding
- [ ] Clear `reducedDepthRegion` after successful 32bpp send

**Deliverable**: Server sends 32bpp data during idle time regardless of client's current pixel format.

### Phase 3: Protocol Extension
**Estimated effort**: 1 day

- [ ] Add `flags` byte to `PersistentCachedRectInit` header
- [ ] Define `NATIVE_FORMAT_FLAG` (bit 0)
- [ ] When flag set, include 16-byte PixelFormat after flags
- [ ] Update `SMsgWriter::writePersistentCachedRectInit()`
- [ ] Update `CMsgReader::readPersistentCachedRectInit()`

**Deliverable**: Wire protocol carries pixel format with cache INIT messages.

### Phase 4: Client-Side Format Handling
**Estimated effort**: 1-2 days

- [ ] Parse `flags` and optional PixelFormat in cache INIT
- [ ] Decode payload using specified format (not connection format)
- [ ] Store decoded pixels at native quality in cache
- [ ] On cache HIT retrieval, convert from stored format to display format
- [ ] Delete existing cache (no migration needed)

**Deliverable**: Client displays 32bpp cached content regardless of current wire format.

### Phase 5: Testing
**Estimated effort**: 1-2 days

- [ ] Manual testing with bandwidth-limited connection
- [ ] Verify 8bpp regions upgrade to 32bpp during idle
- [ ] Verify cache HITs return 32bpp quality
- [ ] Verify AutoSelect sees upgrade traffic in throughput

**Deliverable**: Feature works end-to-end.

## Design Decisions

The following decisions have been made for this implementation:

### 1. Upgrade Priority: 8bpp First

**Decision**: 8bpp (reduced depth) regions are upgraded BEFORE JPEG/lossy regions.

**Rationale**: Purple/color-shifted areas from 8bpp are much more visually objectionable than JPEG compression artifacts. Users notice and complain about purple immediately; JPEG artifacts are often acceptable.

**Implementation**: In lossless refresh, process `reducedDepthRegion` before `lossyRegion`.

### 2. AutoSelect Includes All Traffic

**Decision**: Lazy upgrade bandwidth IS included in throughput measurement.

**Rationale**: AutoSelect measures overall network capacity, not just "realtime" traffic. All bytes sent contribute to understanding the link's capabilities.

**Implementation**: No special handling needed - upgrades use the same write path and naturally contribute to throughput measurement.

### 3. No Backward Compatibility Required

**Decision**: This is a testing repository with exactly one PersistentCache on disk. Breaking changes are acceptable.

**Implications**:
- Can delete and recreate cache freely
- No format migration code needed
- No fallback paths for old clients
- Simpler protocol changes (no version negotiation)
- Can change on-disk format without migration

### 4. Cache Storage: Always Highest Quality

**Decision**: Store at highest quality received, convert on retrieval.

**Implementation**: When 32bpp data arrives for an ID that has 8bpp cached, replace with 32bpp.

### 5. Hash Computation

**Decision**: Keep current behavior - hash computed on server's 32bpp framebuffer.

**Rationale**: Hash identifies content semantically, not by format. Same visual content at different depths shares the same cache ID.

## Metrics and Monitoring

### Server Metrics
- `reduced_depth_region_pixels`: Current size of reduced-depth regions
- `native_format_upgrades_sent`: Count of 32bpp upgrade messages
- `native_format_upgrade_bytes`: Bytes sent for upgrades
- `upgrade_bandwidth_utilization`: % of lossless refresh budget used for upgrades

### Client Metrics
- `cache_entries_by_format`: Count of entries at each bpp
- `cache_upgrades_received`: Count of entries upgraded from 8bpp to 32bpp
- `cache_hits_by_quality`: Cache hits returning 8bpp vs 32bpp data
- `format_conversions`: Count of on-retrieval format conversions

## References

- `common/rfb/EncodeManager.cxx`: Server-side encoding and lossless refresh
- `common/rfb/VNCSConnectionST.cxx`: Connection handling, `writeLosslessRefresh()`
- `vncviewer/DecodeManager.cxx`: Client-side decoding
- `vncviewer/PersistentCache.cxx`: Client cache implementation
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: Existing cache protocol documentation
