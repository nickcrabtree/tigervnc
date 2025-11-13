# ContentCache E2E Test - 0% Hit Rate Root Cause Analysis

**Test Run:** 2025-11-13 09:16:24  
**Test:** `test_cpp_contentcache.py`  
**Result:** 0% cache hit rate (0 lookups, 0 hits)

## Executive Summary

The ContentCache e2e test achieves **0% hit rate** because all rectangles are **below the minimum cache size threshold** (4096 pixels). The 48×48 pixel logos (2304 pixels) used in the test are subdivided into even smaller fragments during encoding, all of which are rejected by the cache.

## Root Cause

### 1. **Logo Size Too Small**
- Test uses: `tigervnc_64.png` (actually 48×48 pixels = 2304 pixels)
- ContentCache minimum: `Server::contentCacheMinRectSize = 4096` pixels (equivalent to 64×64)
- **Result: Logo is 57% too small** to be cached

### 2. **Rectangle Subdivision**
The server's encoding pipeline further subdivides rectangles:
- Original logo regions are split into smaller sub-rectangles
- Examples from logs:
  - `(1246,981 50×21)` = 1050 pixels
  - `(1819,981 51×21)` = 1071 pixels  
  - `(1819,1002 51×38)` = 1938 pixels
- All fragments are **well below the 4096 pixel threshold**

### 3. **Debug Logging Confirms**
From server logs (`cpp_cc_content_server_995.log`):
```
EncodeManager: CC writeSubRect: rect (1246,981 50x21) area=1050 passMin=no
EncodeManager: CC attempt ContentCache lookup for rect (1246,981 50x21)
EncodeManager: CC SKIP: below minSize=4096 rect (1246,981 50x21) area=1050
```

Every single rectangle is rejected with `CC SKIP: below minSize=4096`.

## Test Statistics

- **Framebuffer updates:** 28
- **Total rectangles encoded:** 91
  - Solid: 17 rects
  - Bitmap RLE: 9 rects
  - Indexed RLE: 1 rect
  - Tight JPEG: 64 rects
- **ContentCache lookups:** 0 (all rejected before lookup)
- **Cache insertions:** 0
- **Hit rate:** 0.0%

## Why This Matters

The ContentCache protocol is designed for **larger repeated UI elements**:
- Window decorations (title bars, borders)
- Toolbar buttons and icons
- Application logos in headers
- Repeated widgets

The 48×48 logo test case is simply too small to exercise the cache.

## Solutions

### Option 1: Use Larger Test Images (Recommended)
- Create or use logos ≥ 64×64 pixels (4096+ pixels)
- Ideal size: 128×128 (16,384 pixels) or larger
- This matches real-world ContentCache use cases

### Option 2: Lower the Cache Threshold for Testing
- Temporarily reduce `Server::contentCacheMinRectSize` to 2000 pixels
- Allows 48×48 logos to be cached
- **Caveat:** Not representative of production behavior

### Option 3: Use Different Test Scenario
- Test with repeated window title bars (typically 200×24 = 4800 pixels)
- Test with repeated toolbar buttons
- Test with application windows containing logos/branding

## Additional Findings

### Positive: Debug Logging Works Perfectly
The new CC-prefixed debug logging successfully reveals:
- ✅ Update boundaries: `CC doUpdate begin`
- ✅ Rectangle subdivision: `CC rect no-split`, `CC subrect`
- ✅ Per-rectangle decisions: `CC writeSubRect`
- ✅ Cache lookup attempts: `CC attempt ContentCache lookup`
- ✅ Skip reasons: `CC SKIP: below minSize`
- ✅ Client capabilities: `clientCC=yes clientPC=no`

### Protocol Negotiation Success
- Client advertised ContentCache support: ✅
- Client disabled PersistentCache as requested: ✅
- Server disabled PersistentCache: ✅ (`EnablePersistentCache=0`)

### Server Build Success
- Local `Xnjcvnc` server compiled successfully after disabling ASan
- Server correctly initialized ContentCache (2048MB, unlimited age)
- All 28 framebuffer updates processed without crashes

## Recommendations

1. **Immediate:** Create 128×128 or 256×256 test logos for ContentCache tests
2. **Document:** Update test documentation to explain minimum image sizes
3. **Validate:** Rerun test with larger logos to confirm cache hits occur
4. **Baseline:** Establish expected hit rates for different test scenarios

## Log Artifacts

- Server log: `tests/e2e/_artifacts/20251113_091624/logs/cpp_cc_content_server_995.log`
- Viewer log: `tests/e2e/_artifacts/20251113_091624/logs/cpp_cc_test_viewer.log`
- Full test output: `/tmp/cc_test_full.log`

## Conclusion

The 0% hit rate is **expected behavior** given the test setup. The ContentCache implementation is working correctly - it's simply rejecting rectangles that are too small, which is the intended design for performance reasons.

To properly test ContentCache functionality, we need test content that meets the minimum size threshold of 4096 pixels (64×64 or larger).
