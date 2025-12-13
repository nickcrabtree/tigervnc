# Cache Design Clarification - December 12, 2025

## Current Design: Always Seed + Lossy Hash Reporting

### Overview

The cache system is designed to enable **first-occurrence caching** for both lossless and lossy content. This provides the fastest user experience by immediately caching lossy-encoded content (which displays quickly) while the system lazily refreshes to lossless quality with idle bandwidth.

### Key Principles

1. **Seeds are ALWAYS sent** - Both lossy and lossless encodings send seed messages with the canonical (lossless) hash
2. **Client detects hash mismatches** - For lossy content, the decoded pixels produce a different hash
3. **Client stores under actual hash** - Lossy content is stored under the lossy hash, not rejected
4. **Client reports back** - Client sends message 247 (PersistentCacheHashReport) with canonical→lossy mapping
5. **Server learns mapping** - Server can use this for future dual-hash lookups

### Protocol Flow

```
Server                                    Client
  │                                         │
  ├─► Encode subrects (JPEG/lossy) ───────►│
  │                                         │
  ├─► CachedRectSeed(canonical_hash) ─────►│
  │                                         │
  │                                         ├─ Decode pixels
  │                                         ├─ Compute hash of decoded pixels
  │                                         ├─ Detect: decoded_hash ≠ canonical_hash
  │                                         ├─ Store under decoded_hash (lossy)
  │                                         │
  │◄── PersistentCacheHashReport ──────────┤
  │     (canonical_hash, lossy_hash)       │
  │                                         │
  ├─ Store mapping                          │
  │  canonical → lossy                      │
```

### Benefits of This Design

1. **Fast initial display**: Client immediately caches lossy content for quick display
2. **No network overhead for seed rejection**: Client doesn't need to request data again
3. **Lazy lossless refresh**: System can refresh to lossless quality during idle time
4. **First-occurrence caching**: Cache hits on first appearance of content, not second
5. **Server learning**: Server builds canonical→lossy mappings for future sessions

### What This Means for Testing

Tests should validate:
- ✅ Seeds are sent for BOTH lossy and lossless encodings
- ✅ Lossless encodings: NO hash reports (hash matches exactly)
- ✅ Lossy encodings: Hash reports ARE sent (message 247)
- ✅ Both encodings achieve good hit rates
- ✅ No visual corruption

Tests should NOT expect:
- ❌ Seeds to be skipped for lossy encodings
- ❌ Hash mismatches to be treated as corruption
- ❌ Client to reject lossy content

### Historical Note

An earlier design (documented in CACHE_IMPROVEMENTS_2025-12-05.md) proposed skipping seeds for lossy encodings. This was superseded by the lossy hash reporting protocol (LOSSY_HASH_REPORTING_PROTOCOL.md) which enables better performance through immediate caching of lossy content.

### Implementation Files

**Protocol Definition:**
- `common/rfb/msgTypes.h`: Message 247 definition
- `common/rfb/CMsgWriter.cxx`: Client writes message 247
- `common/rfb/SMsgReader.cxx`: Server reads message 247

**Seed Mechanism:**
- `common/rfb/EncodeManager.cxx`: Lines 1407-1416 (bounding box seeds)
- `common/rfb/EncodeManager.cxx`: Lines 1422-1446 (bordered region seeds)
- Seeds sent with canonical hash for ALL encodings

**Client-Side Detection:**
- `common/rfb/DecodeManager.cxx`: Lines 1137-1162 (hash mismatch detection)
- Stores under lossy hash (line 1169)
- Reports via message 247 (lines 1156-1161)

**Server-Side Learning:**
- `common/rfb/VNCSConnectionST.cxx`: Lines 965-977 (handlePersistentCacheHashReport)
- Stores canonical→lossy mapping via cacheLossyHash()

### Test Files

Updated to match current design:
- `tests/e2e/test_seed_mechanism.py`: Validates seed sending and hash reporting
- `tests/e2e/test_lossy_lossless_parity.py`: Validates parity between encodings
- `tests/e2e/test_hash_collision_handling.py`: Validates collision handling (hash mismatch ≠ corruption)

### Summary

**The current design intentionally allows lossy content in the cache because:**
1. It provides a faster user experience (immediate display)
2. The system can refresh to lossless during idle bandwidth
3. It enables first-occurrence caching instead of second-occurrence
4. It reduces network overhead by avoiding seed rejection/re-request cycles

This is a feature, not a bug.
