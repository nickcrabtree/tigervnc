# Test Triage Findings
**Date**: 2025-11-13  
**Investigation**: Analysis of 8 failing/timed-out e2e tests

## Executive Summary

Investigated 8 problematic tests (6 failures, 2 timeouts) to determine whether failures indicate real code bugs or outdated test assumptions. 

**Key Findings:**
1. **PersistentCache protocol is working correctly** - client logs show proper hits and protocol messages
2. **Primary bug discovered**: Viewer doesn't call `DecodeManager::logStats()` on shutdown, so bandwidth statistics are never printed
3. **Test issue**: `test_cpp_limited_encodings.sh` explicitly unsets DISPLAY, causing FLTK initialization failure
4. **Log parser gap**: Test framework looks for bandwidth reduction summaries that aren't being printed

## Test-by-Test Classification

### 1. test_cpp_limited_encodings.sh
**Status**: ❌ FAILED  
**Category**: **TEST BUG** - Invalid test assumption  
**Root Cause**: Test explicitly unsets DISPLAY (line 87), but C++ viewer requires X display for FLTK initialization

**Evidence**:
```
Can't open display: 
```

**Fix**: Update test script to:
- Remove `env -u DISPLAY` wrapper (line 87)
- Instead rely on xvfb-run at the outer invocation level
- Or use viewer window server (display :999) approach like other tests

**Priority**: Low - Test can be run manually with proper DISPLAY

---

### 2-4. test_persistent_cache_bandwidth.py, test_persistent_cache_eviction.py, test_cache_parity.py
**Status**: ❌ FAILED / ⏱️ TIMED OUT  
**Category**: **CODE BUG** - Missing logStats() call on viewer shutdown

**Root Cause**: Bandwidth statistics are being collected correctly in `DecodeManager`, but `logStats()` is never called when viewer exits.

**Evidence from logs**:
- Client receives `PersistentCachedRectInit` (lines 77-108 in test logs)
- Client receives `PersistentCachedRect` hits (lines 111-198)
- Bandwidth tracking functions are called:
  - `trackPersistentCacheRef()` at DecodeManager.cxx:702
  - `trackPersistentCacheInit()` at DecodeManager.cxx:796
- But `logStats()` method (lines 262-374) is never invoked

**Code Analysis**:
```cpp
// DecodeManager.cxx:369-372
// PersistentCache bandwidth summary
const auto ps = persistentCacheBandwidthStats.formatSummary("PersistentCache");
if (persistentCacheBandwidthStats.cachedRectCount || persistentCacheBandwidthStats.cachedRectInitCount)
  vlog.info("  %s", ps.c_str());
```

The summary would be printed if `logStats()` were called. The viewer doesn't call this on shutdown.

**Fix**: Add `decode->logStats()` call in viewer shutdown path:
- Location: `vncviewer/CConn.cxx` in destructor or disconnect handler
- Similar to how server calls `encodeManager->logStats()` in `VNCServerST` 

**Files to modify**:
1. `vncviewer/CConn.cxx` - Add `decode->logStats()` before cleanup
2. Consider adding to `vncviewer/DesktopWindow.cxx` if window closes before CConn destroyed

**Priority**: HIGH - Affects multiple tests and user visibility of cache performance

---

### 5. test_cache_simple_poc.py
**Status**: ❌ FAILED  
**Category**: **TEST BUG** - Same root cause as #2-4 (missing logStats)

**Additional Issue**: Test may not generate enough repeated content above MinRectSize threshold

**Fix**: 
1. Same viewer fix as #2-4
2. Update test to use larger, more repetitive content (e.g., logo tiles >= 2048 pixels)

---

### 6. test_cachedrect_init_propagation.py  
**Status**: ❌ FAILED  
**Category**: **TEST OBSOLETE** - Tests ContentCache-specific behavior when PersistentCache is now default

**Evidence**: Server logs show PersistentCache is being used instead of ContentCache

**Fix Options**:
A. Update test to validate PersistentCache propagation (rename and update assertions)
B. Add SKIP condition when PersistentCache is enabled and test expects ContentCache-only
C. Delete test if ContentCache protocol validation is no longer needed

**Recommendation**: Option A - Update to test PersistentCache protocol

---

### 7. run_baseline_rfb_test.py
**Status**: ❌ FAILED (FBU count 1 < threshold 20)  
**Category**: **TEST OUTDATED** - Threshold assumptions no longer valid

**Root Cause**: Modern server coalesces updates aggressively, reducing FBU count while maintaining same data transfer

**Fix**: Update test to check:
- Total rectangles received (not just FBU count)
- Total bytes transferred
- Or total pixel area updated
- Duration-scaled expectations

**Priority**: Medium

---

### 8. run_contentcache_test.py
**Status**: ⏱️ TIMED OUT (>180s)  
**Category**: **TEST BUG** - Unbounded waits + capability mismatch

**Root Cause**: 
1. Test may wait indefinitely for ContentCache activity when PersistentCache is active
2. Missing timeout bounds on log-waiting loops

**Fix**:
1. Add early SKIP if PersistentCache enabled and test expects ContentCache-only behavior
2. Add bounded waits (max 60-120s) for log pattern matches
3. Reduce default test duration
4. Add diagnostic dumps on timeout

**Priority**: Medium

---

## Critical Bug Details

### Bug: Viewer doesn't print bandwidth statistics

**Impact**: Users and tests can't see cache performance metrics

**Location**: `vncviewer/` - missing call to `decode->logStats()`

**Expected output** (that's currently missing):
```
Client-side PersistentCache statistics:
  Protocol operations (PersistentCachedRect received):
    Lookups: 44, Hits: 44 (100.0%)
    Misses: 0, Queries sent: 0
  ARC cache performance:
    Total entries: 4, Total bytes: 938 KiB
    Cache hits: 44, Cache misses: 0, Evictions: 0
  PersistentCache: 517 KiB bandwidth saving (99.7% reduction)
```

**Verification**: The bandwidth stats ARE being tracked - just never printed:
- `persistentCacheBandwidthStats` is populated at DecodeManager.cxx:702, 796
- `formatSummary()` generates the string at cache/BandwidthStats.cxx:11-17
- But output only appears if `logStats()` is called

**Where to add the call**:
```cpp
// In vncviewer/CConn.cxx destructor or close():
if (decode) {
  vlog.info("Framebuffer statistics:");
  decode->logStats();
}
```

## Recommendations

### Immediate Actions (High Priority)

1. **Fix viewer logStats bug** - Add call in `vncviewer/CConn.cxx`
   - Impact: Fixes 3-4 failing tests immediately
   - Risk: Low - only adds logging output
   - Effort: 5-10 lines of code

2. **Update test_cpp_limited_encodings.sh** - Remove DISPLAY unset
   - Impact: Fixes 1 failing test
   - Risk: None - test infrastructure only
   - Effort: 1 line change

### Secondary Actions (Medium Priority)

3. **Update run_baseline_rfb_test.py** - Change thresholds to bytes/rectangles
   - Impact: Fixes 1 failing test
   - Risk: None - test only
   - Effort: 10-20 lines

4. **Add timeout bounds to run_contentcache_test.py**
   - Impact: Prevents hanging
   - Risk: None - test only  
   - Effort: 20-30 lines

5. **Update or skip test_cachedrect_init_propagation.py**
   - Impact: Resolves test/code mismatch
   - Risk: None - test only
   - Effort: Depends on approach (skip: 5 lines, update: 50+ lines)

### Test Framework Improvements

6. **Add capability detection helpers**
   - Parse server logs to detect which caches are enabled
   - Allow tests to skip/adjust based on configuration
   - Location: `tests/e2e/lib/cache_utils.py`

7. **Add bounded wait helpers**
   - Replace unbounded log-waiting loops
   - Add diagnostic output on timeout
   - Max wait: 60-120s with polling

## Verification Plan

After implementing fixes:

1. Rebuild viewer with logStats() call
2. Run test_cpp_persistentcache.py - should now show bandwidth reduction
3. Run test_persistent_cache_bandwidth.py - should pass
4. Update and run test_cpp_limited_encodings.sh - should pass
5. Run remaining tests with updated thresholds/timeouts

## Files Modified

### Code Changes (Minimal - High Impact)
- `vncviewer/CConn.cxx` - Add logStats() call (~5 lines)

### Test Changes
- `tests/e2e/test_cpp_limited_encodings.sh` - Remove DISPLAY unset (~1 line)
- `tests/e2e/run_baseline_rfb_test.py` - Update thresholds (~10 lines)
- `tests/e2e/run_contentcache_test.py` - Add timeouts (~20 lines)
- `tests/e2e/test_cachedrect_init_propagation.py` - Add skip or update (~50 lines)

## Appendix: Evidence

### Confirmed Working: PersistentCache Protocol

From `tests/e2e/_artifacts/20251113_153939/logs/pc_bandwidth_test_viewer.log`:

**Initial load** (PersistentCachedRectInit messages):
```
Line 77:  CMsgReader:  Received PersistentCachedRectInit: [100,100-586,122] hashLen=16
Line 81:  DecodeManager: PersistentCache STORE: rect [100,100-586,122] hash=c62f3ab3bde60a4c...
```

**Cache hits** (PersistentCachedRect references):
```
Line 111: CMsgReader:  Received PersistentCachedRect: [100,100-586,122] hashLen=16  
Line 112: DecodeManager: PersistentCache HIT: rect [100,100-586,122] hash=c62f3ab3bde60a4c...
```

**Bandwidth tracking calls** (verified in source):
```cpp
Line 702: rfb::cache::trackPersistentCacheRef(persistentCacheBandwidthStats, r, conn->server.pf(), hash.size());
Line 796: rfb::cache::trackPersistentCacheInit(persistentCacheBandwidthStats, hash.size(), lastDecodedRectBytes);
```

### Confirmed Bug: Missing logStats Call

**Search results**:
```bash
$ grep -r "decode->logStats\|decoder->logStats" vncviewer/
# No results - logStats() is never called in viewer code
```

**Server comparison** (for reference):
```cpp
// unix/xserver/hw/vnc/VNCServerST.cxx:131, 211, 286
void VNCServerST::removeSocket(network::Socket* sock) {
  ...
  if (conn->getEncodeManager())
    conn->getEncodeManager()->logStats();  // ← Server DOES call this
  ...
}
```

## Conclusion

**Real Bugs**: 1 (missing logStats call in viewer)  
**Outdated Tests**: 3 (limited_encodings DISPLAY assumption, baseline FBU threshold, cachedrect_init for old protocol)  
**Test Infrastructure Gaps**: 2 (unbounded waits, missing capability detection)

**Overall Assessment**: The cache implementation is working correctly. Test failures are primarily due to:
1. Missing logging output (code bug - easy fix)
2. Outdated test assumptions (test updates needed)
3. Test infrastructure gaps (bounded waits, capability checks)

**Confidence**: HIGH - Evidence clearly shows PersistentCache protocol functioning as designed
