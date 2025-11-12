# PersistentCache Session Tracking Bug - 2025-11-12

## Summary

PersistentCache is not tracking which hashes have been sent to the client **during the current session**, resulting in 0% hit rate even when the same content appears multiple times within a single connection.

## Expected Behavior

**Within a single session:**
1. Server encodes rectangle with hash X → Sends full data to client → **Tracks that client now has hash X**
2. Server encodes another rectangle with same hash X → **Recognizes client has hash X** → Sends reference instead

**Across sessions:**
1. Client loads hash inventory from disk on startup
2. Client sends inventory to server: "I have hashes A, B, C, D..."
3. Server uses this inventory + session tracking to know what client has

## Actual Behavior

**Within session:**
- Server sends hash X to client (first logo)
- Server sees hash X again (second identical logo) 
- Server checks: "Does client have hash X?" → **NO** (not in initial inventory)
- Server sends full data again ❌

Result: **0% hit rate** within session, all misses.

## Evidence from Test Run

From `test_cpp_persistentcache.py` with tiled logos (12 identical logos):

```
PersistentCache Performance:
  Cache lookups: 16
  Cache hits:    0 (0.0%)  ← Should be ~50-66% like ContentCache
  Cache misses:  16
```

Server log shows all misses:
```
EncodeManager: PersistentCache MISS: rect [100,100-167,191] - client doesn't
EncodeManager: PersistentCache MISS: rect [100,540-167,631] - client doesn't
...
```

Even though both rectangles contain the **same 67×91 logo image** (same pixel data, same hash), the second one is still a MISS.

## Root Cause

PersistentCache server-side implementation is **not maintaining a session-scoped set of "hashes sent to this client"**.

Compare with ContentCache which correctly tracks this:
- ContentCache: 50% hit rate (4 hits, 4 misses) ✓
- PersistentCache: 0% hit rate (0 hits, 16 misses) ✗

## Code Location

Need to check:
- `common/rfb/EncodeManager.cxx` - PersistentCache lookup logic
- `common/rfb/VNCSConnectionST.cxx` - Connection-scoped hash tracking
- `common/rfb/PersistentCache.cxx` - Cache state management

The bug is likely in `tryPersistentCacheLookup()` which only checks:
1. Initial hash inventory from disk
2. But NOT hashes sent during current session

## Fix Needed

Add **session-scoped hash tracking** to PersistentCache:

```cpp
// In VNCSConnectionST or similar:
std::unordered_set<HashValue> hashesKnownByClient;  // Session-scoped!

// When sending PersistentCachedRectInit (full data with hash):
hashesKnownByClient.insert(hash);

// When checking if client has hash:
bool clientHasHash(hash) {
    return hashesKnownByClient.contains(hash);  // Check session state!
}
```

This is exactly how ContentCache works with its cache ID tracking.

## Test Case

The tiled logos test is a perfect validation:
- 12 logos displayed
- Only ~4 unique images (due to how display windows overlap)
- Should see 4 misses (initial sends) + 8 hits (references)
- Currently seeing 16 misses (broken)

## Impact

**Severe**: PersistentCache provides NO bandwidth benefit within a session, only across reconnections. This defeats much of its purpose for applications that show repeated content (like UI elements, logos, icons, toolbar buttons, etc).

## Related Issues

This explains why previous tests showed 0% hit rate. It wasn't a test problem - it's a real implementation bug where PersistentCache fundamentally doesn't work correctly within a single session.

## Next Steps

1. Find where ContentCache tracks "known cache IDs" per connection
2. Implement equivalent "known hashes" tracking for PersistentCache
3. Re-run test - should see ~50-66% hit rate like ContentCache
4. Verify cross-session persistence still works
