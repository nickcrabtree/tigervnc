# PersistentCache Parity Implementation Plan

**Created**: November 5, 2025  
**Based on**: `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md` and `CONTENTCACHE_RUST_PARITY_PLAN.md`  
**Scope**: Apply ContentCache improvements (ARC eviction, bandwidth tracking, synchronization) to PersistentCache  
**Status**: Planning Phase

> Unified cache note (November 2025): This plan assumes separate ContentCache and PersistentCache engines on the C++ side. After the unification work tracked in `docs/remove_contentcache_implementation.md`, all cache behaviour is implemented by a single engine. Any remaining ContentCache-specific migration tasks in this document should be read as historical context; new work should target the unified engine only.

---

## Executive Summary

This document provides an implementation plan to bring **PersistentCache** (hash-based, cross-session caching) to full feature parity with **ContentCache** (session-only, server-assigned IDs). Based on recent ContentCache enhancements (October 30 - November 6, 2025), the PersistentCache implementation needs:

1. **ARC Eviction Protocol** - Full client→server eviction notifications with adaptive cache management
2. **Bandwidth Tracking** - Comprehensive savings metrics and concise reporting
3. **Synchronization Discipline** - Insert-after-encoding, queue-to-next-cycle correctness
4. **Code Deduplication** - Shared ArcCache, BandwidthStats, and protocol helpers
5. **Testing Infrastructure** - Unit tests, e2e tests with timeout discipline, cross-platform validation

**Timeline Estimate**: 4-6 weeks for C++, 2-3 weeks for Rust (separate)  
**Complexity**: High (protocol changes, shared modules, multi-component integration)

---

## Table of Contents

1. [Gap Analysis](#gap-analysis)
2. [Phase 1: Scope and Audit](#phase-1-scope-and-audit)
3. [Phase 2: Protocol Extensions](#phase-2-protocol-extensions)
4. [Phase 3: Shared C++ Modules](#phase-3-shared-c-modules)
5. [Phase 4: C++ Viewer Enhancements](#phase-4-c-viewer-enhancements)
6. [Phase 5: C++ Server Enhancements](#phase-5-c-server-enhancements)
7. [Phase 6: Testing and Validation](#phase-6-testing-and-validation)
8. [Phase 7: Documentation and Rollout](#phase-7-documentation-and-rollout)
9. [Phase 8: Rust Viewer Implementation](#phase-8-rust-viewer-implementation-separate)
10. [Success Criteria](#success-criteria)
11. [Risk Assessment](#risk-assessment)

---

## Gap Analysis

### What PersistentCache Currently Has ✅

From `PERSISTENTCACHE_DESIGN.md` and current implementation:

- ✅ **Hash-based protocol** (encoding 102/103, pseudo-encoding -321)
- ✅ **Basic ARC implementation** in GlobalClientPersistentCache (T1/T2/B1/B2 lists, adaptive p)
- ✅ **Disk persistence** (load/save to `~/.cache/tigervnc/persistentcache.dat`)
- ✅ **Query/response flow** (msgTypePersistentCacheQuery = 254)
- ✅ **Server-side hash computation** (ContentHash utility with correct stride handling)
- ✅ **Basic client/server integration** (DecodeManager, EncodeManager)

### What's Missing ❌

From `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md` Part 1 (C++ ContentCache improvements) and November 6, 2025 ContentKey fix:

#### 0. ContentKey for PersistentCache (November 6, 2025)

**Note**: PersistentCache uses variable-length hashes (not fixed uint64_t cache IDs), so it's LESS susceptible to dimension mismatch issues than ContentCache. However, for consistency and future-proofing:

**Current**: PersistentCache keyed only by hash bytes (std::vector<uint8_t>)  
**Consider**: Composite key with dimensions (width, height, hashBytes)

**Impact**: Low priority for PersistentCache since:
- Hashes are cryptographic (SHA-256), collision probability negligible
- Different-sized rectangles of similar content produce different hashes
- No observed dimension mismatch bugs in PersistentCache

**Recommendation**: Monitor ContentCache ContentKey effectiveness before deciding whether to retrofit PersistentCache. If ContentCache shows improved hit rates or eliminated corruption, consider adding dimensions to PersistentCache hash key.

**Files potentially affected** (if implemented):
- `common/rfb/GlobalClientPersistentCache.{h,cxx}` - Key structure
- `common/rfb/ContentHash.h` - Hash computation with dimension encoding
- `common/rfb/VNCSConnectionST.cxx` - Server-side key tracking

#### 1. ARC Eviction Notifications

**Current**: PersistentCache evicts entries but doesn't notify the server  
**Needed**:
- Client→server eviction message (new msgTypePersistentCacheEviction = 251)
- Batched eviction notifications (similar to ContentCache msg type 250)
- Server tracks client's known-hash set and removes evicted hashes

**Impact**: Server wastes bandwidth sending PersistentCachedRect references for evicted hashes

#### 2. Bandwidth Tracking

**Current**: No bandwidth savings metrics for PersistentCache  
**Needed**:
- Track bytes for PersistentCachedRect (header + hash)
- Track bytes for PersistentCachedRectInit (header + hash + encoding + payload)
- Estimate baseline transmission (without cache)
- Calculate and report bandwidth savings percentage on viewer exit

**Impact**: No visibility into real-world PersistentCache effectiveness

#### 3. Synchronization Discipline

**Current**: PersistentCache may register hashes prematurely  
**Needed**:
- Only register hash as "client-known" when sending PersistentCachedRectInit
- Insert/register AFTER encoding completes
- Queue init to next update cycle (not same cycle as first compute)

**Impact**: Race conditions can cause visual corruption or cache misses

#### 4. Code Duplication

**Current**: ContentCache and PersistentCache have separate ARC implementations  
**Needed**:
- Shared ArcCache utility (template on key type)
- Shared BandwidthStats helpers
- Shared protocol message helpers (batching, validation)

**Impact**: Maintenance burden, risk of divergence, duplicate bugs

#### 5. Parameter and Logging Parity

**Current**: PersistentCache parameters not registered in viewer --help  
**Needed**:
- PersistentCacheSize parameter visible and configurable
- Canonical logging (HIT/MISS/STORE with rect coordinates and hash prefixes)
- Periodic server-side summaries

**Impact**: Poor user experience, difficult debugging

---

## Phase 1: Scope and Audit

**Duration**: 2-3 days  
**Complexity**: Low  
**Dependencies**: None

### Goals

- Enumerate exact deltas from ContentCache improvements
- Audit current PersistentCache implementation
- Create gap report and task breakdown

### Tasks

#### Task 1.1: Read and Analyze ContentCache Changes

**Files to review**:
- `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md` (all 9 parts)
- `CONTENTCACHE_RUST_PARITY_PLAN.md` (phases 1-5)
- `ARC_ALGORITHM.md` (ARC specification)
- Recent commits (Oct 30 - Nov 5, 2025)

**Deliverable**: Summary document of applicable changes

#### Task 1.2: Audit Current PersistentCache Implementation

**C++ Viewer**:
- `common/rfb/GlobalClientPersistentCache.{h,cxx}` - ARC completeness
- `common/rfb/DecodeManager.{h,cxx}` - Integration points
- `common/rfb/CMsgWriter.{h,cxx}` - Protocol messages
- `common/rfb/CMsgReader.{h,cxx}` - Message parsing

**C++ Server**:
- `common/rfb/VNCSConnectionST.{h,cxx}` - Per-connection state
- `common/rfb/EncodeManager.{h,cxx}` - Hash tracking
- `common/rfb/SMsgReader.{h,cxx}` - Client message handling
- `common/rfb/ContentHash.h` - Hashing utility

**Deliverable**: Gap report with specific line numbers and missing features

#### Task 1.3: Create Detailed Task Breakdown

**Deliverable**: Refined task list with:
- Estimated effort (hours/days)
- Dependencies between tasks
- Risk assessment per task
- Acceptance criteria

**Reference**: ContentCache commits for implementation patterns

---

## Phase 2: Protocol Extensions

**Duration**: 1 week  
**Complexity**: Medium  
**Dependencies**: Phase 1 complete  
**Status**: 🔄 IN PROGRESS (Started January 8, 2026)

### Goals

- Define msgTypePersistentCacheEviction (251) wire format
- Add capability negotiation for eviction notifications
- Implement protocol message readers/writers

### 2.1 Protocol Specification

#### Task 2.1: Define Wire Format

**Message Type**: `msgTypePersistentCacheEviction = 251`  
**Direction**: Client → Server  
**Purpose**: Notify server of evicted persistent cache entries

**Wire Format**:
```
┌─────────────────────────────────────┐
│ type: U8 = 251                      │
│ padding: U8 = 0                     │
│ padding: U16 = 0                    │
│ count: U32                          │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: U8                       │
│   hashBytes[hashLen]                │
└─────────────────────────────────────┘
```

**Constraints**:
- Max count: 1000 (per message)
- Max hashLen: 64 bytes
- Max total message size: 64 KB

**File**: `common/rfb/msgTypes.h`

```cpp
const int msgTypePersistentCacheEviction = 251;
```

**Reference**: ContentCache msgTypeCacheEviction = 250 (commit d019b7d9)

#### Task 2.2: Capability Negotiation

**Option A**: Piggyback on existing pseudo-encoding negotiation  
- Client sends `-321` (PersistentCache support)
- Eviction notifications implicitly enabled if both sides support it

**Option B**: Add explicit capability bit  
- Define `rfbCapPersistentCacheEvictNotifyV1`
- Advertise during handshake
- Gate eviction messages on capability presence

**Recommendation**: Option A (simpler, follows ContentCache pattern)

**Fallback**: Client suppresses evictions if server doesn't support -321

#### Task 2.3: Add Protocol Constants

**File**: `common/rfb/encodings.h`

```cpp
// After line 64 (existing PersistentCache constants)

// PersistentCache eviction notification (client→server)
// Similar to encodingCacheEviction (104) but for hash-based cache
// Format: U32 count, followed by count variable-length hashes
const int encodingPersistentCacheEviction = 105;
```

**Documentation**: Add comment explaining relationship to ContentCache eviction (104)

### 2.2 Client Protocol Implementation

#### Task 2.4: Implement CMsgWriter::writePersistentCacheEviction()

**File**: `common/rfb/CMsgWriter.{h,cxx}`

**Signature**:
```cpp
void CMsgWriter::writePersistentCacheEviction(
    const std::vector<std::vector<uint8_t>>& hashes);
```

**Implementation**:
```cpp
void CMsgWriter::writePersistentCacheEviction(
    const std::vector<std::vector<uint8_t>>& hashes)
{
  if (hashes.empty())
    return;
    
  // Validate count
  if (hashes.size() > 1000) {
    vlog.error("Too many hashes to evict (%zu), clamping to 1000",
               hashes.size());
  }
  
  size_t count = std::min(hashes.size(), (size_t)1000);
  
  startMsg(msgTypePersistentCacheEviction);
  os->writeU8(0);   // padding
  os->writeU16(0);  // padding
  os->writeU32(count);
  
  for (size_t i = 0; i < count; i++) {
    const auto& hash = hashes[i];
    
    // Validate hash length
    if (hash.size() > 64) {
      vlog.error("Hash too long (%zu bytes), skipping", hash.size());
      continue;
    }
    
    os->writeU8((uint8_t)hash.size());
    os->writeBytes(hash.data(), hash.size());
  }
  
  endMsg();
}
```

**Validation**:
- Max 1000 hashes per message
- Max 64 bytes per hash
- Graceful handling of oversized inputs

**Reference**: CMsgWriter::writeCacheEviction() (commit d019b7d9)

#### Task 2.5: Add Batching Support

**Problem**: Large eviction sets exceed message size limits

**Solution**: Split into multiple messages

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

### 2.3 Server Protocol Implementation

#### Task 2.6: Implement SMsgReader::readPersistentCacheEviction()

**File**: `common/rfb/SMsgReader.{h,cxx}`

**Signature**:
```cpp
std::vector<std::vector<uint8_t>> 
SMsgReader::readPersistentCacheEviction();
```

**Implementation**:
```cpp
std::vector<std::vector<uint8_t>>
SMsgReader::readPersistentCacheEviction()
{
  is->skip(1);  // padding
  is->skip(2);  // padding
  uint32_t count = is->readU32();
  
  // Validate count
  if (count > 1000) {
    throw protocol_error("Invalid eviction count");
  }
  
  std::vector<std::vector<uint8_t>> hashes;
  hashes.reserve(count);
  
  for (uint32_t i = 0; i < count; i++) {
    uint8_t hashLen = is->readU8();
    
    // Validate hash length
    if (hashLen > 64) {
      throw protocol_error("Invalid hash length");
    }
    
    std::vector<uint8_t> hash(hashLen);
    is->readBytes(hash.data(), hashLen);
    hashes.push_back(hash);
  }
  
  return hashes;
}
```

**Validation**:
- Bounds checks on count and hashLen
- Protocol_error for malformed data

**Reference**: SMsgReader::readCacheEviction() (commit d019b7d9)

#### Task 2.7: Add Handler Interface

**File**: `common/rfb/SMsgHandler.h`

```cpp
virtual void handlePersistentCacheEviction(
    const std::vector<std::vector<uint8_t>>& hashes) = 0;
```

**File**: `common/rfb/VNCSConnectionST.{h,cxx}`

```cpp
// Declaration
virtual void handlePersistentCacheEviction(
    const std::vector<std::vector<uint8_t>>& hashes);

// Implementation
void VNCSConnectionST::handlePersistentCacheEviction(
    const std::vector<std::vector<uint8_t>>& hashes)
{
  if (!cp.supportsPersistentCache())
    return;
    
  size_t removed = 0;
  for (const auto& hash : hashes) {
    if (clientKnownPersistentHashes_.erase(hash) > 0) {
      removed++;
    }
  }
  
  vlog.debug("PersistentCache eviction: %zu hashes removed from known set",
             removed);
  
  persistentCacheStats_.evictionsReceived += hashes.size();
}
```

**Reference**: VNCSConnectionST::handleCacheEviction() (commit 95a1d63c)

### 2.4 Integration

#### Task 2.8: Wire Up Message Dispatch

**File**: `common/rfb/SMsgReader.cxx`

**In readMsg() switch statement**:
```cpp
case msgTypePersistentCacheEviction:
  {
    auto hashes = readPersistentCacheEviction();
    handler->handlePersistentCacheEviction(hashes);
  }
  break;
```

---

## Phase 3: Shared C++ Modules

**Duration**: 2 weeks  
**Complexity**: High  
**Dependencies**: Phase 2 complete  
**Status**: 🔄 IN PROGRESS (Started January 8, 2026)

### Goals

- Create reusable ArcCache template
- Create BandwidthStats helpers
- Create protocol helpers
- Migrate ContentCache and PersistentCache to shared modules

### 3.1 ArcCache Utility

#### Task 3.1: Design Template Interface

**File**: `common/rfb/cache/ArcCache.h` (new directory)

```cpp
template<typename Key, typename Entry>
class ArcCache {
public:
  using ByteSizeFunc = std::function<size_t(const Entry&)>;
  using EvictionCallback = std::function<void(const Key&)>;
  
  ArcCache(size_t maxBytes, ByteSizeFunc sizeFunc,
           EvictionCallback evictCb = nullptr);
  ~ArcCache();
  
  // Cache operations
  bool has(const Key& key) const;
  const Entry* get(const Key& key);
  void insert(const Key& key, Entry entry);
  void clear();
  
  // Statistics
  struct Stats {
    size_t totalEntries;
    size_t totalBytes;
    uint64_t cacheHits;
    uint64_t cacheMisses;
    uint64_t evictions;
    size_t t1Size;
    size_t t2Size;
    size_t b1Size;
    size_t b2Size;
    size_t targetT1Size;  // Adaptive parameter p
  };
  Stats getStats() const;
  
private:
  // ARC lists
  std::list<Key> t1_;  // Recently used once
  std::list<Key> t2_;  // Frequently used
  std::list<Key> b1_;  // Ghost: evicted from T1
  std::list<Key> b2_;  // Ghost: evicted from T2
  
  // Track list membership
  enum ListType { NONE, T1, T2, B1, B2 };
  struct ListInfo {
    ListType list;
    typename std::list<Key>::iterator iter;
  };
  std::unordered_map<Key, ListInfo> listMap_;
  
  // Actual cache storage
  std::unordered_map<Key, Entry> cache_;
  
  // ARC parameters
  size_t maxBytes_;
  size_t currentBytes_;
  size_t p_;  // Adaptive target for T1 size
  
  // Callbacks
  ByteSizeFunc sizeFunc_;
  EvictionCallback evictCb_;
  
  // Statistics
  Stats stats_;
  
  // ARC algorithm helpers
  void replace(const Key& key, size_t entrySize);
  void moveToT2(const Key& key);
  void moveToB1(const Key& key);
  void moveToB2(const Key& key);
  void removeFromList(const Key& key);
  size_t getEntrySize(const Key& key) const;
};
```

**Reference**: ContentCache.cxx ARC implementation (commit 95a1d63c)

#### Task 3.2: Implement ArcCache Template

**File**: `common/rfb/cache/ArcCache.cxx`

**⚠️ CRITICAL BUGFIX (November 5, 2025 - commit 4bbb6621):**

When storing pixel data from `PixelBuffer::getBuffer()`, the returned pointer is NOT contiguous memory.
It points to the first row of a subrect within a larger framebuffer, with rows separated by `stride` bytes.

**DO NOT** use single memcpy:
```cpp
// ❌ WRONG - causes SIGSEGV by reading past allocated memory
size_t dataSize = height * stridePixels * bytesPerPixel;
memcpy(cached.pixels.data(), pixels, dataSize);
```

**MUST** copy row-by-row:
```cpp
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

This bug caused viewer crashes in `ContentCache::storeDecodedPixels()` at line 890 (old code).
Fixed in commit 4bbb6621. See crash report: `njcvncviewer-2025-11-05-104759.ips`

**Implementation strategy**:
1. Copy ARC logic from ContentCache.cxx
2. Templatize on Key and Entry types
3. Replace hardcoded uint64_t with Key template parameter
4. Replace CachedPixels with Entry template parameter
5. Use sizeFunc callback for byte calculations
6. Call evictCb on evictions
7. **IMPORTANT**: Ensure pixel copying uses row-by-row approach (see bugfix above)

**Testing**: Unit tests before integration (see Phase 6)

#### Task 3.3: Migrate ContentCache to ArcCache

**Goal**: Replace ContentCache's internal ARC with ArcCache<uint64_t, CachedPixels>

**Steps**:
1. Add ArcCache member to ContentCache
2. Update all cache operations to delegate to ArcCache
3. Remove duplicate T1/T2/B1/B2 lists and helper methods
4. Run existing ContentCache tests to verify behavior unchanged

**Acceptance**: All existing tests pass without modification

#### Task 3.4: Migrate PersistentCache to ArcCache

**Goal**: Replace GlobalClientPersistentCache's ARC with ArcCache<std::vector<uint8_t>, CachedPixels>

**Steps**:
1. Add ArcCache member with vector hasher
2. Delegate cache operations
3. Remove duplicate ARC code
4. Add eviction callback to populate pendingEvictions_

**Acceptance**: Disk persistence still works, load/save round-trip

### 3.2 BandwidthStats Helpers

Status: Implemented (DecodeManager migrated to shared stats)

#### Task 3.5: Create Shared BandwidthStats Module

**File**: `common/rfb/cache/BandwidthStats.{h,cxx}`

```cpp
struct CacheProtocolStats {
  // With cache
  uint64_t cachedRectBytes;      // Reference messages
  uint32_t cachedRectCount;
  uint64_t cachedRectInitBytes;  // Init messages
  uint32_t cachedRectInitCount;
  
  // Without cache (estimated baseline)
  uint64_t alternativeBytes;
  
  // Computed
  uint64_t bandwidthSaved() const;
  double reductionPercentage() const;
  
  // Formatting
  std::string formatSummary() const;
};

// For ContentCache (20 bytes ref, 24 + payload init)
void trackContentCacheRef(CacheProtocolStats& stats,
                          const core::Rect& r,
                          const PixelFormat& pf);

void trackContentCacheInit(CacheProtocolStats& stats,
                           const core::Rect& r,
                           size_t compressedBytes);

// For PersistentCache (header + hash ref, header + hash + encoding + payload init)
void trackPersistentCacheRef(CacheProtocolStats& stats,
                             const core::Rect& r,
                             size_t hashLen);

void trackPersistentCacheInit(CacheProtocolStats& stats,
                              const core::Rect& r,
                              size_t hashLen,
                              size_t compressedBytes);
```

**Reference**: DecodeManager.cxx bandwidth tracking (commits c9d5fa1d, b1a680c0)

### 3.3 Protocol Helpers

#### Task 3.6: Create Shared Protocol Utilities

**File**: `common/rfb/cache/ProtocolHelpers.{h,cxx}`

```cpp
// Validate and split large ID/hash arrays for batched sending
template<typename T>
std::vector<std::vector<T>> batchForSending(
    const std::vector<T>& items,
    size_t maxBatchSize = 100);

// Validate message counts and sizes
bool validateMessageCount(uint32_t count, uint32_t maxCount = 1000);
bool validateItemSize(size_t size, size_t maxSize = 64);

// Format cache ID/hash for logging
std::string formatCacheId(uint64_t id);
std::string formatHash(const std::vector<uint8_t>& hash, size_t prefixLen = 8);
```

---

## Phase 4: C++ Viewer Enhancements

**Duration**: 1.5 weeks  
**Complexity**: Medium-High  
**Dependencies**: Phase 2, Phase 3 complete  
**Status**: 🔄 IN PROGRESS (Started January 8, 2026)

### Goals

- Upgrade PersistentCache ARC to full parity
- Add eviction callback and pending queue
- Integrate bandwidth tracking
- Send eviction notifications

### 4.1 ARC Upgrade

#### Task 4.1: Migrate to Shared ArcCache

**File**: `common/rfb/GlobalClientPersistentCache.{h,cxx}`

**Changes**:
```cpp
// Before
std::list<std::vector<uint8_t>> t1_;
std::list<std::vector<uint8_t>> t2_;
// ... (ARC implementation)

// After
ArcCache<std::vector<uint8_t>, CachedPixels,
         HashVectorHasher> arcCache_;
std::vector<std::vector<uint8_t>> pendingEvictions_;
```

**Eviction callback**:
```cpp
arcCache_ = new ArcCache<...>(
    maxCacheSize_,
    [](const CachedPixels& entry) { return entry.byteSize(); },
    [this](const std::vector<uint8_t>& hash) {
      pendingEvictions_.push_back(hash);
    });
```

#### Task 4.2: Expose Eviction Queue

**File**: `common/rfb/GlobalClientPersistentCache.h`

```cpp
bool hasPendingEvictions() const {
  return !pendingEvictions_.empty();
}

std::vector<std::vector<uint8_t>> getPendingEvictions() {
  auto result = std::move(pendingEvictions_);
  pendingEvictions_.clear();
  return result;
}
```

### 4.2 Bandwidth Tracking

#### Task 4.3: Add Bandwidth Stats to DecodeManager

**File**: `common/rfb/DecodeManager.h`

```cpp
#include <rfb/cache/BandwidthStats.h>

class DecodeManager {
  // ...
  CacheProtocolStats persistentCacheBandwidthStats_;
};
```

#### Task 4.4: Track PersistentCachedRect

**File**: `common/rfb/DecodeManager.cxx`

**In handlePersistentCachedRect()**:
```cpp
void DecodeManager::handlePersistentCachedRect(
    const core::Rect& r,
    const std::vector<uint8_t>& hash,
    ModifiablePixelBuffer* pb)
{
  // ... existing lookup logic ...
  
  // Track bandwidth
  trackPersistentCacheRef(persistentCacheBandwidthStats_,
                         r, hash.size());
  
  if (cached == nullptr) {
    // ... cache miss handling ...
  } else {
    // ... cache hit handling ...
  }
}
```

#### Task 4.5: Track PersistentCachedRectInit

**In storePersistentCachedRect()**:
```cpp
void DecodeManager::storePersistentCachedRect(
    const core::Rect& r,
    const std::vector<uint8_t>& hash,
    ModifiablePixelBuffer* pb)
{
  // ... existing storage logic ...
  
  // Track bandwidth
  // lastDecodedRectBytes already set by decodeRect()
  trackPersistentCacheInit(persistentCacheBandwidthStats_,
                          r, hash.size(), lastDecodedRectBytes);
}
```

#### Task 4.6: Report Statistics on Shutdown

**In DecodeManager::logStats()**:
```cpp
void DecodeManager::logStats()
{
  // ... existing ContentCache stats ...
  
  // PersistentCache bandwidth summary
  if (persistentCacheBandwidthStats_.cachedRectCount > 0 ||
      persistentCacheBandwidthStats_.cachedRectInitCount > 0) {
    vlog.info("%s", persistentCacheBandwidthStats_.formatSummary().c_str());
  }
}
```

**Output format** (matching ContentCache):
```
PersistentCache: 4.7 MiB bandwidth saving (90.7% reduction)
```

### 4.3 Eviction Notifications

#### Task 4.7: Send Evictions in flush()

**File**: `common/rfb/DecodeManager.cxx`

**In flush() method**:
```cpp
void DecodeManager::flush()
{
  // ... existing work queue drain ...
  
  // Send ContentCache evictions
  if (contentCache != nullptr && contentCache->hasPendingEvictions()) {
    // ... existing ContentCache eviction sending ...
  }
  
  // Send PersistentCache evictions
  if (persistentCache != nullptr && persistentCache->hasPendingEvictions()) {
    auto evictions = persistentCache->getPendingEvictions();
    if (!evictions.empty()) {
      vlog.debug("Sending %zu PersistentCache eviction notifications",
                 evictions.size());
      conn->writer()->writePersistentCacheEvictionBatched(evictions);
    }
  }
  
  // ... existing query flushing ...
}
```

**Ordering**: After work queue empty, after queries sent

---

## Phase 5: C++ Server Enhancements

**Duration**: 1 week  
**Complexity**: Medium  
**Dependencies**: Phase 2, Phase 4 complete  
**Status**: 🔄 IN PROGRESS (Started January 8, 2026)

### Goals

- Track per-connection known-hash set
- Handle eviction notifications
- Synchronization discipline

### 5.1 Per-Connection State - Implemented

- Requested-hash tracking added (VNCSConnectionST)
- Eviction handling updates known-hash set

### 5.2 Encoder Decision Logic - Implemented

- If client knows hash → PersistentCachedRect
- Else if client requested hash → PersistentCachedRectInit (encode + send; then mark known and clear request)
- Else → fallback to normal encoding

### 5.1 Per-Connection State

#### Task 5.1: Add Known-Hash Tracking

**File**: `common/rfb/VNCSConnectionST.h`

```cpp
class VNCSConnectionST {
  // ...
private:
  std::unordered_set<std::vector<uint8_t>, HashVectorHasher>
      clientKnownPersistentHashes_;
      
  struct {
    uint64_t hashesTracked;
    uint64_t referenceSent;
    uint64_t initSent;
    uint64_t evictionsReceived;
  } persistentCacheStats_;
};
```

#### Task 5.2: Register Hash on Init Sent

**File**: `common/rfb/EncodeManager.cxx`

**After queueing PersistentCachedRectInit**:
```cpp
// Only register hash as known after we queue the init
if (conn->supportsPersistentCache()) {
  conn->registerKnownPersistentHash(hash);
}
```

**File**: `common/rfb/VNCSConnectionST.cxx`

```cpp
void VNCSConnectionST::registerKnownPersistentHash(
    const std::vector<uint8_t>& hash)
{
  clientKnownPersistentHashes_.insert(hash);
  persistentCacheStats_.hashesTracked++;
}

bool VNCSConnectionST::hasKnownPersistentHash(
    const std::vector<uint8_t>& hash) const
{
  return clientKnownPersistentHashes_.count(hash) > 0;
}
```

#### Task 5.3: Check Known Hash Before Sending Reference

**File**: `common/rfb/EncodeManager.cxx`

**In tryPersistentCacheLookup()**:
```cpp
bool EncodeManager::tryPersistentCacheLookup(
    const core::Rect& r, const PixelBuffer* pb)
{
  // ... compute hash ...
  
  // Only send reference if client knows this hash
  if (!conn->hasKnownPersistentHash(hash)) {
    return false;  // Fall back to sending init
  }
  
  // Send PersistentCachedRect reference
  writer->writePersistentCachedRect(r, hash);
  persistentCacheStats_.referenceSent++;
  return true;
}
```

### 5.2 Eviction Handling

#### Task 5.4: Remove Evicted Hashes

**File**: `common/rfb/VNCSConnectionST.cxx`

**Implementation** (see Task 2.7 for full code)

### 5.3 Synchronization

#### Task 5.5: Queue Init to Next Cycle

**File**: `common/rfb/EncodeManager.cxx`

**Pattern** (matching ContentCache):
1. Compute hash for rectangle
2. Check if client knows hash
3. If NO: queue encoding work for next cycle, register hash AFTER encoding completes
4. If YES: send reference immediately

**Implementation**:
```cpp
struct PendingPersistentInit {
  core::Rect rect;
  std::vector<uint8_t> hash;
};

std::queue<PendingPersistentInit> pendingPersistentInits_;

// In writeRects():
if (shouldUsePersistentCache && !clientKnowsHash) {
  // Queue for next cycle
  pendingPersistentInits_.push({r, hash});
  continue;
}

// At end of update cycle:
while (!pendingPersistentInits_.empty()) {
  auto& pending = pendingPersistentInits_.front();
  
  // Encode now
  writer->writePersistentCachedRectInit(pending.rect, pending.hash, pb);
  
  // Register as known AFTER sending
  conn->registerKnownPersistentHash(pending.hash);
  
  pendingPersistentInits_.pop();
}
```

**Reference**: ContentCache synchronization (commits e3d1c2b8, 44de3dca)

---

## Phase 6: Testing and Validation

**Duration**: 1.5 weeks  
**Complexity**: High  
**Dependencies**: Phases 3, 4, 5 complete

### Goals

- Unit tests for shared modules
- Unit tests for protocol messages
- e2e tests for eviction, bandwidth, cross-platform

### 6.1 Unit Tests

#### Task 6.1: ArcCache Unit Tests

**File**: `tests/unit/test_arc_cache.cxx` (new)

```cpp
TEST(ArcCache, BasicInsertAndLookup) {
  ArcCache<uint64_t, int> cache(
      1024,  // 1 KB max
      [](const int& val) { return sizeof(int); });
  
  cache.insert(1, 100);
  ASSERT_TRUE(cache.has(1));
  EXPECT_EQ(*cache.get(1), 100);
}

TEST(ArcCache, PromotionT1ToT2) {
  // ... test T1→T2 on second access
}

TEST(ArcCache, GhostHitInB1) {
  // ... test ghost hit adjusts p up
}

TEST(ArcCache, EvictionCallback) {
  std::vector<uint64_t> evicted;
  ArcCache<uint64_t, int> cache(
      100,  // Small capacity
      [](const int&) { return 10; },
      [&](uint64_t key) { evicted.push_back(key); });
  
  // Fill beyond capacity
  for (int i = 0; i < 20; i++) {
    cache.insert(i, i * 10);
  }
  
  EXPECT_GT(evicted.size(), 0);
}
```

**Coverage**: All ARC edge cases from ContentCache behavior

#### Task 6.2: Protocol Message Unit Tests

**File**: `tests/unit/test_persistent_cache_protocol.cxx` (new)

```cpp
TEST(PersistentCacheProtocol, EvictionRoundtrip) {
  std::vector<std::vector<uint8_t>> hashes = {
    {0xAA, 0xBB, 0xCC, 0xDD},
    {0x11, 0x22, 0x33, 0x44, 0x55}
  };
  
  // Write
  rdr::MemOutStream mos;
  CMsgWriter writer(&mos);
  writer.writePersistentCacheEviction(hashes);
  
  // Read
  rdr::MemInStream mis(mos.data(), mos.length());
  SMsgReader reader(&mis, nullptr);
  auto readHashes = reader.readPersistentCacheEviction();
  
  EXPECT_EQ(hashes, readHashes);
}

TEST(PersistentCacheProtocol, LargeHashBatch) {
  // Test batching with 1000+ hashes
}

TEST(PersistentCacheProtocol, InvalidCount) {
  // Test count > 1000 throws protocol_error
}
```

#### Task 6.3: Bandwidth Calculation Tests

**File**: `tests/unit/test_bandwidth_stats.cxx` (new)

```cpp
TEST(BandwidthStats, PersistentCacheRefAccounting) {
  CacheProtocolStats stats{};
  core::Rect r(0, 0, 64, 64);
  size_t hashLen = 16;
  
  trackPersistentCacheRef(stats, r, hashLen);
  
  // 12 bytes (header) + 1 byte (hashLen field) + 16 bytes (hash) = 29 bytes
  EXPECT_EQ(stats.cachedRectBytes, 29u);
  
  // Baseline would be header + encoding + compressed
  // Estimate: 10:1 compression on 64×64×4 = 16384 bytes → ~1638 compressed
  // Total: 12 + 4 + 1638 = 1654 bytes
  EXPECT_GT(stats.alternativeBytes, 1000u);
}

TEST(BandwidthStats, ReductionPercentage) {
  CacheProtocolStats stats{};
  stats.cachedRectBytes = 100;
  stats.alternativeBytes = 10000;
  
  double reduction = stats.reductionPercentage();
  EXPECT_NEAR(reduction, 99.0, 0.1);
}
```

#### Task 6.4: Hashing Stride Test

**File**: `tests/unit/test_content_hash.cxx`

```cpp
TEST(ContentHash, StrideInPixelsNotBytes) {
  // Create test pixel buffer
  uint8_t pixels[256];
  PixelFormat pf(32, 24, ...);
  core::Rect r(0, 0, 8, 8);
  
  int stridePixels = 10;  // Stride includes padding
  int bytesPerPixel = pf.bpp / 8;
  
  // Hash should cover stridePixels × bytesPerPixel bytes per row
  auto hash = ContentHash::computeRect(pixels, r, pf, stridePixels);
  
  // ... verify hash matches expected value
  // This test prevents regression of stride-in-pixels bug (Oct 7 2025)
}
```

### 6.2 E2E Tests

#### Task 6.5: PersistentCache Eviction Test

**File**: `tests/e2e/test_persistent_cache_eviction.py` (new)

```python
#!/usr/bin/env python3
"""
Test PersistentCache eviction notifications.

Verifies:
- Client sends evictions (251) when cache is full
- Server removes evicted hashes from known set
- Server stops sending PersistentCachedRect for evicted hashes
"""

def test_eviction_notification():
    # Start server on :998 with small persistent cache
    server = start_test_server(
        display=998,
        persistent_cache_size_mb=16,
        log_file="/tmp/persistent_evict_server.log"
    )
    
    # Start C++ viewer with tiny cache to force evictions
    viewer = start_cpp_viewer(
        display=998,
        persistent_cache_size_mb=8,  # Force evictions
        log_file="/tmp/persistent_evict_viewer.log"
    )
    
    # Generate content exceeding cache size
    drive_workload(duration=60, create_large_images=True)
    
    # Verify logs
    server_log = read_log("/tmp/persistent_evict_server.log")
    assert "PersistentCache eviction: " in server_log
    assert "hashes removed from known set" in server_log
    
    viewer_log = read_log("/tmp/persistent_evict_viewer.log")
    assert "Sending" in viewer_log and "PersistentCache eviction" in viewer_log
```

**CRITICAL**: Use isolated displays `:998/:999` (NEVER `:1/:2/:3`)  
**CRITICAL**: All commands MUST use `timeout` per WARP.md  
**CRITICAL**: NEVER use `pkill` or `killall`

#### Task 6.6: Bandwidth Tracking Test

**File**: `tests/e2e/test_persistent_cache_bandwidth.py` (new)

```python
def test_bandwidth_savings():
    server = start_test_server(display=998)
    viewer = start_cpp_viewer(
        display=998,
        log_file="/tmp/persistent_bandwidth_viewer.log"
    )
    
    # Exercise repeated frames (high cache hit rate)
    drive_workload(duration=30, repeat_content=True)
    
    # Stop viewer gracefully
    viewer.terminate()
    viewer.wait(timeout=5)
    
    # Check for bandwidth summary
    log = read_log("/tmp/persistent_bandwidth_viewer.log")
    
    # Should see summary line:
    # "PersistentCache: X.X MiB bandwidth saving (YY.Y% reduction)"
    assert re.search(r"PersistentCache:.*bandwidth saving.*reduction", log)
    
    # Extract percentage
    match = re.search(r"(\d+\.\d+)% reduction", log)
    if match:
        reduction = float(match.group(1))
        assert reduction > 80.0, f"Expected >80% reduction, got {reduction}%"
```

#### Task 6.7: Cross-Platform Test

**File**: `tests/e2e/test_persistent_cache_cross_platform.sh` (enhance existing)

```bash
# Test macOS viewer ↔ Linux server with PersistentCache

# On Linux server (quartz)
timeout 120 ssh nickc@quartz 'cd /home/nickc/code/tigervnc && \
  build/unix/vncserver/Xnjcvnc :998 -PersistentCache=1 -PersistentCacheSize=256'

# On macOS
timeout 60 build/vncviewer/njcvncviewer -PersistentCache=1 \
  -Log=*:stderr:100 birdsurvey.hopto.org:998 \
  2>&1 | tee /tmp/persistent_cross_platform.log

# Verify PersistentCache engaged
grep "PersistentCache.*bandwidth saving" /tmp/persistent_cross_platform.log
```

### 6.3 Parity Validation

#### Task 6.8: ContentCache vs PersistentCache Hit Rate Comparison

**Goal**: Verify both caches achieve similar hit rates on identical workloads

**Script**: `tests/e2e/compare_cache_hit_rates.py`

```python
def test_cache_parity():
    workload_script = "scripts/standard_workload.py"
    
    # Run with ContentCache
    run_test(cache_type="ContentCache", workload=workload_script)
    cc_stats = parse_stats("/tmp/contentcache_test.log")
    
    # Run with PersistentCache
    run_test(cache_type="PersistentCache", workload=workload_script)
    pc_stats = parse_stats("/tmp/persistentcache_test.log")
    
    # Compare hit rates
    cc_hit_rate = cc_stats.hits / (cc_stats.hits + cc_stats.misses)
    pc_hit_rate = pc_stats.hits / (pc_stats.hits + pc_stats.misses)
    
    diff = abs(cc_hit_rate - pc_hit_rate)
    assert diff < 0.05, f"Hit rates differ by {diff*100}% (tolerance: 5%)"
```

---

## Phase 7: Documentation and Rollout

**Duration**: 3-4 days  
**Complexity**: Low  
**Dependencies**: Phases 4, 5, 6 complete

### 7.1 Documentation Updates

#### Task 7.1: Protocol Documentation

**File**: `common/rfb/encodings.h`

**Add comments**:
```cpp
// PersistentCache eviction notification (client→server)
// Message type 251: Client notifies server of evicted persistent cache entries
// This allows server to stop sending PersistentCachedRect (102) references
// for hashes the client no longer has in memory.
//
// Wire format (msgTypePersistentCacheEviction = 251):
//   U8: message type (251)
//   U8: padding (0)
//   U16: padding (0)
//   U32: count (number of evicted hashes)
//   Repeated count times:
//     U8: hashLen
//     U8[hashLen]: hash bytes
//
// See also: encodingCacheEviction (104) for ContentCache equivalent
const int encodingPersistentCacheEviction = 105;
```

#### Task 7.2: Design Document Updates

**File**: `PERSISTENTCACHE_DESIGN.md`

**Add sections**:
- Eviction Protocol (msgTypePersistentCacheEviction details)
- ARC Eviction Algorithm (reference to shared ArcCache)
- Bandwidth Tracking and Reporting
- Synchronization Rules (when to register hashes as known)

#### Task 7.3: Implementation Notes

**File**: `PERSISTENTCACHE_PARITY_IMPLEMENTATION_NOTES.md` (new)

**Contents**:
- Mapping of ContentCache commits to PersistentCache changes
- Shared module architecture
- Migration guide for existing deployments
- Troubleshooting common issues

#### Task 7.4: Parameter Documentation

**Viewer --help**:
```
PersistentCache Options:
  -PersistentCache          Enable PersistentCache protocol (default: off)
  -PersistentCacheSize=N    Cache size in MB (default: 2048)
  -PersistentCachePath=PATH Custom cache file path
                            (default: ~/.cache/tigervnc/persistentcache.dat)
```

**Server --help**:
```
PersistentCache Options:
  -PersistentCache                Enable PersistentCache protocol (default: on)
  -PersistentCacheMinRectSize=N   Min rect size in pixels to cache (default: 4096)
```

### 7.2 CMake and Build Integration

#### Task 7.5: Update CMakeLists.txt

**File**: `common/rfb/CMakeLists.txt`

```cmake
set(RFB_SOURCES
  # ... existing files ...
  
  # Cache utilities
  cache/ArcCache.cxx
  cache/BandwidthStats.cxx
  cache/ProtocolHelpers.cxx
)
```

#### Task 7.6: CI Integration

**File**: `.github/workflows/test-persistent-cache.yml` (new, if using GitHub Actions)

```yaml
name: PersistentCache Tests

on:
  push:
    branches: [ main ]
  pull_request:

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Build
        run: |
          cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug
          make -C build -j$(nproc)
      - name: Run Unit Tests
        run: |
          ctest --test-dir build -R PersistentCache -V
  
  e2e-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Setup Test Environment
        run: |
          sudo apt-get update
          sudo apt-get install -y xvfb xorg-server-source
      - name: Build
        run: |
          cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
          make -C build -j$(nproc)
      - name: Run E2E Tests
        run: |
          cd tests/e2e
          timeout 300 python3 test_persistent_cache_eviction.py
          timeout 300 python3 test_persistent_cache_bandwidth.py
```

### 7.3 Staged Rollout

#### Task 7.7: Rollout Plan

**Stage 1**: Shared modules + unit tests  
- Land ArcCache, BandwidthStats, ProtocolHelpers
- Run unit tests in CI
- No behavior changes for users

**Stage 2**: Migrate ContentCache to shared ARC  
- Swap ContentCache to use ArcCache
- Verify all existing tests pass
- Monitor for regressions

**Stage 3**: PersistentCache viewer enhancements  
- Add eviction notifications (client-side only)
- Add bandwidth tracking
- Still works with old servers (no eviction support)

**Stage 4**: Server-side eviction handling  
- Add known-hash tracking
- Handle eviction messages
- Synchronization discipline

**Stage 5**: Enable by default (optional)  
- Make PersistentCache opt-out instead of opt-in
- Communicate to users in release notes

---

## Phase 8: Rust Viewer Implementation (Separate)

**Duration**: 2-3 weeks  
**Complexity**: Medium  
**Dependencies**: C++ implementation complete (Phases 1-7)

**NOTE**: This phase is intentionally separate and can be done as a follow-on task after C++ parity is achieved.

### 8.1 Shared Modules

#### Task 8.1: Create arc_cache Module

**File**: `rust-vnc-viewer/rfb-cache/src/arc_cache.rs` (new crate)

```rust
pub struct ArcCache<K, V> {
    t1: LinkedList<K>,
    t2: LinkedList<K>,
    b1: LinkedList<K>,
    b2: LinkedList<K>,
    cache: HashMap<K, V>,
    list_map: HashMap<K, ListInfo>,
    p: usize,
    max_bytes: usize,
    current_bytes: usize,
    evict_callback: Option<Box<dyn Fn(&K)>>,
}

impl<K, V> ArcCache<K, V> {
    pub fn new(max_bytes: usize, evict_callback: Option<Box<dyn Fn(&K)>>) -> Self;
    pub fn get(&mut self, key: &K) -> Option<&V>;
    pub fn insert(&mut self, key: K, value: V, size: usize);
    pub fn stats(&self) -> ArcStats;
}
```

**Tests**: Mirror C++ ArcCache tests

#### Task 8.2: Create bandwidth_stats Module

**File**: `rust-vnc-viewer/rfb-cache/src/bandwidth_stats.rs`

```rust
pub struct CacheProtocolStats {
    pub cached_rect_bytes: u64,
    pub cached_rect_count: u32,
    pub cached_rect_init_bytes: u64,
    pub cached_rect_init_count: u32,
    pub alternative_bytes: u64,
}

impl CacheProtocolStats {
    pub fn bandwidth_saved(&self) -> u64;
    pub fn reduction_percentage(&self) -> f64;
    pub fn format_summary(&self) -> String;
}
```

### 8.2 PersistentClientCache Upgrade

#### Task 8.3: Migrate to ArcCache

**File**: `rust-vnc-viewer/rfb-encodings/src/persistent_cache.rs`

**Replace HashMap with ArcCache**:
```rust
pub struct PersistentClientCache {
    cache: ArcCache<[u8; 16], PersistentCachedPixels>,
    pending_evictions: Vec<[u8; 16]>,
}

impl PersistentClientCache {
    pub fn new(max_size_mb: usize) -> Self {
        let evict_cb = Box::new(|id: &[u8; 16]| {
            // Callback will be set via closure below
        });
        
        // ... (implementation with callback to populate pending_evictions)
    }
    
    pub fn has_pending_evictions(&self) -> bool;
    pub fn drain_pending_evictions(&mut self) -> Vec<[u8; 16]>;
}
```

### 8.3 Protocol Implementation

#### Task 8.4: Add Eviction Message Writer

**File**: `rust-vnc-viewer/rfb-protocol/src/messages/client.rs`

```rust
pub struct PersistentCacheEviction {
    pub hashes: Vec<[u8; 16]>,
}

impl PersistentCacheEviction {
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        stream.write_u8(251)?;  // msgTypePersistentCacheEviction
        stream.write_u8(0)?;    // padding
        stream.write_u16(0)?;   // padding
        stream.write_u32(self.hashes.len() as u32)?;
        
        for hash in &self.hashes {
            stream.write_u8(hash.len() as u8)?;
            stream.write_bytes(hash)?;
        }
        
        Ok(())
    }
}
```

#### Task 8.5: Send Evictions Post-FBU

**File**: `rust-vnc-viewer/rfb-client/src/framebuffer.rs`

```rust
impl Framebuffer {
    pub fn take_persistent_cache_evictions(&mut self) -> Vec<[u8; 16]> {
        if let Some(cache) = self.persistent_cache.as_mut() {
            cache.drain_pending_evictions()
        } else {
            Vec::new()
        }
    }
}
```

**File**: `rust-vnc-viewer/rfb-client/src/event_loop.rs`

```rust
// After processing FramebufferUpdate
if let Some(fb) = framebuffer.as_mut() {
    let evictions = fb.take_persistent_cache_evictions();
    if !evictions.is_empty() {
        debug!("Sending {} PersistentCache evictions", evictions.len());
        let msg = PersistentCacheEviction { hashes: evictions };
        msg.write_to(&mut client.stream).await?;
    }
}
```

### 8.4 Bandwidth Tracking

#### Task 8.6: Add Tracking to Decoder

**File**: `rust-vnc-viewer/rfb-encodings/src/persistent_cached_rect.rs`

```rust
impl Decoder for PersistentCachedRectDecoder {
    async fn decode(...) -> Result<()> {
        // ... existing decode logic ...
        
        // Track bandwidth
        if let Some(stats) = buffer.get_bandwidth_stats_mut() {
            stats.track_persistent_cache_ref(rect, id.len());
        }
        
        // ... blit ...
    }
}
```

#### Task 8.7: Report on Exit

**File**: `rust-vnc-viewer/src/main.rs`

```rust
// On shutdown
if let Some(stats) = framebuffer.persistent_cache_bandwidth_stats() {
    info!("{}", stats.format_summary());
}
```

### 8.5 Testing

#### Task 8.8: Rust Unit Tests

**File**: `rust-vnc-viewer/rfb-cache/tests/arc_cache_tests.rs`

```rust
#[test]
fn test_arc_promotion_t1_to_t2() {
    let mut cache = ArcCache::new(1024, None);
    cache.insert(1, vec![0u8; 100], 100);
    
    // First access: stays in T1
    cache.get(&1);
    assert_eq!(cache.stats().t1_size, 1);
    
    // Second access: promotes to T2
    cache.get(&1);
    assert_eq!(cache.stats().t2_size, 1);
    assert_eq!(cache.stats().t1_size, 0);
}
```

#### Task 8.9: Rust E2E Tests

**Goal**: Run existing e2e tests with Rust viewer

```bash
# tests/e2e/test_persistent_cache_eviction.py
# Add --viewer rust flag

./test_persistent_cache_eviction.py --viewer rust
```

#### Task 8.10: Parity Validation

**Script**: `tests/e2e/compare_cpp_rust_persistent_cache.py`

```python
def test_cpp_rust_parity():
    workload = "scripts/standard_workload.py"
    
    # Run with C++ viewer
    cpp_stats = run_test(viewer="cpp", workload=workload)
    
    # Run with Rust viewer
    rust_stats = run_test(viewer="rust", workload=workload)
    
    # Compare bandwidth savings
    cpp_reduction = cpp_stats.reduction_percentage
    rust_reduction = rust_stats.reduction_percentage
    
    diff = abs(cpp_reduction - rust_reduction)
    assert diff < 5.0, f"Reductions differ by {diff}% (tolerance: 5%)"
```

---

## Success Criteria

### Functional Criteria ✅

1. **ARC Parity**:
   - [ ] PersistentCache uses shared ArcCache utility
   - [ ] Promotion T1→T2 works on second access
   - [ ] Ghost hits adjust adaptive parameter p correctly
   - [ ] Eviction callback populates pending queue

2. **Eviction Protocol**:
   - [ ] Client sends msgTypePersistentCacheEviction (251)
   - [ ] Server receives and parses eviction messages
   - [ ] Server removes evicted hashes from known set
   - [ ] Server stops sending references for evicted hashes

3. **Bandwidth Tracking**:
   - [ ] PersistentCachedRect bytes tracked correctly
   - [ ] PersistentCachedRectInit bytes tracked correctly
   - [ ] Baseline bytes estimated reasonably
   - [ ] One-line summary emitted on viewer exit

4. **Synchronization**:
   - [ ] Hash registered as known only after sending init
   - [ ] Init queued to next update cycle
   - [ ] No references sent before client has hash

5. **Code Sharing**:
   - [ ] ContentCache uses shared ArcCache
   - [ ] PersistentCache uses shared ArcCache
   - [ ] BandwidthStats helpers used by both caches
   - [ ] Protocol helpers reduce duplication

### Performance Criteria 📊

1. **Hit Rate**:
   - PersistentCache hit rate ≥ 80% on typical workloads
   - Within 5% of ContentCache hit rate for same workload

2. **Bandwidth Savings**:
   - ≥ 90% reduction for high hit rate scenarios
   - Summary reports realistic savings percentage

3. **Overhead**:
   - ARC metadata < 1% of cache size
   - Eviction notifications < 1 KB per second average

### Testing Criteria 🧪

1. **Unit Tests**:
   - [ ] ArcCache tests cover all ARC edge cases
   - [ ] Protocol roundtrip tests pass
   - [ ] Bandwidth calculation tests pass
   - [ ] Stride hashing regression test passes

2. **E2E Tests**:
   - [ ] Eviction test passes (forces evictions, verifies notifications)
   - [ ] Bandwidth test passes (verifies summary output)
   - [ ] Cross-platform test passes (macOS ↔ Linux)
   - [ ] All tests use timeouts and avoid pkill/killall

3. **Parity Tests**:
   - [ ] C++ ContentCache vs PersistentCache hit rates within 5%
   - [ ] C++ vs Rust PersistentCache hit rates within 5%

### Documentation Criteria 📚

1. **Protocol Docs**:
   - [ ] encodings.h has msgTypePersistentCacheEviction docs
   - [ ] Wire format specified with diagrams
   - [ ] Capability negotiation explained

2. **Implementation Docs**:
   - [ ] PERSISTENTCACHE_DESIGN.md updated
   - [ ] Migration notes for shared modules
   - [ ] Troubleshooting guide

3. **User Docs**:
   - [ ] Parameters documented in --help
   - [ ] Examples in README
   - [ ] Release notes mention new features

---

## Risk Assessment

### High Risk ⚠️

#### Risk 1: Shared ArcCache Breaks ContentCache

**Likelihood**: Medium  
**Impact**: High  
**Mitigation**:
- Migrate ContentCache first with extensive testing
- Keep old implementation available for rollback
- Run all existing ContentCache tests before/after

#### Risk 2: Eviction Protocol Incompatibility

**Likelihood**: Low  
**Impact**: High  
**Mitigation**:
- Use separate message type (251) to avoid conflicts
- Test with old/new server combinations
- Document backward compatibility behavior

### Medium Risk ⚙️

#### Risk 3: Hash Synchronization Bugs

**Likelihood**: Medium  
**Impact**: Medium  
**Mitigation**:
- Follow proven ContentCache synchronization pattern
- Add e2e tests that stress ordering
- Extensive logging at debug level

#### Risk 4: Rust-C++ Parity Divergence

**Likelihood**: Medium  
**Impact**: Medium  
**Mitigation**:
- Implement C++ first as reference
- Rust follows C++ API shape closely
- Parity tests enforce consistency

### Low Risk ℹ️

#### Risk 5: Performance Regression

**Likelihood**: Low  
**Impact**: Low  
**Mitigation**:
- Shared modules template-based (zero-cost abstraction)
- Benchmark before/after migration
- Profile hot paths

#### Risk 6: Documentation Drift

**Likelihood**: Medium  
**Impact**: Low  
**Mitigation**:
- Update docs as part of each PR
- Review docs in code review
- Keep WARP.md as single source of truth

---

## Timeline Summary

### C++ Implementation (4-6 weeks)

| Phase | Duration | Key Deliverables |
|-------|----------|------------------|
| 1. Scope & Audit | 2-3 days | Gap report, task breakdown |
| 2. Protocol | 1 week | msgType 251, reader/writer |
| 3. Shared Modules | 2 weeks | ArcCache, BandwidthStats, helpers |
| 4. Viewer | 1.5 weeks | Eviction queue, bandwidth tracking |
| 5. Server | 1 week | Known-hash set, sync discipline |
| 6. Testing | 1.5 weeks | Unit tests, e2e tests, parity |
| 7. Docs & Rollout | 3-4 days | Documentation, CI, staged rollout |

**Total**: 4-6 weeks depending on complexity and testing depth

### Rust Implementation (2-3 weeks, separate)

| Phase | Duration | Key Deliverables |
|-------|----------|------------------|
| 8.1. Shared Modules | 1 week | arc_cache, bandwidth_stats |
| 8.2. Cache Upgrade | 3-4 days | ArcCache integration |
| 8.3. Protocol | 3-4 days | Message writer, eviction sending |
| 8.4. Bandwidth | 2-3 days | Tracking and reporting |
| 8.5. Testing | 1 week | Unit tests, e2e tests, parity |

**Total**: 2-3 weeks (can start after C++ Phase 5)

---

## References

- **CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md** - Source of ContentCache improvements
- **CONTENTCACHE_RUST_PARITY_PLAN.md** - Rust parity implementation plan
- **ARC_ALGORITHM.md** - ARC algorithm specification
- **PERSISTENTCACHE_DESIGN.md** - Existing PersistentCache protocol design
- **WARP.md** - Project conventions and safety rules

---

## Appendix: Task Dependencies

```
Phase 1 (Scope & Audit)
  ↓
Phase 2 (Protocol) ← Phase 3 (Shared Modules)
  ↓                       ↓
Phase 4 (Viewer) ←───────┘
  ↓
Phase 5 (Server)
  ↓
Phase 6 (Testing)
  ↓
Phase 7 (Docs & Rollout)
  ↓
Phase 8 (Rust - separate track)
```

**Critical Path**: Phase 1 → 2 → 4 → 5 → 6 → 7  
**Parallel Work**: Phase 3 can overlap with Phase 2

---

**END OF IMPLEMENTATION PLAN**
