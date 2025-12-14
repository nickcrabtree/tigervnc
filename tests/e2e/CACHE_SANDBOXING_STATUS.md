# E2E Test Persistent Cache Sandboxing Status

**Date**: 2025-12-13
**Issue**: Tests were using production persistent cache (~/.cache/tigervnc/persistentcache) which corrupts user's real cache with test data.
**Solution**: All tests must use `artifacts.get_sandboxed_cache_dir()` to get a test-specific cache directory.

## Framework Changes

✅ **COMPLETED**: Added `ArtifactManager.get_sandboxed_cache_dir()` helper method in `framework.py`

This method:
- Returns a test-specific cache directory under artifacts (e.g., `_artifacts/20251213_153853/persistent_cache/`)
- Creates the directory if it doesn't exist
- Ensures tests NEVER touch production cache

## Tests Requiring Updates

### ✅ Updated to Use Helper

**All tests now properly sandboxed!**

1. **test_cpp_persistentcache.py** - ✅ Updated
2. **test_cache_parity.py** - ✅ Updated
3. **test_cache_simple_poc.py** - ✅ Updated

### ✅ Already Sandboxed or No PersistentCache

These tests either don't use PersistentCache or already had proper sandboxing:

1. **test_persistent_cache_eviction.py** - ✅ Already OK
2. **test_persistentcache_v3_sharded.py** - ✅ Already OK
3. **test_hash_collision_handling.py** - ✅ Already OK  
4. **test_seed_mechanism.py** - ✅ Already OK
5. **test_large_rect_cache_strategy.py** - ✅ Already OK
6. **test_cpp_cache_back_to_back.py** - ✅ Already OK
7. **test_lossy_lossless_parity.py** - ✅ Already OK
8. **test_large_rect_lossy_first_hit.py** - ✅ Already OK
9. **test_persistent_cache_bandwidth.py** - ✅ Already OK
10. **test_cpp_cache_eviction.py** - ✅ Already OK

### ℹ️ Non-Test Files

- **manual_cache_validation.sh** (line 91) - Manual test script, document that it uses production cache
- **TEST_TRIAGE_FINDINGS.md** - Documentation, no changes needed

## Implementation Pattern

### Before (UNSAFE - uses production cache):
```python
viewer = run_viewer(
    binaries['cpp_viewer'], port, artifacts, tracker,
    'test_viewer', params=['PersistentCache=1']
)
```

### After (SAFE - uses sandboxed cache):
```python
cache_dir = artifacts.get_sandboxed_cache_dir()
viewer = run_viewer(
    binaries['cpp_viewer'], port, artifacts, tracker,
    'test_viewer', params=[
        'PersistentCache=1',
        f'PersistentCachePath={cache_dir}'
    ]
)
```

## Verification Checklist

For each test file:
- [ ] Check if it enables PersistentCache (`PersistentCache=1`)
- [ ] If yes, verify it sets `PersistentCachePath` parameter
- [ ] Confirm it uses `artifacts.get_sandboxed_cache_dir()` or equivalent
- [ ] Test runs successfully without touching ~/.cache/tigervnc/persistentcache

## Priority

**CRITICAL**: All tests must be sandboxed before next release to prevent corrupting user caches.

## Next Steps

1. Update remaining test files to use sandboxed cache helper
2. Run each updated test to verify functionality
3. Document manual_cache_validation.sh behavior (uses production cache intentionally)
4. Consider adding a CI check that fails if tests touch ~/.cache/tigervnc/persistentcache
