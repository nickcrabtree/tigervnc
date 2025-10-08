# Next Steps - Rust VNC Viewer Development

**Current Phase**: Phase 1 In Progress - Task 1.1 Complete ✅  
**Last Updated**: 2025-10-08 11:57 UTC

---

## 🎯 IMMEDIATE NEXT STEP

**Continue `rfb-pixelbuffer` crate - PixelBuffer traits and ManagedPixelBuffer**

Task 1.1 (PixelFormat) is complete! Now we need the buffer traits and implementation.

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

### Task 1.2: Create PixelBuffer traits 🔄 NEXT


**File**: `rfb-pixelbuffer/src/buffer.rs`

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
- [ ] Task 1.2: Buffer traits defined 🔄 NEXT
- [ ] Task 1.3: ManagedPixelBuffer implemented
- [ ] Task 1.4: lib.rs updated
- [ ] Task 1.5: Dependencies added
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
