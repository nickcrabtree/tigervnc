# Picture Toggle E2E Test

## Overview

The `test_toggle_pictures.py` test validates the **tiling enhancement** described in `docs/content_and_persistent_cache_tiling_enhancement.md` by measuring **cache hits per toggle** when switching between two large pictures.

## The Key Metric: Hits Per Toggle

### Without Tiling Enhancement (Current Baseline)

Today, when a large rectangle is sent:
1. Server decomposes it into many small tiles (64×64 or similar)
2. Each tile is cached individually
3. On re-display, **many** cache hits occur (one per tile)
4. Result: **dozens of hits per toggle**

### With Tiling Enhancement (Goal)

With the tiling enhancement:
1. Server hashes the **entire** large rectangle first
2. Server asks client: "Do you have this large rectangle?"
3. First time: Client says "no" → server sends pixels normally, then tells client to store the hash
4. Next time: Client says "yes" → **exactly 1 cache hit** for entire rectangle
5. Result: **1 hit per toggle**

## Test Flow

```
1. Display pictureA (HYDRATION)
   └─ Server: hash entire rectangle
   └─ Client: "no, I don't have it"
   └─ Server: sends pixels (may be decomposed into sub-rectangles)
   └─ Server: "you now have hash X for this rectangle"
   └─ Cache hits during hydration are ignored

2. Display pictureB (HYDRATION)
   └─ Same as above for pictureB

3. Toggle back to pictureA (POST-HYDRATION)
   └─ Server: hash entire rectangle → same hash as step 1
   └─ Client: "yes, I have it"
   └─ Expected: EXACTLY 1 cache hit

4. Toggle to pictureB (POST-HYDRATION)
   └─ Server: hash entire rectangle → same hash as step 2
   └─ Client: "yes, I have it"
   └─ Expected: EXACTLY 1 cache hit

5. Continue toggling...
   └─ Each toggle: EXACTLY 1 cache hit expected
```

## Test Materials

The test uses two picture fixtures:
- `pictureA.png`: 1034×800 pixels, 517 KB (827,200 pixels)
- `pictureB.png`: 1034×800 pixels, 621 KB (635,536 bytes)

Both pictures are well above the 2048 pixel minimum threshold for ContentCache.

## Running the Test

### Basic Usage

```bash
cd /home/nickc/code/tigervnc/tests/e2e
python3 test_toggle_pictures.py
```

### Common Parameters

```bash
# Increase number of toggles (default: 10)
python3 test_toggle_pictures.py --toggles 20

# Adjust delay between toggles (default: 2.0 seconds)
python3 test_toggle_pictures.py --delay 3.0

# Increase cache size (default: 256 MB)
python3 test_toggle_pictures.py --cache-size 512

# Adjust expected hits per toggle (default: 1.0)
python3 test_toggle_pictures.py --expected-hits-per-toggle 1.0

# Adjust tolerance for hits per toggle (default: 0.5)
python3 test_toggle_pictures.py --hits-tolerance 0.5

# Change hydration toggles (default: 2)
python3 test_toggle_pictures.py --hydration-toggles 3

# Verbose scenario output
python3 test_toggle_pictures.py --verbose
```

### Custom Displays and Ports

The test uses two isolated displays by default:
- Display :998 for VNC server with content
- Display :999 for VNC viewer window

To use different displays:
```bash
python3 test_toggle_pictures.py \
  --display-content 997 --port-content 6897 \
  --display-viewer 996 --port-viewer 6896
```

## What Gets Measured

### Primary Metric: Hits Per Toggle

- **Hits per toggle** = Total cache hits / Total toggles
- **Target**: 1.0 (with tiling enhancement)
- **Current baseline**: Many hits per toggle (without enhancement)

### Supporting Metrics

- **Total lookups**: Number of times cache was consulted
- **Cache hits**: Successful cache lookups
- **Cache misses**: Failed cache lookups
- **Hit rate**: Percentage of hits out of total lookups
- **Bandwidth reduction**: Percentage saved by cache

## Interpreting Results

### Success Criteria

A test is considered successful if:

1. ✓ Hits per toggle is within tolerance (default: 1.0 ± 0.5)
2. ✓ PersistentCache remains disabled (viewer was configured with `PersistentCache=0`)
3. ✓ Viewer didn't crash during the test

### Expected Results

**Without Tiling Enhancement (current):**
- Hits per toggle: **>> 1** (many small tile hits)
- Test will PASS but show a WARNING
- This is expected baseline behavior

**With Tiling Enhancement (goal):**
- Hits per toggle: **~1** (one large rectangle hit)
- Test will PASS without warnings
- This is the target behavior

### Common Failure Modes

| Failure | Cause | Solution |
|---------|-------|----------|
| Hits per toggle too low (<0.5) | Cache not working at all | Check ContentCache negotiation in logs |
| Hits per toggle too high (>1.5) | Tiling enhancement not active (WARNING) | Expected until enhancement is implemented |
| PersistentCache active | Viewer ignoring PersistentCache=0 | Check viewer configuration logic |
| Viewer crash | Segfault or resource exhaustion | Check VNC viewer and server logs |

## Log Files

After the test completes, logs are available in the artifacts directory:

- `toggle_test_viewer.log`: C++ viewer ContentCache protocol messages and cache statistics
- `toggle_content_server_998.log`: Server-side cache operations and encoding stats
- `toggle_viewerwin_server_999.log`: Viewer window server (minimal logging)

## Advanced Tuning

### Testing Different Cache Sizes

```bash
# Small cache (prone to eviction)
python3 test_toggle_pictures.py --cache-size 32

# Large cache (less eviction)
python3 test_toggle_pictures.py --cache-size 1024
```

### Testing with Different Toggle Frequencies

```bash
# Slow toggles (allow pipeline settle time)
python3 test_toggle_pictures.py --delay 5.0 --toggles 20

# Fast toggles (stress test pipeline)
python3 test_toggle_pictures.py --delay 1.0 --toggles 5
```

## Integration with Tiling Enhancement

This test is the **primary validation** for the tiling enhancement described in `docs/content_and_persistent_cache_tiling_enhancement.md`.

### How Tiling Enhancement Works

1. **Pre-decomposition hash**: Server hashes the entire large rectangle BEFORE decomposing
2. **Client query**: Server asks client "do you have this hash?"
3. **On miss**: Server decomposes and sends pixels normally, then tells client to store the hash
4. **On hit**: Client says "yes" → 1 CachedRect message for entire rectangle

### Expected Results

| Metric | Without Enhancement | With Enhancement |
|--------|---------------------|------------------|
| Hits per toggle | Many (10+) | Exactly 1 |
| Bandwidth per toggle | Low (many small refs) | Minimal (1 ref) |
| Messages per toggle | Many CachedRect | 1 CachedRect |

### Test Behavior

- **Before enhancement**: Test PASSES with WARNING about high hits-per-toggle
- **After enhancement**: Test PASSES without warnings
- **If cache broken**: Test FAILS due to low hits-per-toggle

## See Also

- `docs/content_and_persistent_cache_tiling_enhancement.md` - Tiling enhancement design
- `test_cpp_contentcache.py` - General ContentCache test with logos
- `test_cpp_persistentcache.py` - PersistentCache (cross-session) test
- `scenarios_static.py` - Scenario framework and StaticScenarioRunner
