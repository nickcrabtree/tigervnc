# PersistentCache Session Bug Fix & Abstraction Plan

**Date:** 2025-11-12  
**Issue:** PersistentCache has 0% hit rate within sessions due to missing session-scoped hash tracking  
**Goal:** Fix the bug while abstracting common cache functionality to prevent future drift

---

## Problem Summary

### Current Bug
PersistentCache only checks the client's **initial hash inventory** (sent at connection start) but never tracks hashes sent **during the current session**. This means:

1. **First occurrence** of content: Server computes hash, doesn't find it in client's initial inventory, **falls back to normal encoding** (no cache benefit)
2. **Second occurrence** of same content: Server computes hash again, **still doesn't find it** (because session tracking is missing), falls back again ❌

**Result:** 0% hit rate within a session, defeating the purpose of the cache for repeated content like UI elements, logos, and icons.

### Why ContentCache Works
ContentCache correctly implements session tracking via:
- `conn->knowsCacheId(cacheId)` - checks if client knows this ID **in this session**
- `conn->markCacheIdKnown(cacheId)` - records when we send new IDs to the client
- Result: First occurrence → send `CachedRectInit` (full data + ID), second occurrence → send `CachedRect` (reference only) ✓

---

## Solution Strategy

### Core Fix
Add **session-scoped hash tracking** to PersistentCache, mirroring ContentCache's approach:

1. Track which hashes the server has sent to the client in this session
2. When encoding a rectangle:
   - If client already knows hash → send `PersistentCachedRect` (reference, 20 bytes)
   - If client doesn't know hash → send `PersistentCachedRectInit` (full data + hash), **then mark hash as known**
3. Never silently "fall back" after computing a hash for a supported rectangle

### Prevent Future Drift
Both caches should follow **identical decision flow**:

```
1. Check: Is cache enabled and rectangle above minimum size?
   → NO: Use normal encoding
   → YES: Continue

2. Compute cache key (ID for ContentCache, hash for PersistentCache)

3. Check: Does client know this key?
   → YES: Send reference (CachedRect or PersistentCachedRect) ← HIT
   → NO: Send Init (full data + key), mark key as known ← MISS/INIT

4. Update statistics (lookups, hits, misses, bytes saved)
```

Extract this flow into shared helpers to ensure both caches remain aligned.

---

## Implementation Plan

### Phase 1: Core Fix (Session Tracking)

#### 1.1 Add Session State to Connection
**Files:** `common/rfb/VNCSConnectionST.h`, `common/rfb/VNCSConnectionST.cxx`, `common/rfb/SConnection.h`

Add to `VNCSConnectionST`:
```cpp
// Session-scoped tracking of persistent hashes known by client
std::unordered_set<std::vector<uint8_t>, HashVectorHasher> knownPersistentHashes_;

// Check if client knows this hash (from inventory OR sent this session)
bool knowsPersistentHash(const std::vector<uint8_t>& hash) const;

// Mark hash as known (after sending PersistentCachedRectInit)
void markPersistentHashKnown(const std::vector<uint8_t>& hash);
```

Add virtual methods to `SConnection` (default no-ops):
```cpp
virtual bool knowsPersistentHash(const std::vector<uint8_t>&) const { return false; }
virtual void markPersistentHashKnown(const std::vector<uint8_t>&) {}
```

#### 1.2 Wire Client Inventory into Session State
**File:** `common/rfb/VNCSConnectionST.cxx`

In `handlePersistentHashList()`:
```cpp
void VNCSConnectionST::handlePersistentHashList(..., const std::vector<std::vector<uint8_t>>& hashes) {
  // Existing: Forward to EncodeManager
  for (const auto& hash : hashes) {
    encodeManager.addClientKnownHash(hash);
  }
  
  // NEW: Also populate session tracking
  for (const auto& hash : hashes) {
    knownPersistentHashes_.insert(hash);
  }
}
```

#### 1.3 Fix PersistentCache Lookup Logic
**File:** `common/rfb/EncodeManager.cxx`

Change `tryPersistentCacheLookup()` to return hash and decision:
```cpp
enum class CacheDecision { NoopUnsupported, HitRef, MissSendInit };

struct PersistentCacheLookupResult {
  CacheDecision decision;
  std::vector<uint8_t> hash;  // Computed hash if decision != NoopUnsupported
};

PersistentCacheLookupResult tryPersistentCacheLookup(const core::Rect& rect, const PixelBuffer* pb) {
  // Check: Cache enabled and above minimum size?
  if (!usePersistentCache || rect.area() < Server::persistentCacheMinRectSize) {
    return { CacheDecision::NoopUnsupported, {} };
  }
  
  persistentCacheStats.cacheLookups++;
  
  // Compute hash (CRITICAL: stride is in pixels, multiply by bytesPerPixel!)
  std::vector<uint8_t> hash = ContentHash::computeRect(pb, rect);
  
  // Check: Does client know this hash?
  if (conn->knowsPersistentHash(hash)) {
    return { CacheDecision::HitRef, hash };  // Client has it, send reference
  } else {
    return { CacheDecision::MissSendInit, hash };  // Client doesn't have it, send Init
  }
}
```

#### 1.4 Update Encoding Path
**File:** `common/rfb/EncodeManager.cxx` in `writeSubRect()`

```cpp
void EncodeManager::writeSubRect(const core::Rect& rect, const PixelBuffer* pb) {
  // Try PersistentCache first
  if (usePersistentCache) {
    auto result = tryPersistentCacheLookup(rect, pb);
    
    switch (result.decision) {
      case CacheDecision::HitRef:
        // Client knows hash, send reference
        persistentCacheStats.cacheHits++;
        conn->writer()->writePersistentCachedRect(rect, result.hash);
        // Update stats...
        return;
      
      case CacheDecision::MissSendInit:
        // Client doesn't know hash, send full data + hash
        persistentCacheStats.cacheMisses++;
        
        // Encode with payload encoder (choose encoder based on rect analysis)
        // ... encoder selection logic ...
        
        conn->writer()->writePersistentCachedRectInit(rect, result.hash, encoder->encoding);
        encoder->writeRect(ppb, palette);
        conn->writer()->endRect();
        
        // Mark hash as known for future references
        conn->markPersistentHashKnown(result.hash);
        return;
      
      case CacheDecision::NoopUnsupported:
        // Fall through to normal encoding
        break;
    }
  }
  
  // Try ContentCache or use normal encoding...
}
```

---

### Phase 2: Abstraction & Drift Prevention

#### 2.1 Unified Statistics Structure
**File:** `common/rfb/cache/CacheStats.h` (new)

```cpp
namespace rfb {
  struct CacheStats {
    uint64_t lookups;
    uint64_t hits;
    uint64_t misses;
    uint64_t bytesSaved;
    
    CacheStats() : lookups(0), hits(0), misses(0), bytesSaved(0) {}
    
    double hitRate() const {
      return lookups > 0 ? (double)hits / lookups : 0.0;
    }
    
    void log(const char* cacheName) const {
      vlog.info("%s: lookups=%llu hits=%llu (%.1f%%) misses=%llu saved=%lluKB",
                cacheName, lookups, hits, hitRate() * 100, misses, bytesSaved / 1024);
    }
  };
}
```

Replace `ContentCacheStats` and `PersistentCacheStats` in `EncodeManager.h` with this unified type.

#### 2.2 Shared Cache Decision Type
**File:** `common/rfb/cache/CacheDecision.h` (new)

```cpp
namespace rfb {
  enum class CacheDecisionKind {
    NoopUnsupported,  // Cache disabled or rect below threshold
    HitRef,           // Client knows key, send reference
    MissSendInit      // Client doesn't know key, send Init
  };
  
  // Safe result type (no union with non-POD types like std::vector)
  struct CacheDecision {
    CacheDecisionKind kind;
    uint64_t cacheId;              // For ContentCache
    std::vector<uint8_t> hash;     // For PersistentCache
    
    bool isHit() const { return kind == CacheDecisionKind::HitRef; }
    bool needsInit() const { return kind == CacheDecisionKind::MissSendInit; }
  };
}
```

#### 2.3 Refactor ContentCache to Match Flow
**File:** `common/rfb/EncodeManager.cxx`

Ensure `tryContentCacheLookup()` follows the same pattern:
- Return `CacheDecision` instead of `bool`
- Check `conn->knowsCacheId()` for session tracking
- If miss, queue `CachedRectInit` and mark known after sending

Both caches now have identical structure, just different key types (ID vs hash).

#### 2.4 Optional: Shared Helper Function
**File:** `common/rfb/EncodeManager.cxx` (private helper)

```cpp
// Template helper for common cache logic (optional, if it reduces duplication)
template<typename KeyType>
CacheDecision tryCacheLookup(
    bool enabled,
    int minRectSize,
    const core::Rect& rect,
    std::function<KeyType()> computeKey,
    std::function<bool(const KeyType&)> clientKnows
) {
  if (!enabled || rect.area() < minRectSize) {
    return { CacheDecisionKind::NoopUnsupported };
  }
  
  KeyType key = computeKey();
  
  if (clientKnows(key)) {
    return { CacheDecisionKind::HitRef, key };
  } else {
    return { CacheDecisionKind::MissSendInit, key };
  }
}
```

**Note:** Keep this lightweight; avoid over-engineering. The main benefit is having both caches follow the same explicit flow, even if not fully templated.

---

### Phase 3: Testing & Validation

#### 3.1 Unit Tests
**File:** `tests/unit/persistentcache_session.cxx` (new)

Test scenarios:
1. First occurrence of hash → should send `PersistentCachedRectInit`
2. Second occurrence of same hash → should send `PersistentCachedRect` (reference)
3. After disconnect, session state cleared → first occurrence sends Init again
4. Client inventory respected → hashes from inventory get references immediately

#### 3.2 E2E Tests
**Files:** 
- `tests/e2e/test_cpp_persistentcache.py` - update expected hit rate to ~50-66%
- `tests/e2e/test_cache_parity.py` (new) - run same workload with both caches, assert similar hit rates

Commands:
```bash
timeout 300 ctest --test-dir build -R persistentcache -V
timeout 300 python3 tests/e2e/test_cache_parity.py
```

**Safety:** All tests use e2e harness on displays `:998`, `:999` only. Never use `pkill`/`killall`.

#### 3.3 Manual Validation
1. Run server with PersistentCache enabled
2. Connect client, draw repeated content (logos, icons)
3. Check server logs:
   - Should see `PersistentCache INIT` for first occurrence
   - Should see `PersistentCache HIT` for subsequent occurrences
   - Hit rate should be ~50-66% for tiled content

---

### Phase 4: Documentation

#### 4.1 New Design Document
**File:** `CACHE_PROTOCOL_DESIGN.md` (new)

Document the unified cache flow:
- Decision tree (disabled → fallback, known → ref, unknown → init)
- Session tracking requirements
- Statistics tracking
- How to add future cache types

#### 4.2 Update Existing Docs
- `PERSISTENTCACHE_DESIGN.md` - add session tracking section
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` - reference shared flow
- `WARP.md` - note about cache abstraction

---

## Success Criteria

### Must Have
- [ ] PersistentCache hit rate > 40% on tiled logo test (was 0%)
- [ ] Session tracking: second occurrence of content → reference, not full encoding
- [ ] Cross-session inventory still works: reconnect with inventory → immediate hits
- [ ] All unit tests pass
- [ ] All e2e tests pass with updated expectations

### Should Have
- [ ] ContentCache and PersistentCache follow identical decision flow
- [ ] Unified statistics structure used by both caches
- [ ] Consistent logging format between caches
- [ ] Parity test shows both caches achieve similar hit rates (±10%)

### Nice to Have
- [ ] Shared helper function reduces code duplication
- [ ] Comprehensive design document for future cache implementations
- [ ] Code comments reference shared flow to prevent drift

---

## Safety Considerations

### Critical Rules (from WARP.md)
1. **NEVER use `pkill` or `killall`** - these kill ALL matching processes including production
2. **Use timeouts for all commands** - prevent hanging
3. **Only test on displays `:998`, `:999`** via e2e harness - never touch production displays `:1`, `:2`, `:3`
4. **Kill by specific PID only** after verification - never by pattern

### Testing Safety
- All tests run through e2e harness which manages isolated test servers
- No manual server starts on production displays
- All build/test commands use timeouts
- No interference with user's working desktop (display `:2`)

### Code Safety
- Stride calculation: **multiply by bytesPerPixel** (learned from October bug)
- Hash computation: use `ContentHash::computeRect()` which handles stride correctly
- Mark-known: only after successfully sending Init, not on normal encoding
- Memory: use `std::vector<uint8_t>` not unions for cross-platform safety

---

## Timeline Estimate

- **Phase 1 (Core Fix):** 2-3 hours
  - Session tracking: 1 hour
  - Lookup fix: 1 hour
  - Encoding path: 1 hour
  
- **Phase 2 (Abstraction):** 2-3 hours
  - Unified stats: 30 min
  - Decision type: 30 min
  - ContentCache refactor: 1-2 hours
  
- **Phase 3 (Testing):** 2-3 hours
  - Unit tests: 1 hour
  - E2E tests: 1 hour
  - Manual validation: 1 hour
  
- **Phase 4 (Documentation):** 1-2 hours

**Total:** 7-11 hours for complete implementation with abstraction

---

## Follow-Up Work

### Immediate (Part of This PR)
- Fix the session tracking bug
- Add unified statistics
- Update tests
- Document the shared flow

### Future (Separate PRs)
- Further deduplication if safe (minRectSize checks, encoder selection)
- Performance optimization (hash computation caching?)
- Cross-cache benchmarking tools
- Unified cache management API for multiple cache types

---

## References

- **Bug Report:** `tests/e2e/PERSISTENTCACHE_SESSION_BUG.md`
- **ContentCache:** `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`
- **PersistentCache:** `PERSISTENTCACHE_DESIGN.md`
- **Safety Rules:** `WARP.md` (especially process management section)
- **Hash Bug:** Stride-in-pixels lesson from 2025-10-07 bug

---

## Approval Checklist

Before merging, verify:
- [ ] PersistentCache hit rate improved from 0% to 50%+
- [ ] No regressions in ContentCache behavior
- [ ] All tests pass (unit + e2e)
- [ ] Cross-session inventory still works
- [ ] No stride/bytesPerPixel errors in hash computation
- [ ] Session state cleared on disconnect
- [ ] Logging consistent between caches
- [ ] Documentation updated
- [ ] No test interference with production displays
- [ ] Code review completed
