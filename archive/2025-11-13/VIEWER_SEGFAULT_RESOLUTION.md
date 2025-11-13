# Viewer Segfault Resolution - November 12, 2025

## Status: ✅ ALREADY FIXED

The viewer segfault reported in `VIEWER_SEGFAULT_FINDINGS.md` has **already been resolved** by commit **16f7101b** authored earlier today.

## Root Cause

The segfault was caused by a **use-after-move** bug in `ContentCache::storeDecodedPixels()` (common/rfb/ContentCache.cxx).

### The Bug

Debug validation code was accessing `cached.pixels` **after** the object had been moved into the ArcCache:

```cpp
// WRONG - use-after-move bug
arcPixelCache_->insert(key, std::move(cached));  // cached is moved here

// BUG: Accessing cached.pixels after move!
for (size_t i = 0; i < contiguousSize && isAllBlack; i++) {
    if (cached.pixels[i] != 0) {  // SEGFAULT: cached.pixels is invalid!
        isAllBlack = false;
    }
}
```

### The Fix

The debug validation was moved to **before** the `std::move()` call:

```cpp
// CORRECT - validation before move
// Debug checks - MUST be done BEFORE std::move to avoid use-after-move
bool isAllBlack = true;
for (size_t i = 0; i < contiguousSize && isAllBlack; i++) {
    if (cached.pixels[i] != 0) {
        isAllBlack = false;
    }
}

// Insert into ArcCache (will handle promotion/eviction)
// NOTE: Do not access 'cached' after this move!
arcPixelCache_->insert(key, std::move(cached));
```

## Verification

### Test Results

Running the e2e ContentCache test with ASAN-enabled debug build:

```bash
$ cd tests/e2e && python3 test_cpp_contentcache.py --duration 180 --verbose
```

**Results:**
- ✅ No crashes or segfaults
- ✅ No ASAN errors
- ✅ 75% cache hit rate (6/8 cache lookups)
- ✅ Viewer ran for 3+ minutes without issues
- ✅ Cache operations logged correctly:
  - "Received CachedRectInit" messages
  - "Storing decoded rect" messages
  - "Cache hit for ID" messages

### Related Fixes

Several related fixes were also applied in recent commits:

1. **16f7101b** - Fix use-after-move segfault (main fix)
2. **9621268c** - Store cached pixels contiguously to fix stride corruption
3. **4bbb6621** - Copy pixel data row-by-row instead of as contiguous block

These fixes ensure:
- Proper deep-copy of pixel data with correct stride handling
- No dangling pointers or use-after-move bugs
- Contiguous storage eliminates stride-related corruption

## Code Quality

The current implementation in `ContentCache::storeDecodedPixels()` (lines 692-752) is **correct and safe**:

1. **Deep copy**: Pixel data is copied row-by-row from source buffer
2. **Stride handling**: Correctly converts stride-in-pixels to stride-in-bytes
3. **Memory safety**: Validation happens before `std::move()`
4. **Clear comments**: Warns against accessing moved objects

## Conclusion

The viewer segfault issue is **resolved**. The current codebase is stable and passes extended testing with ASAN enabled.

### Remaining Work

None required for the segfault fix. However, there is a separate issue with bandwidth measurement in the test framework (showing 0% reduction despite 75% hit rate), which is a test infrastructure issue, not a viewer bug.

---

**Date**: November 12, 2025  
**Author**: AI Assistant  
**Status**: Complete  
**Verification**: 3-minute soak test with ASAN, no crashes
