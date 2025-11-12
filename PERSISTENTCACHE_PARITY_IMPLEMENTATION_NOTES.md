# PersistentCache Parity Implementation Notes

**Started**: January 8, 2026  
**Branch**: `feature/persistentcache-parity-cpp`  
**Plan**: `PERSISTENTCACHE_PARITY_PLAN.md`  
**Goal**: Bring PersistentCache to feature parity with ContentCache improvements (October-November 2025)

---

## Implementation Progress

### Phase 1: Scope and Audit ✅ COMPLETE

**Completed**: January 8, 2026

- ✅ Reviewed ContentCache improvements (ARC eviction, bandwidth tracking, synchronization)
- ✅ Analyzed existing PersistentCache implementation
- ✅ Created comprehensive 16-item TODO list
- ✅ Confirmed defaults and safety constraints

**Key Decisions**:
- Message type: `msgTypePersistentCacheEviction = 249` (251 is SetDesktopSize, 250 is ContentCache eviction)
- Wire format: u8 type, u8 pad, u16 count, repeated (u8 hashLen, u8[hashLen] hash)
- Skip initial HashList advertisement; rely on on-demand queries (already batched)
- ArcCache template for PersistentCache only initially; ContentCache migration later
- No cross-cycle queues; use clientRequestedPersistentHashes_ for synchronization

### Phase 2: Protocol Extensions ✅ COMPLETE

**Started**: January 8, 2026  
**Completed**: January 8, 2026

#### Completed
- ✅ Added `msgTypePersistentCacheEviction = 249` constant to `common/rfb/msgTypes.h`
- ✅ Implemented `SMsgReader::readPersistentCacheEviction()` with validation
- ✅ Added `SMsgHandler::handlePersistentCacheEviction()` virtual method
- ✅ Implemented `CMsgWriter::writePersistentCacheEviction()` with validation
- ✅ Implemented `CMsgWriter::writePersistentCacheEvictionBatched()` with batching
- ✅ Wired up message dispatch in `SMsgReader::readMsg()`
- ✅ Added LogWriter support to CMsgWriter

#### Wire Format Implemented
```
Message type: 249 (msgTypePersistentCacheEviction)
Direction: Client → Server
Format:
  U8:  type = 249
  U8:  padding = 0
  U16: count (number of hashes)
  For each hash:
    U8:          hashLen
    U8[hashLen]: hashBytes
```

#### Validation
- Max count: 1000 hashes per message
- Max hashLen: 64 bytes, min hashLen: 1 byte
- Batching: splits into chunks of 100 for safety
- Protocol errors thrown for invalid counts or lengths

#### Build Verification
- ✅ macOS viewer builds cleanly with protocol changes
- ✅ Stub handler added to VNCSConnectionST for compilation
- ✅ No syntax errors or missing symbols

#### Phase 5 Dependencies
- ⏳ Full handler implementation in VNCSConnectionST (clientKnownPersistentHashes_)
- ⏳ Server encoder logic (reference vs init vs fallback)

### Phase 3: Shared C++ Modules ⏳ PENDING

**Dependencies**: Phase 2 complete

#### Planned Tasks
- Create `common/rfb/cache/ArcCache.{h,cxx}` template utility
- Create `common/rfb/cache/BandwidthStats.{h,cxx}` helpers
- Create `common/rfb/cache/ProtocolHelpers.{h,cxx}` utilities

### Phase 4: C++ Viewer Enhancements ⏳ PENDING

**Dependencies**: Phases 2, 3 complete

#### Planned Tasks
- Adapt GlobalClientPersistentCache to use ArcCache
- Add eviction callback and pendingEvictions_ queue
- Integrate bandwidth tracking in DecodeManager
- Send eviction notifications during flush()

### Phase 5: C++ Server Enhancements ⏳ PENDING

**Dependencies**: Phases 2, 4 complete

#### Planned Tasks
- Track clientKnownPersistentHashes_ and clientRequestedPersistentHashes_
- Implement encoder reference vs init vs fallback logic
- Add synchronization discipline (register after sending init)

### Phase 6: Testing and Validation ⏳ PENDING

**Dependencies**: Phases 3, 4, 5 complete

#### Planned Tasks
- Unit tests: ArcCache, protocol messages, bandwidth stats
- E2E tests: eviction flow, bandwidth tracking, cross-platform
- Parity validation: ContentCache vs PersistentCache hit rates

### Phase 7: Documentation and Rollout ⏳ PENDING

**Dependencies**: Phases 4, 5, 6 complete

#### Planned Tasks
- Update PERSISTENTCACHE_DESIGN.md with eviction protocol
- Update parameter documentation (--help)
- CI integration and staged rollout

---

## Technical Details

### Protocol Wire Format

**msgTypePersistentCacheEviction (249)**:
```
┌─────────────────────────────────────┐
│ type: U8 = 249                      │
│ padding: U8 = 0                     │
│ count: U16 (big-endian)             │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: U8                       │
│   hashBytes[hashLen]                │
└─────────────────────────────────────┘
```

**Constraints**:
- Max count: 1000 (per message)
- Max hashLen: 64 bytes
- Batching: split into chunks of 100 for safety

### Key Files Modified

#### Phase 2 (Protocol)
- ✅ `common/rfb/msgTypes.h` - Added constant (line 61) with documentation
- ✅ `common/rfb/SMsgReader.h` - Added readPersistentCacheEviction() declaration (line 64)
- ✅ `common/rfb/SMsgReader.cxx` - Implemented reader (lines 123-125, 654-699) with validation
- ✅ `common/rfb/SMsgHandler.h` - Added handlePersistentCacheEviction() virtual (line 78)
- ✅ `common/rfb/CMsgWriter.h` - Added write methods (lines 75-76)
- ✅ `common/rfb/CMsgWriter.cxx` - Implemented writer (lines 29, 46, 300-346) with batching
- ✅ `common/rfb/VNCSConnectionST.h` - Added override declaration (line 157)
- ✅ `common/rfb/VNCSConnectionST.cxx` - Added stub handler (lines 930-940)

#### Phase 3 (Shared Utilities)
- ✅ `common/rfb/cache/ArcCache.h` (new, header-only)
- ⏳ `common/rfb/cache/BandwidthStats.{h,cxx}` (new)
- ⏳ `common/rfb/cache/ProtocolHelpers.{h,cxx}` (new)
- ⏳ `common/rfb/cache/README.md` (new)

#### Phase 4 (Viewer)
- ⏳ `common/rfb/GlobalClientPersistentCache.{h,cxx}` - ArcCache integration
- ⏳ `common/rfb/DecodeManager.{h,cxx}` - Eviction sending, bandwidth tracking
- ⏳ `common/rfb/CConnection.cxx` - Verify negotiation preference

#### Phase 5 (Server)
- ⏳ `common/rfb/VNCSConnectionST.{h,cxx}` - Known-hash tracking
- ⏳ `common/rfb/EncodeManager.{h,cxx}` - Enhanced encoder logic

#### Phase 6 (Tests)
- ⏳ `tests/unit/test_persistent_cache_protocol.cxx` (new)
- ⏳ `tests/unit/test_arc_cache.cxx` (new)
- ⏳ `tests/unit/test_bandwidth_stats.cxx` (new)

### Critical Implementation Patterns

#### 1. Row-by-Row Pixel Copying (Bugfix from Nov 5, 2025)

**CRITICAL**: `PixelBuffer::getBuffer()` returns stride in **pixels**, not bytes!

```cpp
// ❌ WRONG - causes SIGSEGV by reading past allocated memory
size_t dataSize = height * stridePixels * bytesPerPixel;
memcpy(cached.pixels.data(), pixels, dataSize);

// ✅ CORRECT - respects stride between rows
const uint8_t* src = pixels;
uint8_t* dst = cached.pixels.data();
size_t rowBytes = width * bytesPerPixel;
size_t srcStrideBytes = stridePixels * bytesPerPixel;
size_t dstStrideBytes = stridePixels * bytesPerPixel;

for (int y = 0; y < height; y++) {
    memcpy(dst, src, rowBytes);
    src += srcStrideBytes;
    dst += dstStrideBytes;
}
```

**Reference**: Commit 4bbb6621, crash report njcvncviewer-2025-11-05-104759.ips

#### 2. Synchronization Discipline

**Pattern from ContentCache**:
1. Compute hash for rectangle
2. Check if client knows hash:
   - If YES: send PersistentCachedRect reference immediately
   - If NO: check if client requested hash:
     - If YES: send PersistentCachedRectInit + register as known + clear request
     - If NO: fall back to normal encoding

**Server State**:
```cpp
std::unordered_set<std::vector<uint8_t>, HashVectorHasher> 
    clientKnownPersistentHashes_;      // Client confirmed has this hash
std::unordered_set<std::vector<uint8_t>, HashVectorHasher> 
    clientRequestedPersistentHashes_;  // Client asked for this hash
```

#### 3. Batching Eviction Messages

```cpp
void CMsgWriter::writePersistentCacheEvictionBatched(
    const std::vector<std::vector<uint8_t>>& hashes)
{
  const size_t batchSize = 100;  // Conservative batch size
  
  for (size_t offset = 0; offset < hashes.size(); offset += batchSize) {
    size_t end = std::min(offset + batchSize, hashes.size());
    std::vector<std::vector<uint8_t>> batch(
        hashes.begin() + offset, hashes.begin() + end);
    writePersistentCacheEviction(batch);
  }
}
```

#### 4. Bandwidth Tracking

```cpp
struct CacheProtocolStats {
  uint64_t cachedRectBytes;       // Reference messages
  uint32_t cachedRectCount;
  uint64_t cachedRectInitBytes;   // Init messages
  uint32_t cachedRectInitCount;
  uint64_t alternativeBytes;      // Estimated baseline
};

// PersistentCachedRect reference: 12 (header) + 1 (hashLen) + hashLen
// PersistentCachedRectInit: 12 (header) + 1 (hashLen) + hashLen + encoding + payload
```

**Output format**:
```
PersistentCache: 4.7 MiB bandwidth saving (90.7% reduction)
```

---

## Development Constraints

### Platform Considerations

**macOS (local development)**:
- ✅ Viewer code compiles and runs
- ✅ Unit tests compile and run
- ❌ Server code does NOT compile (Linux-only)
- ✅ Server code edits allowed (lint/syntax check only)

**Linux (remote server at quartz)**:
- ✅ Full server compilation
- ⚠️ DO NOT run or test server locally
- ⚠️ Use CI for Linux server builds

### Safety Rules (from WARP.md)

**🔴 ABSOLUTELY FORBIDDEN**:
- ❌ `pkill` or `killall` commands
- ❌ Running test servers on displays `:1`, `:2`, or `:3`
- ❌ Killing production VNC processes

**✅ SAFE PRACTICES**:
- ✅ Use e2e test framework (displays `:998`, `:999`)
- ✅ Kill only specific verified PIDs
- ✅ Always use `timeout` for commands that might hang
- ✅ Backup non-git files before editing (suffix: yyy-dd-mm_hhmm.bak)

### Negotiation Preference

**CRITICAL**: Ensure PersistentCache is preferred over ContentCache when both are advertised.

**Viewer** (`common/rfb/CConnection.cxx:1025-1026`):
```cpp
// Push PersistentCache BEFORE ContentCache
encodings.push_back(pseudoEncodingPersistentCache);    // -321
encodings.push_back(pseudoEncodingContentCache);        // -320
```

**Server**: Check for PersistentCache support first, fall back to ContentCache.

---

## Commit Structure

Planned commits for PR (following Phase 2-7 completion):

1. **Protocol: PersistentCache eviction message**
   - msgTypePersistentCacheEviction constant
   - SMsgReader, CMsgWriter implementations
   - Documentation in msgTypes.h

2. **Shared: ArcCache template utility**
   - common/rfb/cache/ArcCache.{h,cxx}
   - Unit tests
   - Documentation

3. **Viewer: GlobalClientPersistentCache ARC upgrade**
   - Migrate to ArcCache
   - Add eviction callback
   - pendingEvictions_ queue

4. **Viewer: Eviction sending and bandwidth tracking**
   - DecodeManager::flush() eviction sending
   - CacheProtocolStats integration
   - Summary logging

5. **Server: Known-hash tracking and eviction handling**
   - clientKnownPersistentHashes_ set
   - clientRequestedPersistentHashes_ set
   - handlePersistentCacheEviction() implementation

6. **Server: Encoder reference vs init logic**
   - Enhanced tryPersistentCacheLookup()
   - Synchronization discipline
   - Statistics tracking

7. **Tests: Protocol and cache unit tests**
   - test_persistent_cache_protocol.cxx
   - test_arc_cache.cxx
   - test_bandwidth_stats.cxx

8. **Docs: Design and implementation notes**
   - PERSISTENTCACHE_DESIGN.md updates
   - PERSISTENTCACHE_PARITY_IMPLEMENTATION_NOTES.md (this file)
   - Parameter documentation

---

## References

- **PERSISTENTCACHE_PARITY_PLAN.md** - Complete implementation plan
- **CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md** - Source of improvements
- **PERSISTENTCACHE_DESIGN.md** - Original design document
- **ARC_ALGORITHM.md** - ARC cache algorithm specification
- **WARP.md** - Project conventions and safety rules

---

## Next Steps

1. Implement SMsgReader::readPersistentCacheEviction()
2. Add SMsgHandler::handlePersistentCacheEviction() virtual method
3. Implement CMsgWriter::writePersistentCacheEviction()
4. Wire up message dispatch in SMsgReader::readMsg()
5. Test protocol message round-trip with unit tests

**Current Focus**: Phase 2 (Protocol Extensions) - Message readers/writers

---

**Last Updated**: January 8, 2026
