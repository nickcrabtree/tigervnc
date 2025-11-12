# Static Content Scenarios for Reliable Cache Testing

## Purpose

Traditional cache tests using `xterm` windows have unreliable hit rates because:
- Terminal rendering varies based on timing
- Font rendering creates slight pixel variations
- Window manager decorations can vary
- Command output changes (timestamps, etc.)

**Static scenarios** solve this by generating **identical, reproducible pixel patterns** using simple X11 primitives.

## Implementation

File: `scenarios_static.py`

### Classes

#### `StaticPatternGenerator`
Creates static bitmap patterns using ImageMagick `convert`.

**Patterns**:
- `checkerboard`: 32x32 checkerboard tiling
- `gradient`: Black-to-white gradient
- `text`: Fixed text on white background
- `solid`: Solid color (not recommended for cache testing)

#### `StaticScenarioRunner`
Executes test scenarios with guaranteed identical content.

### Available Scenarios

#### 1. `repeated_static_content()` ✅ **Recommended**

**Strategy**:
1. Create 3 static pattern images (checkerboard, gradient, text)
2. Display each pattern at fixed position
3. Close window
4. Display **same** pattern at **same** position
5. Repeat → Guaranteed identical pixels → Cache hit

**Advantages**:
- Guaranteed byte-for-byte identical content
- Fixed positions ensure identical rectangle boundaries
- Textured patterns (not solid colors) require caching

**Usage**:
```python
runner = StaticScenarioRunner(display, verbose=True)
stats = runner.repeated_static_content(duration_sec=60)
```

**Expected Result**: 
- Server: High cache hit rate (> 70%)
- Client: Receives `CachedRect` messages (references)

---

#### 2. `solid_color_test()` ❌ **Not Recommended**

**Strategy**:
- Cycles through solid colors using `xsetroot`
- Very simple, but...

**Problem**: 
- Solid colors detected by encoder
- Encoded as 12-byte solid rectangles
- **Bypasses cache entirely** (too small, too efficient)

**Usage**:
```python
runner = StaticScenarioRunner(display, verbose=True)
stats = runner.solid_color_test(cycles=15)
```

**Expected Result**: 
- 0 cache lookups (solid encoder used instead)

---

#### 3. `moving_window_test()` 🔄 **Advanced**

**Strategy**:
- Create ONE static window
- Move it to different positions
- Tests cache behavior with same content at different locations

**Purpose**:
- Tests if cache can handle position changes
- With current implementation (dimensions in hash), each position creates new cache entry

**Usage**:
```python
runner = StaticScenarioRunner(display, verbose=True)
stats = runner.moving_window_test(cycles=10)
```

**Expected Result**: 
- Cache misses (different positions = different rectangles)
- Useful for testing cache memory management

---

## Test Integration

### ContentCache Test

**File**: `test_cpp_contentcache.py`

**Updated to use**:
```python
from scenarios_static import StaticScenarioRunner

runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
stats = runner.repeated_static_content(duration_sec=args.duration)
```

**Before** (xterm):
- Unpredictable hit rates (0-30%)
- Content varies due to timing
- Rectangle subdivision inconsistent

**After** (static patterns):
- Predictable hit rates (> 70% expected)
- Identical content guaranteed
- Consistent rectangle boundaries

---

## How It Works

### Pattern Creation

```bash
# Checkerboard (640x480)
convert -size 640x480 pattern:checkerboard output.png

# Gradient
convert -size 640x480 gradient: output.png

# Text on white
convert -size 640x480 xc:white -pointsize 14 \
    -annotate +10+10 'Cache Test Pattern' output.png
```

### Pattern Display

```bash
# Display at fixed position (100,100)
display -title "static" -geometry +100+100 pattern.png &
```

### Repetition

1. Display pattern A at position (100,100)
2. Wait for VNC encoding/transmission
3. Close window
4. Display **same pattern** at **same position**
5. VNC server sees identical content → Cache hit
6. Server sends `CachedRect` (20 bytes) instead of full encoding (KB)

---

## Why This Guarantees Cache Hits

### Identical Content
- Same PNG file used repeatedly
- ImageMagick produces deterministic output
- No timing-dependent variations

### Identical Rectangle Boundaries
- Fixed window positions
- Consistent window manager decorations
- Server divides framebuffer identically

### Cacheable Size
- 640×480 = 307,200 pixels >> 4096 threshold
- Textured patterns require Tight/ZRLE encoding
- Non-trivial data size worth caching

---

## Requirements

**ImageMagick** (for pattern generation):
```bash
sudo apt-get install imagemagick
```

**X11 Display Tools**:
- `display` (ImageMagick)
- `xsetroot` (X11 apps)
- `wmctrl` (optional, for moving windows)

---

## Troubleshooting

### "convert: not found"
Install ImageMagick:
```bash
sudo apt-get install imagemagick
```

### "display: not found"
Same as above (ImageMagick provides both).

### Still getting 0% hit rates

Check server log for:
```bash
grep "ContentCache insert:" logs/server_*.log
grep "ContentCache protocol hit:" logs/server_*.log
```

**Possible causes**:
1. **Solid color detection**: Encoder bypasses cache for solid colors
2. **Below threshold**: Rectangles < 4096 pixels not cached
3. **Different subdivision**: Server dividing framebuffer differently each time

**Solution**: Use `repeated_static_content()` with large textured patterns.

---

## Performance Expectations

### With Static Patterns

**Server side** (check logs):
```
ContentCache: Hit rate: 75.0% (9 hits, 3 misses, 12 total)
```

**Client side** (check logs):
```
CMsgReader: Received CachedRect: [100,100-740,580] cacheId=1
CMsgReader: Received CachedRect: [100,100-740,580] cacheId=2
...
```

**Bandwidth**:
- First display: ~50-100 KB (full Tight/ZRLE encoding)
- Subsequent displays: 20 bytes (CachedRect reference)
- Reduction: > 99% for repeated content

---

## Future Enhancements

### Potential Improvements

1. **XCB/Xlib direct drawing**: Create patterns without ImageMagick dependency
2. **Custom window manager**: Guarantee exact window placement
3. **Raw X11 rectangles**: Draw directly to root window for perfect control
4. **Timing control**: Synchronize with VNC frame updates

### Alternative Approaches

1. **Screenshot replay**: Capture once, replay exactly
2. **Synthetic framebuffer**: Generate pixels programmatically
3. **X11 recording**: Record X11 protocol events, replay

---

## Related Files

- `scenarios_static.py` - Static scenario implementation
- `test_cpp_contentcache.py` - ContentCache test using static scenarios
- `test_cpp_persistentcache.py` - PersistentCache test (can use same scenarios)
- `scenarios.py` - Original dynamic scenarios (xterm-based)

---

## Conclusion

Static scenarios provide **reliable, reproducible cache hit testing** by:

✅ Generating **identical pixel patterns**  
✅ Using **fixed positions** for consistent rectangles  
✅ Creating **textured content** that benefits from caching  
✅ Eliminating **timing-dependent variations**  

**Recommendation**: Use `repeated_static_content()` for all cache hit rate validation.

---

**Created**: November 12, 2025  
**Author**: Development Session  
**Status**: Implemented and Documented
