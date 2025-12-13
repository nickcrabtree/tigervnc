# Lossy vs Lossless Cache Behavior

**Date**: 2025-12-13  
**Status**: IN PROGRESS (Viewer-Managed Dual-Hash Design)

## Overview

The TigerVNC cache system (both ContentCache and PersistentCache) supports caching both lossless and lossy content, with automatic quality preference to ensure the best visual fidelity.

## Content Hash IDs

### Canonical ID (Lossless)
- Computed from the **server's framebuffer** before encoding
- Represents the "true" pixel values without any compression artifacts
- Used for lossless encodings (Raw, ZRLE without JPEG subencodings)

### Lossy ID (Compressed)
- Computed from the **decoded pixels at the client** after lossy compression
- Represents the actual pixels the client has (with compression artifacts)
- Different from canonical ID due to JPEG/lossy compression
- Used for lossy encodings (Tight with JPEG, TightJPEG)

## Storage Behavior

### NEW: Viewer-Managed Dual-Hash Design

Each cache entry stores **BOTH** the canonical and actual hash:

```cpp
struct CachedPixels {
    uint64_t canonicalHash;  // Server's lossless hash (always stored)
    uint64_t actualHash;     // Client's computed hash (may differ if lossy)
    PixelFormat format;
    uint16_t width, height;
    std::vector<uint8_t> pixels;
    int stridePixels;
    bool isPersistable;      // TRUE for both lossless AND lossy (changed!)
};
```

**Key Changes**:
1. Store BOTH hashes with every entry
2. Persist lossy entries to disk (set `isPersistable=true`)
3. Viewer manages canonical→lossy mapping (not server)
4. Cross-session lossy cache hits now work

### Client-Side Storage

```cpp
uint64_t actualHash = hash(decoded_pixels);
bool isLossless = (actualHash == canonicalHash);

// Store entry with BOTH hashes
CachedPixels entry;
entry.canonicalHash = canonicalHash;  // Server's canonical ID
entry.actualHash = actualHash;        // Client's computed hash
entry.pixels = decoded_pixels;
entry.isPersistable = true;           // ALWAYS persist (even lossy!)

persistentCache->insert(entry);
```

**Indexing**: Entries are indexed by `actualHash` for fast lookup, but also searchable by `canonicalHash`.

### Client-Side Lookup

When viewer receives `PersistentCachedRect(canonicalId)`:

```cpp
// 1. Try direct lookup by canonical hash (lossless hit)
entry = cache.findByCanonicalHash(canonicalId);
if (entry && entry->actualHash == canonicalId) {
    return entry;  // Lossless hit!
}

// 2. Try lookup by canonical hash where actualHash differs (lossy hit)
entry = cache.findByCanonicalHash(canonicalId);
if (entry) {
    return entry;  // Lossy hit (have lossy version of this canonical content)!
}

// 3. Miss - request from server
sendPersistentCacheQuery(canonicalId);
```

**Benefits**:
- ✅ Viewer decides whether to use lossy version
- ✅ Lossy entries persist across sessions
- ✅ No server-side lossy mapping needed
- ✅ Works even if viewer was restarted

### Server-Side Behavior

**Simplified!** Server only needs to:
1. Always send canonical hash in references
2. Track which canonical IDs the viewer has (any quality)
3. No lossy hash mapping needed

```cpp
// Server just checks if viewer has canonical ID (any quality level)
if (conn->knowsPersistentId(canonicalId)) {
    // Viewer has this content (either lossless or lossy)
    conn->writer()->writePersistentCachedRect(rect, canonicalId);
} else {
    // Viewer doesn't have it - send full data
    sendFullData(rect, canonicalId);
}
```

## Quality Preference

### Viewer-Side Decision

Viewer decides quality preference during lookup:

```cpp
// Viewer receives PersistentCachedRect(canonicalId)
entry = cache.findByCanonicalHash(canonicalId);

if (!entry) {
    // Miss - don't have this content
    sendQuery(canonicalId);
} else if (entry->actualHash == canonicalId) {
    // Have lossless version - use it!
    blitPixels(entry->pixels);
} else {
    // Have lossy version - use it (better than re-downloading!)
    blitPixels(entry->pixels);
    
    // Optionally: request lossless upgrade in background
    // (future enhancement)
}
```

**Quality Upgrades**: When viewer receives new lossless version of content it has lossy:
1. Store new lossless entry (same canonical hash, but actualHash now matches)
2. Old lossy entry remains until evicted
3. Future lookups will find lossless version first (exact hash match)

## Quality Upgrade Path (NEW DESIGN)

### Scenario: Lossy First, Then Lossless

1. **First occurrence (lossy encoding)**:
   - Server sends `PersistentCachedRectInit(canonical=100)` with JPEG encoding
   - Client decodes → computes `actualHash=200`, stores entry with `{canonical=100, actual=200, pixels}`
   - Entry is indexed by `actualHash=200` but searchable by `canonicalHash=100`
   - Client sends `HashTypeReport(canonical=100, type=LOSSY)`
   - Server marks canonical ID 100 as "known (lossy quality)"

2. **Second occurrence (still lossy)**:
   - Server checks: `knowsPersistentId(100)` → TRUE (viewer has it, quality=lossy)
   - Server sends `PersistentCachedRect(100)` with canonical ID
   - Client searches by `canonicalHash=100` → finds entry with `actual=200`
   - Client recognizes lossy hit (actual ≠ canonical), blits pixels → **HIT** ✅
   - Client sends `HashTypeReport(canonical=100, type=LOSSY)` (confirming still lossy)

3. **Later occurrence (lossless encoding available)**:
   - Server sends `PersistentCachedRectInit(canonical=100)` with lossless encoding
   - Client decodes → computes `actualHash=100` (perfect match!)
   - Client stores NEW entry: `{canonical=100, actual=100, pixels}`
   - Both entries now exist (lossy version + lossless version)
   - Client sends `HashTypeReport(canonical=100, type=LOSSLESS)`
   - Server marks canonical ID 100 as "known (lossless quality)"

4. **Future occurrences**:
   - Server sends `PersistentCachedRect(100)` with canonical ID
   - Client searches by `canonicalHash=100` → finds entry with `actual=100` (exact match!)
   - Client uses lossless version → **HIT with better quality!** ✅
   - Old lossy version remains in cache (evicted later by LRU)

**Result**: Cache automatically upgrades to lossless, both versions coexist temporarily.

## Dual Storage with Quality Tracking (NEW)

### USED: Dual-Hash Storage

Each cache entry stores both hashes:
- ✅ Canonical hash (server's lossless reference)
- ✅ Actual hash (client's computed hash, may differ if lossy)
- ✅ Indexed by actual hash for fast direct lookup
- ✅ Searchable by canonical hash for server references
- ✅ Single pixel buffer (not duplicated)

**Benefits**:
- Lossy and lossless versions are separate entries (no confusion)
- Viewer-side canonical hash lookup works even if hash differs
- Cross-session lossy cache hits work (both hashes persisted)
- Quality upgrades create new entries without invalidating old

### NOT USED: Server-Side Lossy Mapping

We do NOT use server-side lossy hash tracking because:
- ❌ Server state lost on restart
- ❌ Doesn't work cross-session
- ❌ Requires complex mapping management
- ✅ Viewer-managed lookup is simpler and more reliable

## Memory and Disk Persistence

### CHANGED: Both Lossy and Lossless Persist

**New Behavior**:
- **Both lossy and lossless** entries are persisted to disk (`isPersistable=true`)
- Canonical hash stored with entry allows lookup even if hash differs
- Lossy entries survive session restarts
- Viewer can use lossy cached content even after restart

**Why This Is Safe**:
- Canonical hash uniquely identifies the content
- Lossy pixels are stable once stored (JPEG decode is deterministic)
- Viewer knows if it has lossy vs lossless (compare actualHash == canonicalHash)
- No visual drift: same lossy pixels reused consistently

**Benefits**:
- ✅ Cross-session cache hits for lossy content
- ✅ Reduced bandwidth even after viewer restart
- ✅ Simpler server logic (no lossy mapping state)
- ✅ Better cache utilization

## Protocol Changes

### NEW: Hash Type Reporting Message

**Problem**: After viewer restart, server needs to know whether viewer has lossless or lossy version of content.

**Solution**: New client→server message reports hash quality:

```cpp
// Message type (add to encodings.h)
const int msgTypePersistentCacheHashType = 253;

// Hash type enum
enum PersistentCacheHashType {
    HASH_TYPE_NONE = 0,      // Don't have this content
    HASH_TYPE_LOSSY = 1,     // Have lossy version (actualHash != canonicalHash)
    HASH_TYPE_LOSSLESS = 2   // Have lossless version (actualHash == canonicalHash)
};

// Message format:
// 1 byte:  message type (253)
// 1 byte:  hash type (0=NONE, 1=LOSSY, 2=LOSSLESS)
// 8 bytes: canonical hash ID
```

**When Sent**:
1. After viewer receives `PersistentCachedRect(canonicalId)`
2. Viewer performs lookup by canonical hash
3. Viewer sends hash type report to server
4. Server updates tracking (knows if viewer has content and quality level)

**Usage Example**:

```cpp
// Viewer receives reference
void handlePersistentCachedRect(canonicalId) {
    entry = cache.findByCanonicalHash(canonicalId);
    
    if (!entry) {
        sendHashTypeReport(canonicalId, HASH_TYPE_NONE);
        sendQuery(canonicalId);  // Request full data
    } else if (entry->actualHash == canonicalId) {
        sendHashTypeReport(canonicalId, HASH_TYPE_LOSSLESS);
        blitPixels(entry->pixels);
    } else {
        sendHashTypeReport(canonicalId, HASH_TYPE_LOSSY);
        blitPixels(entry->pixels);
    }
}
```

**Server Use**:
- Server tracks quality level of viewer's cache
- Can decide whether to upgrade lossy→lossless (future enhancement)
- Prevents sending references when viewer has NONE
- Currently: server accepts any quality level (lossy or lossless)

## Testing Requirements (UPDATED)

### Unit Tests

Must verify:

1. **Dual-hash storage**:
   - `GlobalClientPersistentCache` stores both canonical and actual hash
   - Lookup by canonical hash finds entry even when actual hash differs
   - Direct lookup by actual hash works (fast path)

2. **Persistence**:
   - Lossy entries (`actual != canonical`) persist to disk with `isPersistable=true`
   - After restart, lossy entries can be found by canonical hash
   - Both hashes survive serialization/deserialization

3. **Quality upgrades**:
   - Storing lossless version (`actual == canonical`) creates new entry
   - Both lossy and lossless entries can coexist
   - Lookup prefers lossless (exact hash match) over lossy

4. **Hash type reporting**:
   - Viewer correctly identifies LOSSLESS when `actual == canonical`
   - Viewer correctly identifies LOSSY when `actual != canonical`
   - Viewer correctly reports NONE when entry not found

**Key Unit Tests to Update**:
- `tests/unit/test_lossy_cache.cxx`: Add dual-hash storage tests
- `tests/unit/test_lossy_mapping.cxx`: Update to use canonical-only lookups
- New test: `test_dual_hash_persistence.cxx` - verify cross-session lossy hits

### E2E Tests

Must verify:

1. **Cross-session lossy cache hits** (LibreOffice test):
   - Navigate slides with JPEG encoding (lossy)
   - Store lossy entries with both hashes
   - Revisit slides in same session → lossy cache hits
   - **Restart viewer** → revisit slides → lossy cache hits still work ✅
   - Target: hits per transition ≥ 1.0

2. **Canonical hash lookup works**:
   - Server always sends canonical hash in references
   - Viewer finds lossy entries by canonical hash
   - No server-side lossy mapping state needed

3. **Hash type reporting protocol**:
   - Viewer sends correct hash type after each lookup
   - Server tracks viewer's cache quality
   - Protocol message format correct (1 byte type + 8 byte ID)

4. **Quality upgrades in practice**:
   - Start with lossy encoding (low bandwidth)
   - Switch to lossless encoding (high bandwidth)
   - Verify lossless entries preferred after upgrade

**Key E2E Tests to Update**:
- `tests/e2e/test_libreoffice_slides.py`: Add cross-session test phase
- `tests/e2e/test_cpp_persistentcache.py`: Verify lossy persistence
- All e2e tests: Check for hash type reporting messages in logs

### Test Success Criteria

**Before Implementation** (current state):
- ❌ Lossy entries memory-only, evicted before use
- ❌ LibreOffice test: 0.33 hits per transition
- ❌ Cross-session lossy cache misses

**After Implementation** (target state):
- ✅ Lossy entries persist to disk
- ✅ LibreOffice test: ≥ 1.0 hits per transition
- ✅ Cross-session lossy cache hits work
- ✅ All unit tests pass with dual-hash design
- ✅ All e2e tests pass with new protocol

### Example E2E Test Flow

```python
# Phase 1: Initial session (lossy encoding)
start_viewer()
navigate_slides()  # Stores lossy entries with both hashes
count_hits()       # Expect hits on revisits (intra-session)
stop_viewer()

# Phase 2: New session (verify persistence)
start_viewer()     # Fresh process, loads from disk
navigate_slides()  # Same slides, server sends canonical hashes
count_hits()       # Expect hits (canonical hash lookup finds lossy entries!) ✅

# Phase 3: Quality upgrade (lossless encoding)
set_lossless_encoding()
navigate_slides()  # Stores lossless versions
count_hits()       # Expect hits with higher quality
```

## Implementation Files

### Client Side
- `common/rfb/DecodeManager.cxx`:
  - `storePersistentCachedRect()`: Stores under correct ID
  - `seedCachedRect()`: Handles seed messages
  - `handlePersistentCachedRect()`: Validates and blits cached content

### Server Side
- `common/rfb/VNCSConnectionST.cxx`:
  - `handlePersistentCacheHashReport()`: Updates known set correctly
  - `markPersistentIdKnown()`: Tracks what client can look up

- `common/rfb/EncodeManager.cxx`:
  - `tryPersistentCacheLookup()`: Prefers lossless over lossy
  - Bordered and bbox lookups: Same preference order

### Cache Engine
- `common/rfb/GlobalClientPersistentCache.cxx`:
  - `insert()`: Stores content with quality flag
  - `getByKey()`: Fast lookup by CacheKey

## Edge Cases

### What if server sends canonical ID for lossy content?
- Client will miss (lookup fails)
- Client sends `RequestCachedData`
- Server sends full data with `PersistentCachedRectInit`
- This is why server must track lossy mappings correctly!

### What if client has both lossy and lossless versions?
- They're stored as separate entries (different IDs)
- Server prefers canonical ID in lookups
- Lossy version may eventually be evicted from cache

### What if compression quality changes?
- Different JPEG quality levels produce different lossy IDs
- Each quality level is a separate cache entry
- This is acceptable - prevents visual inconsistency

## Future Enhancements

Potential improvements (not currently needed):

1. **Active quality replacement**: When lossless version arrives, actively evict lossy version
2. **Quality-level tracking**: Store JPEG quality with entry, only reuse same quality
3. **Perceptual hashing**: Use pHash to match similar content despite compression differences

Current implementation is sufficient for all test requirements and real-world usage.

## Related Documentation

- `LOSSY_CACHE_BUG_FIX_2025-12-06.md`: Bug fix implementation details
- `docs/CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: Overall cache design
- `docs/PERSISTENTCACHE_DESIGN.md`: PersistentCache protocol
- `CACHE_FIX_SUMMARY_2025-12-04.md`: Lossy caching enablement
