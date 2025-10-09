# Next Steps - Rust VNC Viewer Development

**Current Phase**: Phase 3 - Encodings (Tasks 3.1-3.3 COMPLETE ✅)  
**Last Updated**: 2025-10-08 16:18 Local

---

## 🎯 IMMEDIATE NEXT STEP

**Task 3.6: Tight Encoding Decoder** - `rfb-encodings/src/tight.rs`

Phases 1 & 2 complete! Tasks 3.1-3.5 done! Now implement Tight encoding (JPEG/zlib compression).

---

## ✅ PHASE 1 COMPLETE!

**Phase 1: Core Types** - rfb-pixelbuffer crate  
**Status**: All tasks complete (1.1-1.6) ✅  
**LOC Written**: 1,416 lines (code + docs + tests)  
**Tests**: 37 passing (19 unit + 18 doctests)  
**Time Taken**: ~3 hours (estimated 4 hours)  
**Commits**: c54a69e7, f3e58499, d0da5f2c

---

## ✅ PHASE 2 COMPLETE!

**Phase 2: Network & Protocol Layer** - rfb-protocol crate  
**Status**: All tasks complete (2.1-2.5) ✅  
**LOC Written**: 3,502 lines (206% of 1,700 target - comprehensive!)  
**Tests**: 118 passing (56 unit + 24 messages + 38 doctests)  
**Time Taken**: ~2.5 hours (estimated 2 weeks - way ahead!)  
**Commits**: 231e4370, f407506c, 2a4758f0, b1b6e088, 15658cbb

### Phase 2 Highlights
- [x] **Task 2.1**: Socket abstractions (TCP, Unix domain)
- [x] **Task 2.2**: RFB I/O streams (buffered read/write)  
- [x] **Task 2.3**: Connection state machine
- [x] **Task 2.4**: RFB message types (all server/client messages)
- [x] **Task 2.5**: Protocol handshake (RFB 3.3/3.8, security, init)

---

## 🚀 PHASE 3: Encodings (rfb-encodings crate) - IN PROGRESS!

**Target**: Implement VNC encoding/decoding for framebuffer updates  
**Estimated Time**: 4 weeks  
**Estimated LOC**: ~3,500  
**Crate**: `rfb-encodings`

### Overview

Phase 3 implements the various encoding schemes VNC uses to efficiently transmit screen updates from server to client. Each encoding provides different tradeoffs between compression ratio, CPU usage, and visual quality.

### ✅ Task 3.1: Crate Setup & Decoder Trait (COMPLETE)

**Files**: `rfb-encodings/src/lib.rs`, `rfb-encodings/Cargo.toml`

**What to implement**:
```rust
pub trait Decoder {
    /// Returns the encoding type this decoder handles
    fn encoding_type(&self) -> i32;
    
    /// Decode a rectangle from the input stream into the pixel buffer
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()>;
}
```

**Dependencies**:
- rfb-common (workspace)
- rfb-pixelbuffer (workspace)
- rfb-protocol (workspace - for Rectangle, PixelFormat)
- anyhow, tokio

**Target LOC**: ~100  
**Tests**: 5 (trait compile tests, basic structure)

### ✅ Task 3.2: Raw Encoding (COMPLETE)

**File**: `rfb-encodings/src/raw.rs` (✅ 369 lines)

**What was implemented**:
- `RawDecoder` struct implementing `Decoder` trait
- Simplest encoding: uncompressed pixel data
- Read `width * height * bytes_per_pixel` bytes
- Copy directly to pixel buffer
- Handle different pixel formats (RGB888, RGB565, etc.)
- 9 unit tests covering all scenarios
- Zero clippy warnings

**Time taken**: ~45 minutes  
**Commit**: 40512429

### ✅ Task 3.3: CopyRect Encoding (COMPLETE)

**File**: `rfb-encodings/src/copyrect.rs` (✅ 403 lines)

**What was implemented**:
- `CopyRectDecoder` struct implementing `Decoder` trait
- Encoding type 1: copy from (src_x, src_y) to (dst_x, dst_y)
- Wire format: just src_x (u16), src_y (u16) - only 4 bytes!
- Uses `MutablePixelBuffer::copy_rect()` with proper offset calculation
- Handles overlapping rectangles correctly
- 10 comprehensive unit tests (empty, single pixel, non-overlapping, overlapping, error cases)
- 2 doctests for API examples
- Zero clippy warnings

**Time taken**: ~35 minutes  
**Commit**: 40512429

### ✅ Task 3.4: RRE Encoding (COMPLETE)

**File**: `rfb-encodings/src/rre.rs` (✅ 720 lines)

**What was implemented**:
- Rise-and-Run-length Encoding
- Background color + N sub-rectangles
- Good for screens with large solid regions
- Wire format: num_subrects (u32), bg_pixel, then for each: pixel + x,y,w,h
- 15 unit tests + 2 doctests
- Zero clippy warnings

**Time taken**: ~1 hour  
**Commit**: 688a9520

### ✅ Task 3.5: Hextile Encoding (COMPLETE)

**File**: `rfb-encodings/src/hextile.rs` (✅ 1,044 lines)

**What was implemented**:
- Tiled encoding with 16x16 tiles (smaller at edges)
- Five sub-encoding modes per tile:
  - RAW: uncompressed pixel data
  - Background-only fills
  - Foreground + monochrome subrects
  - Colored subrects
  - Mixed combinations
- Background/foreground persistence across tiles within rectangles
- Tile type flags: RAW, BACKGROUND_SPECIFIED, FOREGROUND_SPECIFIED, ANY_SUBRECTS, SUBRECTS_COLOURED
- Subrect position/size nibble encoding
- Comprehensive error handling with context
- 23 unit tests covering all scenarios
- Zero clippy warnings

**Time taken**: ~2 hours  
**Commit**: (pending)

### Task 3.6: Tight Encoding (Week 4, Days 1-3)

**Files**: `rfb-encodings/src/tight.rs`, `rfb-encodings/src/tight/jpeg.rs`, `rfb-encodings/src/tight/zlib.rs`

**What to implement**:
- JPEG compression for photo-like regions
- Zlib compression for other regions
- Palette mode for indexed color
- Requires `jpeg-decoder` and `flate2` crates

**Dependencies to add**:
- jpeg-decoder = "0.3"
- flate2 = "1.0"

**Target LOC**: ~1,200  
**Tests**: 15-20

### Task 3.7: ZRLE Encoding (Week 4, Days 4-5)

**File**: `rfb-encodings/src/zrle.rs`

**What to implement**:
- Zlib + RLE combination
- 64x64 tiles
- Multiple sub-encodings

**Target LOC**: ~600  
**Tests**: 15-18

### Module Structure

```
rfb-encodings/
├── Cargo.toml
└── src/
    ├── lib.rs          (Decoder trait + re-exports)
    ├── raw.rs          (Raw encoding - Task 3.2)
    ├── copyrect.rs     (CopyRect - Task 3.3)
    ├── rre.rs          (RRE - Task 3.4)
    ├── hextile.rs      (Hextile - Task 3.5)
    ├── tight/
    │   ├── mod.rs
    │   ├── jpeg.rs
    │   └── zlib.rs
    └── zrle.rs         (ZRLE - Task 3.7)
```

### Testing Strategy

1. **Unit tests**: Each encoding in its own module
2. **Integration tests**: `tests/encoding_roundtrip.rs`
3. **Test vectors**: Create known good encoded data
4. **Property tests**: Random data should decode without errors
5. **Performance benchmarks**: `benches/decode_speed.rs`

### Success Criteria

- [ ] All 7 encodings implemented
- [ ] 100+ tests passing
- [ ] Zero clippy warnings
- [ ] Comprehensive documentation
- [ ] Can decode real VNC server output
- [ ] Performance acceptable (1080p @ 30fps)

---

## ✅ COMPLETED: Task 1.1 - PixelFormat module

**File**: `rfb-pixelbuffer/src/format.rs` (✅ 448 lines)

**What was implemented**:
- PixelFormat struct with full RFB format support
- `bytes_per_pixel()`, `rgb888()` constructor
- `to_rgb888()`, `from_rgb888()` conversion methods
- Endianness-aware encoding/decoding
- Support for arbitrary bit depths (RGB888, RGB565, etc.)
- 75 lines of comprehensive documentation
- 15 tests (9 unit + 6 doctests) - all passing ✅
- Zero clippy warnings ✅

**Time taken**: ~45 minutes (as estimated!)  
**Commit**: `c54a69e7`

---

## ✅ COMPLETED: Task 1.2 - PixelBuffer Traits

**File**: `rfb-pixelbuffer/src/buffer.rs` (✅ 401 lines)

**What was implemented**:
- **PixelBuffer** trait for read-only buffer access
  - `dimensions()` - Get buffer size
  - `pixel_format()` - Get pixel format reference
  - `get_buffer()` - Get read-only slice with stride
- **MutablePixelBuffer** trait extending PixelBuffer
  - `get_buffer_rw()` - Get mutable slice with stride
  - `commit_buffer()` - Finalize changes
  - `fill_rect()` - Fill rectangle with solid color
  - `copy_rect()` - Copy within buffer (handles overlaps)
  - `image_rect()` - Copy external image data
- 96 lines of comprehensive module-level documentation
- Critical "stride is in pixels" warnings throughout
- 18 doctests with realistic usage examples
- All tests passing ✅

**Time taken**: ~50 minutes (estimated 1 hour)  
**Commit**: `f3e58499`

---

## ✅ COMPLETED: Task 1.3 - ManagedPixelBuffer

**File**: `rfb-pixelbuffer/src/managed.rs` (✅ 542 lines)

**What was implemented**:
- Complete `ManagedPixelBuffer` struct with owned data
- `new()` and `resize()` methods
- Full `PixelBuffer` trait implementation
- Full `MutablePixelBuffer` trait implementation
- Overlap detection for `copy_rect()`
- 10 comprehensive unit tests
- 4 doctests with real-world examples

**Time taken**: ~1h 20m (estimated 1.5 hours)  
**Commit**: `d0da5f2c`

---

### Task 1.3: Implement ManagedPixelBuffer (COMPLETED)


**File**: `rfb-pixelbuffer/src/managed.rs`

**What to implement**:
```rust
pub trait PixelBuffer {
    fn dimensions(&self) -> (u32, u32);
    fn pixel_format(&self) -> &PixelFormat;
    fn get_buffer(&self, rect: Rect, stride: &mut usize) -> Option<&[u8]>;
}

pub trait MutablePixelBuffer: PixelBuffer {
    fn get_buffer_mut(&mut self, rect: Rect, stride: &mut usize) -> Option<&mut [u8]>;
    fn commit_buffer(&mut self, rect: Rect);
    fn fill_rect(&mut self, rect: Rect, pixel: &[u8]) -> Result<()>;
    fn copy_rect(&mut self, src: Rect, dst: Rect) -> Result<()>;
    fn image_rect(&mut self, rect: Rect, data: &[u8], stride: usize) -> Result<()>;
}
```

**Reference**: See RUST_VIEWER.md lines 1260-1287  
**Estimated time**: 1 hour  
**LOC**: ~100

---

### Task 1.3: Implement ManagedPixelBuffer

**File**: `rfb-pixelbuffer/src/managed.rs`

**What to implement**:
```rust
pub struct ManagedPixelBuffer {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
    stride: usize, // In pixels!
}

impl ManagedPixelBuffer {
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self { ... }
    pub fn resize(&mut self, width: u32, height: u32) { ... }
    // Implement PixelBuffer trait
    // Implement MutablePixelBuffer trait
}
```

**Reference**: See RUST_VIEWER.md lines 1289-1430  
**Estimated time**: 1.5 hours  
**LOC**: ~250

---

### Task 1.4: Update rfb-pixelbuffer/src/lib.rs

**File**: `rfb-pixelbuffer/src/lib.rs`

Replace stub with:
```rust
pub mod format;
pub mod buffer;
pub mod managed;

pub use format::PixelFormat;
pub use buffer::{PixelBuffer, MutablePixelBuffer};
pub use managed::ManagedPixelBuffer;
```

**Estimated time**: 5 minutes

---

### Task 1.5: Add dependencies to rfb-pixelbuffer

**File**: `rfb-pixelbuffer/Cargo.toml`

Add:
```toml
[dependencies]
rfb-common = { workspace = true }
anyhow = { workspace = true }
```

**Estimated time**: 2 minutes

---

### Task 1.6: Write unit tests

**File**: `rfb-pixelbuffer/src/buffer.rs` (at bottom)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pixel_format_conversion() {
        let pf = PixelFormat::rgb888();
        let rgb = [255, 128, 64, 255];
        let encoded = pf.from_rgb888(rgb);
        let decoded = pf.to_rgb888(&encoded);
        assert_eq!(rgb, decoded);
    }
    
    #[test]
    fn test_managed_buffer_creation() {
        let fb = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        assert_eq!(fb.dimensions(), (100, 100));
    }
    
    #[test]
    fn test_fill_rect() {
        let mut fb = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let rect = Rect::new(10, 10, 20, 20);
        let red = [255, 0, 0, 255];
        fb.fill_rect(rect, &red).unwrap();
        // Verify the pixels
    }
}
```

**Estimated time**: 30 minutes

---

## Verification Steps

After completing Task 1.1-1.6:

```bash
# 1. Build
export TMPDIR=/tmp && cargo build -p rfb-pixelbuffer

# 2. Run tests
cargo test -p rfb-pixelbuffer

# 3. Check for warnings
cargo clippy -p rfb-pixelbuffer

# 4. Format code
cargo fmt -p rfb-pixelbuffer

# 5. Generate docs
cargo doc -p rfb-pixelbuffer --open
```

**Expected result**: All tests pass, no warnings, documentation looks good.

---

## Phase 1 Completion Checklist

- [x] Task 1.1: PixelFormat implemented ✅ (c54a69e7)
- [x] Task 1.2: Buffer traits defined ✅ (f3e58499)
- [ ] Task 1.3: ManagedPixelBuffer implemented 🔄 NEXT
- [x] Task 1.4: lib.rs updated ✅
- [x] Task 1.5: Dependencies added ✅
- [ ] Task 1.6: Tests written and passing
- [x] Verification: cargo test passes (15/15) ✅
- [x] Verification: cargo clippy shows no warnings ✅
- [x] Documentation: PixelFormat has comprehensive doc comments ✅

---

## After Phase 1: What's Next?

### Phase 2: Network & Protocol (Tasks 2.1-2.6)

1. **Task 2.1**: Implement TCP socket wrapper (`rfb-protocol/src/network/socket.rs`)
2. **Task 2.2**: Implement RFB Reader/Writer (`rfb-protocol/src/io/`)
3. **Task 2.3**: Define message types (`rfb-protocol/src/messages.rs`)
4. **Task 2.4**: Implement connection state machine (`rfb-protocol/src/connection.rs`)
5. **Task 2.5**: RFB version negotiation
6. **Task 2.6**: ClientInit/ServerInit exchange

**Reference**: RUST_VIEWER.md lines 466-1184  
**Estimated time**: 1-2 weeks  
**LOC**: ~1,700

---

## Getting Help

- **Stuck on pixel format conversions?** See the C++ code in:
  - `../common/rfb/PixelFormat.cxx`
  - `../common/rfb/PixelBuffer.cxx`

- **Need examples?** Look at the existing TigerVNC tests:
  - `../tests/unit/pixelformat.cxx`

- **Rust questions?** 
  - https://doc.rust-lang.org/book/
  - https://doc.rust-lang.org/std/

---

## Time Estimates

| Task | Estimated Time | Cumulative |
|------|----------------|------------|
| 1.1 - PixelFormat | 45 min | 45 min |
| 1.2 - Buffer traits | 1 hour | 1h 45m |
| 1.3 - ManagedPixelBuffer | 1.5 hours | 3h 15m |
| 1.4 - lib.rs update | 5 min | 3h 20m |
| 1.5 - Dependencies | 2 min | 3h 22m |
| 1.6 - Tests | 30 min | 3h 52m |
| **Total Phase 1** | **~4 hours** | - |

---

## Progress Tracking

Update STATUS.md after completing each task:

```bash
# After Task 1.1
echo "- [x] Task 1.1: PixelFormat implemented" >> progress.txt

# After all tasks
echo "Phase 1 complete!" >> STATUS.md
cargo test --workspace
```

---

## Quick Commands Reference

```bash
# Work on specific crate
cd rfb-pixelbuffer

# Build just this crate
cargo build

# Run tests for this crate
cargo test

# Watch for changes (requires cargo-watch)
cargo watch -x test

# Check without building
cargo check

# Back to workspace root
cd ..
```

---

**Start with Task 1.1!** 🚀

Create `rfb-pixelbuffer/src/format.rs` and implement the `PixelFormat` struct.
