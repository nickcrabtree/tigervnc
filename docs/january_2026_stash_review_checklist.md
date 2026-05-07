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

- [x] `cmake/Modules/CMakeMacroLibtoolFile.cmake`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 002 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with incomplete CMake fragments.

- [x] `cmake/StaticBuild.cmake`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 003 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with incomplete CMake fragments.

- [x] `common/rfb/DecodeManager.cxx`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 004 showed divergent stale cache code with malformed/incomplete DecodeManager fragments; keep current HEAD and do not merge this stash version.

- [x] `common/rfb/GlobalClientPersistentCache.h`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 005 showed stale/malformed header fragments in the stash and only tiny comment-level HEAD-to-stash differences; keep current HEAD.

- [x] `docs/CACHE_IMPROVEMENTS_2025-12-05.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: merge / preserve
  - Notes: Review 006 merged the Jan 2026 rectangle-stability documentation addendum from the stash.

- [x] `docs/LOG_DRIVEN_CACHE_TRACE_TEST_PLAN.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: merge / preserve
  - Notes: Review 007 merged the Jan 2026 current-status and scan-logging documentation update from the stash.

- [x] `docs/SUBREGION_CACHE_DESIGN_UPDATED.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: merge / preserve
  - Notes: Review 008 merged the Jan 2026 current-status and completion-plan documentation update from the stash.

- [x] `docs/content_and_persistent_cache_tiling_enhancement.md`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: merge / preserve
  - Notes: Review 009 merged the Jan 2026 current-status and completion-plan documentation update from the stash.

- [x] `java/CMakeLists.txt`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 010 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with incomplete Java CMake fragments.

- [x] `tests/e2e/capture_slide_screenshots.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 011 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with incomplete Python fragments.

- [x] `tests/e2e/comparator.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 012 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with broken Python fragments.

- [x] `tests/e2e/dark_rect_detector.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 013 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with broken dark-rectangle detector fragments.

- [x] `tests/e2e/framework.py`
  - Class: `DIVERGED_NEEDS_REVIEW`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 014 showed divergent stale framework changes that would remove current sudo and viewer-wrapper support; keep current HEAD.

- [x] `tests/e2e/run_baseline_rfb_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 015 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with broken baseline RFB test fragments.

- [x] `tests/e2e/run_black_box_screenshot_test.py`
  - Class: `SEMANTIC_STASH_NOT_IN_HEAD`
  - Stash status: `M`
  - Decision: reject / do not merge
  - Notes: Review 016 showed HEAD unchanged from stash base ignoring whitespace; stash content is stale formatter fallout with broken black-box screenshot test fragments.

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
