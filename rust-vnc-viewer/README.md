# Rust VNC Viewer

A pure Rust implementation of a VNC (Virtual Network Computing) viewer client.

## Project Structure

This is a Cargo workspace containing multiple crates:

- **`rfb-common`** - Common types (geometry, configuration, cursors)
- **`rfb-pixelbuffer`** - Pixel buffer abstraction and management  
- **`rfb-protocol`** - RFB protocol implementation (network, I/O, messages)
- **`rfb-encodings`** - Encoding/decoding implementations (Raw, Tight, ZRLE, etc.)
- **`platform-input`** - Platform-specific input handling (keyboard, touch)
- **`rvncviewer`** - Main viewer application

## Building

```bash
# Build all crates
cargo build

# Build release version
cargo build --release

# Run the viewer
cargo run --release
```

## Status

🚧 **Under Development** - This is a work in progress.

**Current Phase**: Phase 2 starting - Network & Protocol Layer  
**Last Updated**: 2025-10-08 12:35 UTC

### Completed ✅
- [x] **Phase 0**: Workspace structure with 6 crates
- [x] **Phase 0**: Core types (Point, Rect, Cursor, Config)
- [x] **Phase 0**: Build system verified
- [x] **Phase 0**: Documentation structure
- [x] **Phase 1**: rfb-pixelbuffer crate (1,416 LOC, 37 tests passing)
- [x] **Phase 1**: PixelFormat, PixelBuffer traits, ManagedPixelBuffer

### In Progress 🔄
- [ ] **NEXT**: Socket abstractions (TCP, Unix domain) - Task 2.1
- [ ] RFB I/O streams (buffered reading/writing)
- [ ] Protocol state machine
- [ ] Message types (ServerInit, FramebufferUpdate, etc.)
- [ ] Protocol handshake (version negotiation, security)

### Planned 📋
- [ ] All standard VNC encodings (Tight, ZRLE, etc.)
- [ ] ContentCache implementation
- [ ] GUI framework integration (egui)
- [ ] Touch gesture support
- [ ] Clipboard integration

## Documentation

- **[NEXT_STEPS.md](NEXT_STEPS.md)** - 👈 **START HERE** - Detailed next tasks with code examples
- **[STATUS.md](STATUS.md)** - Current progress and statistics
- **[GETTING_STARTED.md](GETTING_STARTED.md)** - Development guide and workflow
- **[../RUST_VIEWER.md](../RUST_VIEWER.md)** - Complete implementation plan (~12,500 LOC)

## Quick Start

```bash
# Clone and enter directory (already done)
cd rust-vnc-viewer

# Build workspace
export TMPDIR=/tmp && cargo build

# Run tests (when available)
cargo test

# See what to do next
cat NEXT_STEPS.md
```

## License

GPL-2.0-or-later
