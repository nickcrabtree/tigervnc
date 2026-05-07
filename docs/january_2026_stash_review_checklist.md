# January 2026 Stash Review Checklist

Source stash: `stash@{2026-01-27 07:53:18 +0000}`.

## Classification summary

- `DIVERGED_NEEDS_REVIEW 7`
- `FORMAT_ONLY_IN_STASH 478`
- `SEMANTIC_STASH_NOT_IN_HEAD 37`
- `STASH_SUBSUMED_BY_HEAD 245`

## File-by-file review

Tick an item only after the decision and notes are updated.

- [x] `CMakeLists.txt`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 001 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with incomplete CMake fragments.

- [ ] `cmake/Modules/CMakeMacroLibtoolFile.cmake`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `cmake/StaticBuild.cmake`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `common/rfb/DecodeManager.cxx`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `common/rfb/GlobalClientPersistentCache.h`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `docs/CACHE_IMPROVEMENTS_2025-12-05.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `docs/LOG_DRIVEN_CACHE_TRACE_TEST_PLAN.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `docs/SUBREGION_CACHE_DESIGN_UPDATED.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `docs/content_and_persistent_cache_tiling_enhancement.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `java/CMakeLists.txt`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/capture_slide_screenshots.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/comparator.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/dark_rect_detector.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/framework.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/run_baseline_rfb_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/run_black_box_screenshot_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/run_contentcache_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/run_refresh_stability_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/run_resize_latency_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/sandbox_all_tests.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/scenarios.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/scenarios_static.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/screenshot_compare.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cache_parity.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cache_simple_poc.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cachedrect_init_propagation.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cpp_cache_eviction.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cpp_contentcache.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cpp_no_caches.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_cpp_persistentcache.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_dark_rect_corruption.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_hash_collision_handling.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_large_rect_cache_strategy.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_large_rect_lossy_first_hit.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_libreoffice_slides.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_lossless_refresh_zrle.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_lossy_lossless_parity.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_persistent_cache_bandwidth.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_persistent_cache_eviction.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_persistentcache_v3_sharded.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_screenshot_compare_real_corruption_regression.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_seed_mechanism.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `tests/e2e/test_toggle_pictures.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending

- [ ] `vncviewer/CMakeLists.txt`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: pending
  - Notes: pending
