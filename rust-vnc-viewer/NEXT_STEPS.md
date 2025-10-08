# Next Steps - Rust VNC Viewer Development

**Current Phase**: Phase 2 - Network & Protocol Layer (60% complete)  
**Last Updated**: 2025-10-08 14:23 Local

---

## 🎯 IMMEDIATE NEXT STEP

**Task 2.4: RFB Message Types** in `rfb-protocol/src/messages/`

Phase 1 is 100% complete! Phase 2 Tasks 2.1-2.3 done (60%). Ready for message types implementation.

---

## ✅ PHASE 1 COMPLETE!

**Phase 1: Core Types** - rfb-pixelbuffer crate  
**Status**: All tasks complete (1.1-1.6) ✅  
**LOC Written**: 1,416 lines (code + docs + tests)  
**Tests**: 37 passing (19 unit + 18 doctests)  
**Time Taken**: ~3 hours (estimated 4 hours)  
**Commits**: c54a69e7, f3e58499, d0da5f2c

---

## 🚀 PHASE 2: Network & Protocol Layer (rfb-protocol crate)

**Target**: Core networking and RFB protocol implementation  
**Estimated Time**: 2 weeks (13 days)  
**Estimated LOC**: ~1,700  
**Current Status**: 60% complete (Tasks 2.1-2.3 done)

### ✅ Completed Tasks
- ✅ **Task 2.1**: Socket abstractions (TCP, Unix domain) - ~430 LOC
- ✅ **Task 2.2**: RFB I/O streams (buffered read/write) - ~680 LOC
- ✅ **Task 2.3**: Connection state machine - ~545 LOC

---

## 🔄 NEXT: Task 2.4 - RFB Message Types

**Files**: `rfb-protocol/src/messages/` directory

### Module Structure

Create the following files:
- `mod.rs` - Module declarations and shared traits
- `types.rs` - PixelFormat, Rectangle, encoding constants, security types
- `server.rs` - Server-to-client messages
- `client.rs` - Client-to-server messages

### Core Types to Implement (types.rs)

**PixelFormat** struct:
```rust
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: u8,      // Boolean: 0 or 1 only
    pub true_color: u8,      // Boolean: 0 or 1 only
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
    // 3 bytes padding (must be zero)
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> u8 { /* ... */ }
    pub fn read_from<R: AsyncRead + Unpin>(reader: &mut RfbInStream<R>) -> Result<Self>;
    pub fn write_to<W: AsyncWrite + Unpin>(&self, writer: &mut RfbOutStream<W>) -> Result<()>;
}
```

**Rectangle** header:
```rust
pub struct Rectangle {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: i32,
}
```

**Encoding constants**:
```rust
pub const ENCODING_RAW: i32 = 0;
pub const ENCODING_COPYRECT: i32 = 1;
pub const ENCODING_RRE: i32 = 2;
pub const ENCODING_HEXTILE: i32 = 5;
pub const ENCODING_TIGHT: i32 = 7;
pub const ENCODING_ZRLE: i32 = 16;
```

### Server Messages (server.rs)

1. **ServerInit** - framebuffer_width, framebuffer_height, pixel_format, name
2. **FramebufferUpdate** - rectangle headers only (payloads in Phase 3)
3. **SetColorMapEntries** - color map updates
4. **Bell** - simple notification
5. **ServerCutText** - clipboard data

### Client Messages (client.rs)

1. **ClientInit** - shared flag
2. **SetPixelFormat** - change pixel format
3. **SetEncodings** - list of supported encodings
4. **FramebufferUpdateRequest** - request screen updates
5. **KeyEvent** - keyboard input
6. **PointerEvent** - mouse input
7. **ClientCutText** - clipboard data

### Parsing Rules (CRITICAL)

1. **All multi-byte integers are big-endian** (network byte order)
2. **Booleans are strictly 0 or 1** - reject any other value with error
3. **Padding bytes must be zero** - validate or error
4. **For FramebufferUpdate**: Parse rectangle headers fully, but do NOT consume encoding-specific payloads in Task 2.4 (those depend on decoders in Phase 3)
5. **Variable-length messages**: Only parse those with explicit length fields (e.g., ServerCutText, ClientCutText)

### Implementation Steps

1. Create `rfb-protocol/src/messages/` directory
2. Implement `mod.rs` with module declarations and shared traits
3. Implement `types.rs` with PixelFormat, Rectangle, constants
4. Implement `server.rs` with all server messages
5. Implement `client.rs` with all client messages
6. Add 20-25 unit tests covering:
   - PixelFormat round-trip with padding validation
   - All message types parse/serialize correctly
   - Boolean validation (reject values != 0 or 1)
   - Error cases (invalid padding, mismatched counts, etc.)
7. Add rustdoc documentation with examples
8. Export from `rfb-protocol/src/lib.rs`

### Testing Strategy

- Minimal, deterministic test fixtures
- Round-trip tests (write then read)
- Error case validation (invalid booleans, padding, etc.)
- Edge cases (empty strings, zero-length arrays)

**Reference**: RUST_VIEWER.md lines 698-768  
**Estimated time**: 3-4 days  
**Target LOC**: 400-500  
**Target tests**: 20-25

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
