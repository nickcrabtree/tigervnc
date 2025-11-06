# Headless Display Capture Feature Design

## Overview

Implement an optional headless display mode that captures the actual rendered output (not just the internal framebuffer) to detect display rendering bugs.

## Problem Statement

Current verification (`kill -USR1`) compares internal framebuffer state but cannot detect bugs in the display rendering path (Viewport::draw() and platform-specific Surface implementations). When users see corruption that's fixed by a refresh but verification passes, the bug is in the rendering code, not the framebuffer.

## Requirements

1. **Platform-agnostic**: Works on Linux, macOS, and Windows
2. **Optional**: Can be enabled at compile time or runtime
3. **Non-invasive**: Doesn't affect normal operation when disabled
4. **Capture actual rendering**: Intercepts what would be drawn to screen
5. **PNG export**: Save captured display to file for inspection
6. **Signal-triggered**: Use SIGUSR2 to dump current display state

## Architecture

### 1. New Surface Implementation

Create `vncviewer/Surface_Headless.cxx`:

```cpp
class Surface_Headless : public Surface {
  private:
    std::vector<uint8_t> displayBuffer_;
    int width_, height_, stride_;
    rfb::PixelFormat format_;
    
  public:
    // Implement Surface interface
    void draw(Surface* dst, int src_x, int src_y, int x, int y, int w, int h) override;
    void draw(const uint8_t* data, int x, int y, int w, int h, int stride) override;
    
    // Capture API
    void saveToPNG(const char* filename);
    const uint8_t* getDisplayBuffer() const { return displayBuffer_.data(); }
};
```

### 2. Build System Integration

Add CMake option:

```cmake
option(ENABLE_HEADLESS_CAPTURE "Enable headless display capture for testing" OFF)

if(ENABLE_HEADLESS_CAPTURE)
  target_compile_definitions(njcvncviewer PRIVATE HEADLESS_CAPTURE_ENABLED)
  target_sources(njcvncviewer PRIVATE Surface_Headless.cxx)
  target_link_libraries(njcvncviewer png)
endif()
```

### 3. Runtime Toggle

Add parameter to enable at runtime:

```cpp
BoolParameter headlessCapture("HeadlessCapture",
  "Enable headless display capture mode", false);
```

### 4. Signal Handler

Add SIGUSR2 handler to dump display:

```cpp
#ifndef WIN32
static void handleDumpDisplaySignal(int sig)
{
  (void)sig;
  g_dumpDisplayRequested = 1;
}
#endif

// In CConn constructor:
#ifndef WIN32
#ifdef HEADLESS_CAPTURE_ENABLED
  signal(SIGUSR2, handleDumpDisplaySignal);
  vlog.info("Display capture available: kill -USR2 %d", (int)getpid());
#endif
#endif
```

### 5. PNG Export

Use libpng to write display buffer:

```cpp
void Surface_Headless::saveToPNG(const char* filename)
{
  FILE* fp = fopen(filename, "wb");
  if (!fp) return;
  
  png_structp png = png_create_write_struct(PNG_LIBPNG_VER_STRING, nullptr, nullptr, nullptr);
  png_infop info = png_create_info_struct(png);
  
  png_init_io(png, fp);
  png_set_IHDR(png, info, width_, height_, 8, PNG_COLOR_TYPE_RGBA,
               PNG_INTERLACE_NONE, PNG_COMPRESSION_TYPE_DEFAULT,
               PNG_FILTER_TYPE_DEFAULT);
  png_write_info(png, info);
  
  // Write rows
  for (int y = 0; y < height_; y++) {
    png_write_row(png, &displayBuffer_[y * stride_]);
  }
  
  png_write_end(png, nullptr);
  png_destroy_write_struct(&png, &info);
  fclose(fp);
}
```

## Implementation Plan

### Phase 1: Basic Infrastructure (2-3 hours)

1. Create `Surface_Headless.cxx` with basic draw() methods
2. Add CMake option and conditional compilation
3. Store rendered pixels in memory buffer
4. Add debug logging to verify captures are happening

### Phase 2: PNG Export (1-2 hours)

1. Add libpng dependency to CMake
2. Implement `saveToPNG()` method
3. Handle pixel format conversion if needed
4. Add error handling and logging

### Phase 3: Signal Integration (1 hour)

1. Add SIGUSR2 handler
2. Implement dump logic in socket event loop
3. Generate timestamped PNG filenames
4. Add logging to report where PNG was saved

### Phase 4: Testing (1-2 hours)

1. Test with various window sizes
2. Verify PNG output is correct
3. Test with both normal and headless modes
4. Document usage in README or WARP.md

## Usage

```bash
# Build with headless capture support
cmake -DENABLE_HEADLESS_CAPTURE=ON ..
make viewer

# Run viewer
./build/vncviewer/njcvncviewer server:1

# In another terminal, trigger display dump
kill -USR2 <pid>

# Check output
ls -l /tmp/vncviewer_display_*.png
open /tmp/vncviewer_display_20251106_093000_12345.png
```

## Benefits

1. **Detect rendering bugs**: Can see exactly what user sees
2. **Automated testing**: Capture and compare screenshots in CI
3. **Debugging aid**: Visual proof of corruption for bug reports
4. **Platform-independent**: Same code works everywhere
5. **Regression testing**: Compare PNG outputs across versions

## Alternative: Screenshot via Platform APIs

Instead of custom Surface, could use platform-specific screenshot APIs:

**Pros**: Captures actual screen output, including window decorations
**Cons**: Platform-specific, requires more complex code, may have permissions issues

**Decision**: Stick with custom Surface approach for platform independence and simplicity.

## Related Work

- Current verification (SIGUSR1): Compares internal framebuffer
- ContentCache debugging: Logs cache operations
- This feature: Captures actual rendered output

Together, these three tools provide complete debugging coverage:
- Framebuffer verification: Checks server/client framebuffer match
- Rendering capture: Checks framebuffer → display path
- Cache debugging: Checks cache operations

## Open Questions

1. Should we capture cursor in the PNG? (Probably yes for completeness)
2. Should we support continuous capture mode for animations? (Future enhancement)
3. Should we add comparison mode to diff two PNGs? (Use external tools)
4. Should we add automatic capture on detected corruption? (Requires corruption detection)

## Dependencies

- libpng (or stb_image_write.h for header-only alternative)
- FLTK (already required)
- pthreads (already required)

## Implementation Priority

**Medium-High**: Very useful for debugging display issues, but the stride fix (commit 9621268c) may have already resolved most corruption. Worth implementing after confirming whether display rendering bugs still exist.

## See Also

- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: ContentCache architecture
- `tests/e2e/README.md`: End-to-end testing framework
- Commit 21bded2b: Framebuffer verification implementation
