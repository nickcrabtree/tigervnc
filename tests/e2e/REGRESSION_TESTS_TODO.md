# Regression Tests Implementation Guide

## Status

✅ **ALL 5 TESTS IMPLEMENTED**

1. ✅ test_lossy_lossless_parity.py (379 lines)
2. ✅ test_minimal_corruption_lossy.py (45 lines)
3. ✅ test_large_rect_cache_strategy.py (312 lines)
4. ✅ test_seed_mechanism.py (357 lines)
5. ✅ test_hash_collision_handling.py (313 lines)

## Test Descriptions

### 1. test_large_rect_cache_strategy.py (HIGH PRIORITY)

**Purpose**: Validate large rectangle caching strategies (bordered regions, bounding box, tiling)

**Test Approach**:
```python
# 1. Generate large content (1024x768+ images or fullscreen)
# 2. Run with PersistentCache enabled
# 3. Parse server logs for:
#    - "BORDERED:" messages (detecting bordered regions)
#    - "TILING:" messages (large rect subdivision)
#    - Bounding box cache attempts
# 4. Verify cache hits occur for large rects (not just small tiles)
# 5. Assert: bordered_count > 0 OR tiling_count > 0 (large rect logic exercised)
```

**Key Assertions**:
- Large content triggers bounding box or bordered logic
- Server logs contain "BORDERED:" or "TILING:" messages
- Hit rate > 20% for large content
- No visual corruption (compare screenshots)

**Implementation Notes**:
- Use `scenarios_static.random_fullscreen_colors()` or `image_burst(size=640)`
- Parse logs with: `grep -E "BORDERED:|TILING:|bbox" server.log`
- Compare with small-tile scenario to prove different code path

---

### 2. test_seed_mechanism.py (HIGH PRIORITY)

**Purpose**: Explicitly verify seed mechanism respects encoding lossyness

**Test Approach**:
```python
# Run 1: Lossless (ZRLE)
#   - Parse server log for "writeCachedRectSeed" messages
#   - Assert: seed_count > 0
#   - Assert: seed_skip_count == 0

# Run 2: Lossy (Tight)
#   - Parse server log for "Skipped seeding (lossy encoding)" 
#   - Assert: seed_skip_count > 0
#   - Assert: regular seed messages == 0 for lossy rects

# Both runs: Cache still works (hit rate > 20%)
```

**Key Assertions**:
- Lossless: Seeds sent, no seed skips
- Lossy: Seeds skipped (logged), cache works via INITs
- Both: Hit rates > 20%

**Implementation Notes**:
- Similar structure to test_lossy_lossless_parity.py
- Key difference: Focus on seed messages in server logs
- Parse for both positive (seeds sent) and negative (seeds skipped) cases

---

### 3. test_minimal_corruption_lossy.py (HIGH PRIORITY)

**Purpose**: Enhance existing corruption test to cover lossy encoding

**Test Approach**:
```python
# Wrapper around test_minimal_corruption.py
# Adds lossy encoding variants:
#   1. Both viewers with ZRLE (lossless) - existing test
#   2. Both viewers with Tight (lossy) - NEW
#   3. One ZRLE, one Tight (mixed) - NEW
# 
# All must produce identical screenshots (no visual corruption)
```

**Key Assertions**:
- Screenshots identical in all 3 scenarios
- No pixel-level differences
- Test FAILS if any corruption detected

**Implementation Notes**:
- Extend run_black_box_screenshot_test.py with encoding parameters
- OR: Create variants calling test_minimal_corruption.py with PreferredEncoding args
- Most critical test - any failure indicates visual corruption bug

---

### 4. test_hash_collision_handling.py (MEDIUM PRIORITY)

**Purpose**: Verify hash collisions don't cause corruption (edge case)

**Test Approach**:
```python
# This is a synthetic edge case test - requires special content generation
# 
# Option A: Mutation-based
#   1. Generate content A, cache it (hash X)
#   2. Mutate a few pixels to create content B with same hash X (hard!)
#   3. Verify B is not served with A's cached data
#
# Option B: Force collision in test environment
#   1. Monkey-patch ContentHash::computeRect to return fixed hash
#   2. Cache rect A with hash 12345
#   3. Request rect B, also gets hash 12345
#   4. Verify visual output shows B, not A
#
# Option C: Statistical
#   1. Cache 10,000+ unique small rects
#   2. Compute collision rate (should be ~0 with 64-bit hashes)
#   3. Assert: no collisions OR visual corruption if collision
```

**Key Assertions**:
- No visual corruption even with synthetic collisions
- Cache validation prevents wrong data being used
- Collision detection/recovery works

**Implementation Notes**:
- Most complex test to implement correctly
- Consider SKIP for now and implement later if time permits
- Real-world collision rate with 64-bit hashes is ~0

---

## Implementation Priority

1. ✅ test_lossy_lossless_parity.py - DONE
2. ✅ test_minimal_corruption_lossy.py - DONE
3. ✅ test_large_rect_cache_strategy.py - DONE
4. ✅ test_seed_mechanism.py - DONE
5. ✅ test_hash_collision_handling.py - DONE

## Quick Implementation: test_minimal_corruption_lossy.py

```python
#!/usr/bin/env python3
"""Corruption test with lossy encoding (Tight/JPEG)."""

import os
import sys
from pathlib import Path

def main() -> int:
    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"
    
    argv = [
        sys.executable,
        str(runner),
        "--mode", "none",                  # Both viewers: caches OFF
        "--duration", "15",
        "--checkpoints", "1",
        "--viewer1-encoding", "Tight",     # LOSSY encoding
        "--viewer2-encoding", "Tight",     # LOSSY encoding
        *sys.argv[1:],
    ]
    os.execv(sys.executable, argv)

if __name__ == "__main__":
    raise SystemExit(main())
```

**Note**: Requires extending run_black_box_screenshot_test.py to accept encoding parameters.

## Testing Checklist

All tests implemented:

- ✅ test_lossy_lossless_parity.py - Validates lossy/lossless behavior differences
- ✅ test_minimal_corruption_lossy.py - No corruption with lossy encoding
- ✅ test_large_rect_cache_strategy.py - Detects bordered/tiling logic
- ✅ test_seed_mechanism.py - Validates seed prevention for lossy
- ✅ test_hash_collision_handling.py - Edge case collision handling

Run all tests with:
```bash
python3 tests/e2e/test_lossy_lossless_parity.py
python3 tests/e2e/test_minimal_corruption_lossy.py
python3 tests/e2e/test_large_rect_cache_strategy.py
python3 tests/e2e/test_seed_mechanism.py
python3 tests/e2e/test_hash_collision_handling.py
```

## Current Test Coverage Summary

**✅ Well Covered:**
- Basic PersistentCache protocol
- ContentCache protocol
- Eviction handling
- Bandwidth reduction
- Visual corruption (lossless only)

**⚠️ Gaps Addressed by These Tests:**
- Lossy vs lossless behavior differences
- Large rectangle strategies
- Seed mechanism validation
- Hash collision edge cases
- Visual corruption with lossy encoding
