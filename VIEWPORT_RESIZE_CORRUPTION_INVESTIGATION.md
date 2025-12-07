# Viewport Resize Corruption Investigation

**Date**: December 6, 2025  
**Test**: `tests/e2e/test_dark_rect_corruption.py`  
**Bug**: Dark rectangle corruption appears after viewport resize at bottom-right boundary

## Problem Statement

When VNC viewer windows are resized (e.g., from 800×860 to 1600×1760), a dark rectangular artifact appears in the bottom-right region near the old boundary. The corruption manifests as:
- **Location**: x=[797-812], y=[859-881] (272 pixels, 0.01%)
- **Appearance**: Dark gray pixels (~27,27,27) instead of expected white/light gray
- **Persistence**: Corruption remains across subsequent checkpoints
- **Initial severity**: 1132 pixels (0.04%) before fixes

## Root Causes Identified

### 1. Bottom Strip Width Bug (FIXED)
**File**: `common/rfb/CConnection.cxx:171`

When resizing framebuffer and blacking out newly exposed areas, the bottom strip calculation used the NEW width instead of OLD width:

```cpp
// WRONG - blacks out [0, 860-1600, 1760], overwriting valid copied pixels!
rect.setXYWH(0, framebuffer->height(),
             fb->width(),  // ❌ Uses new width
             fb->height() - framebuffer->height());

// CORRECT - blacks out [0, 860-800, 1760], only new area
rect.setXYWH(0, framebuffer->height(),
             framebuffer->width(),  // ✅ Uses old width
             fb->height() - framebuffer->height());
```

**Impact**: This caused valid pixels in the overlap region (x<800, y<860) to be incorrectly blacked out.

**Fix**: Changed line 171 to use `framebuffer->width()` instead of `fb->width()`.

### 2. Uninitialized Pixmap Contents
**File**: `vncviewer/PlatformPixelBuffer.cxx:69`, `vncviewer/Viewport.cxx:428`

On X11, `XCreatePixmap()` creates a Pixmap with uninitialized contents (random garbage). The `PlatformPixelBuffer` constructor calls `clear(0,0,0)` to black it out, but this only marks the XImage as damaged - the Pixmap isn't updated until `getDamage()` is called.

**Problem**: If the Pixmap is rendered before the initial `getDamage()` completes, it shows garbage.

**Fix**: Added immediate `getDamage()` call after creating new framebuffer in `Viewport::resize()`:

```cpp
PlatformPixelBuffer* newFrameBuffer = new PlatformPixelBuffer(w, h);
assert(newFrameBuffer);

// Sync the initial clear(0,0,0) from the constructor to the Pixmap
// immediately so the Pixmap doesn't contain uninitialized garbage.
newFrameBuffer->getDamage();
```

### 3. Missing XSync for Non-SHM XPutImage
**File**: `vncviewer/PlatformPixelBuffer.cxx:127-129`

The `getDamage()` method uses `XPutImage()` or `XShmPutImage()` to copy damaged regions from XImage to Pixmap. For shared memory, it correctly calls `XSync()` to wait for completion. For non-shared memory, it was missing:

```cpp
} else {
    XPutImage(fl_display, pixmap, gc, xim,
              r.tl.x, r.tl.y, r.tl.x, r.tl.y, r.width(), r.height());
    // ❌ Missing XSync - commands may not complete before return
}
```

**Fix**: Added `XSync(fl_display, False)` after `XPutImage()` to ensure commands complete.

## Progress

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Corrupted pixels | 1132 | 272 | 76% reduction |
| Affected checkpoints | 4, 5, 6 | 4, 5, 6 | - |
| Corruption percentage | 0.04% | 0.01% | 75% reduction |

## Remaining Issue

**272 pixels still differ** between two identical viewers (both with `--mode none`). This shouldn't happen and indicates a remaining bug.

### Key Observations

1. **Framebuffer shows correct values**: Logs show pixel (797,859) = (254,254,254,0) WHITE in framebuffer
2. **Screenshots show wrong values**: Both screenshots show (27,27,27) DARK GRAY at same location
3. **Consistent corruption**: Same 272 pixels across checkpoints 4, 5, 6
4. **Both viewers affected**: Not a single-viewer bug, but may differ slightly between viewers

### Hypotheses for Remaining Corruption

#### Hypothesis 1: Server Content Size Mismatch
The VNC server's desktop content stays at 1600×900 while viewers resize to 1600×1760. The newly exposed regions [800,0-1600,1760] and [0,860-800,1760] may show:
- Desktop background color
- Old window content
- Unrefreshed areas

**Evidence**:
- Decode statistics show only 6 raw rects with 368 pixels received (exactly the corruption region size!)
- Server may not be sending fresh content for newly exposed areas
- The region contains varying pixel values suggesting actual content, not just black

**Next steps**:
- Check what the VNC server's desktop actually contains at (797-812, 859-881)
- Verify if `SetDesktopSize` is being sent and received
- Check if server sends updates for newly exposed regions

#### Hypothesis 2: Race Condition in Damage Handling
Even with `XSync()` calls, there may be a race between:
1. `getDamage()` completing the Pixmap update
2. FLTK's event loop triggering a redraw
3. Screenshot capture happening

**Evidence**:
- The two viewers show slightly different pixel values (ground_truth varies, cache uniform)
- Timing-sensitive behavior

**Next steps**:
- Add forced `Fl::flush()` or `Fl::wait(0)` after resize to ensure rendering completes
- Add delay between resize and screenshot capture (test harness already has 12.9s, may not be enough for X11 flush)

#### Hypothesis 3: Incomplete Damage Region
The damage region returned by `getDamage()` might not cover the full affected area. Check if:
- `setFramebuffer()` marks all affected regions as damaged
- The damage union is computed correctly
- Multiple damage regions are being properly merged

**Next steps**:
- Add logging to show exactly which regions are marked as damaged in `commitBufferRW()`
- Verify the damage region union logic in `pixman::Region`
- Check if `fillRect()` (blacking) and `imageRect()` (copying) both call `commitBufferRW()`

## Files Modified

### Core Fixes
- `common/rfb/CConnection.cxx:171` - Fixed bottom strip width calculation
- `vncviewer/Viewport.cxx:428` - Added immediate getDamage() after new framebuffer creation
- `vncviewer/PlatformPixelBuffer.cxx:129` - Added XSync() for non-SHM XPutImage

### Debug Logging (Can be removed after fix)
- `common/rfb/CConnection.cxx:110-176` - Added detailed setFramebuffer logging
- `vncviewer/Viewport.cxx:412-476` - Added detailed resize logging

## Testing

Run the TDD test:
```bash
cd tests/e2e
python3 test_dark_rect_corruption.py --mode none --lossless
```

Expected behavior after full fix:
- All checkpoints should show "OK (identical)"
- Zero pixels should differ between ground_truth and cache viewers

Current behavior:
- Checkpoints 1-3: OK (before resize)
- Checkpoints 4-6: 272 pixels differ (after resize)

## Next Steps (Prioritized)

### 1. Investigate Server Content (HIGH PRIORITY)
The most likely cause is that the viewers are correctly displaying what the server sent, but the server's content at those coordinates differs between the two viewer connections or over time.

**Actions**:
```bash
# Check what server actually has at corruption coordinates
ssh nickc@birdsurvey.hopto.org
DISPLAY=:998 xwd -root | convert - -crop 16x23+797+859 corruption_region.png
DISPLAY=:998 import -window root -crop 16x23+797+859 corruption_region.png
```

Check server logs for framebuffer update requests and responses around the resize time.

### 2. Add Forced Rendering Flush (MEDIUM PRIORITY)
Ensure all X11 rendering completes before screenshots:

```cpp
// In Viewport::resize() after getDamage()
#if !defined(WIN32) && !defined(__APPLE__)
    // Force FLTK to process all pending events and complete rendering
    Fl::flush();
    Fl::wait(0);
    XSync(fl_display, False);
#endif
```

### 3. Verify Damage Region Coverage (MEDIUM PRIORITY)
Add comprehensive logging in `PlatformPixelBuffer::commitBufferRW()`:

```cpp
void PlatformPixelBuffer::commitBufferRW(const core::Rect& r)
{
    FullFramePixelBuffer::commitBufferRW(r);
    mutex.lock();
    vlog.info("commitBufferRW: marking [%d,%d-%d,%d] as damaged",
              r.tl.x, r.tl.y, r.br.x, r.br.y);
    damage.assign_union(r);
    core::Rect current_damage = damage.get_bounding_rect();
    vlog.info("  accumulated damage now: [%d,%d-%d,%d]",
              current_damage.tl.x, current_damage.tl.y, 
              current_damage.br.x, current_damage.br.y);
    mutex.unlock();
}
```

### 4. Test with Remote Server (LOW PRIORITY)
The test uses `SetDesktopSize` which requires server support. Verify the server (Xnjcvnc on :998) properly handles desktop resize:

```bash
# Check server capabilities
grep -i "setdesktopsize" tests/e2e/_artifacts/*/logs/bb_content_server_998.log
grep -i "desktop size" tests/e2e/_artifacts/*/logs/bb_content_server_998.log
```

### 5. Simplify Test Case (RESEARCH)
Create a minimal reproduction:
- Static image instead of browser
- Single viewer instead of two
- Manual resize instead of automated
- Known pixel pattern in corruption region

## Technical Details

### Coordinate System Notes
- VNC uses inclusive top-left, exclusive bottom-right: `Rect(0,0,800,860)` = pixels [0-799, 0-859]
- Stride is in PIXELS not BYTES (must multiply by bytesPerPixel for byte offsets)
- Widget coordinates vs framebuffer coordinates differ by `Fl_Widget::x()` and `Fl_Widget::y()`

### X11 Rendering Pipeline
1. Drawing operations modify XImage buffer (`xim->data`)
2. `commitBufferRW()` marks regions as damaged (in-memory tracking)
3. `getDamage()` copies damaged regions from XImage to Pixmap via `XPutImage()`
4. FLTK's `draw()` renders Pixmap to window via `XRenderComposite()`

### Resize Sequence
1. Window manager resizes window (or test harness calls `xdotool`)
2. FLTK calls `Viewport::resize(x, y, w, h)`
3. If dimensions changed: create new framebuffer, copy old content, black new areas
4. Send `SetDesktopSize` to server (if enabled and supported)

## Update 2025-12-07: Additional Instrumentation and Current Status

Since the initial version of this document, further debugging has been done on the NetWeaver browser resize scenario in test_dark_rect_corruption.py (mode "none", lossless):

- Added detailed damage logging in vncviewer/PlatformPixelBuffer.cxx (commitBufferRW and getDamage) to track which rectangles are flushed to the Pixmap.
- Added Surface::debugSampleRect in vncviewer/Surface.h and Surface_X11.cxx to sample pixels directly from the offscreen Pixmap at the corruption region, gated by TIGERVNC_DEBUG_SAMPLE_REGION.
- Extended vncviewer/DesktopWindow.cxx to sample pixels from the actual window (via XGetImage on fl_xid(this)) at the same coordinates as the screenshots, also gated by TIGERVNC_DEBUG_SAMPLE_REGION.

Key findings from these runs:

1. Damage coverage is correct after the resize. The accumulated damage rectangle in PlatformPixelBuffer::commitBufferRW/getDamage grows to cover the full framebuffer (0,0–800,860) for both viewers, and includes the corruption bbox (797,859–812,881).
2. Offscreen Pixmap contents match between viewers. Surface::debugSampleRect reports identical dark-grey values in that region for both the ground_truth and cache viewers (BGRA 0x20,0x20,0x20,0x00 in the sampled pixels).
3. Window pixels match between viewers at sample times. DesktopWindow's window sampling shows that the top-level window pixels at the same coordinates settle to a stable dark grey (0x202020) in both viewers after the resize.
4. Despite 2 and 3, checkpoint screenshots still differ. In the e2e artifacts, the cache viewer's screenshot region is uniformly dark grey, while the ground_truth screenshot region shows a mixture of greys/whites (58 distinct colors) in the same bbox, leading to the remaining 272-pixel mismatch at checkpoints 4–6.

Conclusions and updated hypotheses:

- The framebuffer, damage tracking, offscreen Pixmap, and window contents appear consistent between the two viewers at the times we have sampled. This makes a pure viewer-side rendering bug in that region less likely.
- The remaining discrepancy now looks more like a screenshot harness issue (timing or coordinate alignment) than a core rendering pipeline bug:
  - The two screenshots may be captured at slightly different times relative to when the windows have fully repainted after resize.
  - Or the screenshot and the viewer's notion of coordinates may be misaligned (e.g. window decorations vs client area), so the bbox reported by the diff does not correspond to the same client pixels in both images.

Immediate next steps based on this newer evidence:

1. Correlate sampling with checkpoint capture time. Arrange for the harness to send an explicit signal or marker before each checkpoint screenshot so both viewers can log or sample the corruption region at the exact moment of capture. This will confirm whether the window pixels truly differ at checkpoint time or if the discrepancy arises elsewhere in the pipeline.
2. Validate coordinate alignment. Double-check that the bbox [797,859–812,881] in the screenshots corresponds to the same client-area pixels in both viewers, taking into account any window manager decorations or offsets. If needed, adjust either the sampling coordinates in DesktopWindow or the way the harness identifies the window region for xwd/convert.
3. Only after 1 and 2 are resolved, revisit earlier hypotheses (server content vs viewer rendering) if evidence shows a real pixel difference on-screen at checkpoint time.
5. Request framebuffer updates for newly exposed regions
6. Server sends updates (may take time, may send black if no content)

## References

- Test: `tests/e2e/test_dark_rect_corruption.py`
- Test framework: `tests/e2e/run_black_box_screenshot_test.py`
- WARP.md context: Search for "Stride is in Pixels, Not Bytes!" section
- Related: ContentCache stride bug from Oct 7, 2025 (similar pixel vs bytes confusion)

## Conclusion

Significant progress made with 76% reduction in corruption. Three real bugs fixed:
1. ✅ Bottom strip width calculation
2. ✅ Uninitialized Pixmap contents
3. ✅ Missing XSync for non-SHM rendering

Remaining 272 pixels likely due to server content mismatch or rendering timing issue. Investigation should focus on what the server is actually sending for those coordinates and whether both viewers are receiving identical updates.
