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

✅ **Working Viewer** - Basic functional VNC client implemented!

**Current Status**: Functional viewer with ContentCache integration planned  
**Last Updated**: 2025-10-23  
**Overall Progress**: ~55% (7,000+ / 12,500 LOC)

### Completed ✅
- [x] **Phase 0**: Workspace structure with 6 crates
- [x] **Phase 1**: rfb-pixelbuffer crate (complete, 19 tests)
- [x] **Phase 2**: rfb-protocol crate (complete, 118 tests)
- [x] **Phase 3**: Basic encodings (Raw, CopyRect)
- [x] **Viewer**: njcvncviewer-rs application
  - [x] RFB handshake and connection management
  - [x] egui-based GUI with framebuffer rendering
  - [x] Mouse and keyboard input forwarding
  - [x] Zoom and scroll functionality
  - [x] Connection state display

### In Progress 🔄
- [ ] **ContentCache protocol integration** (Weeks 1-4)
  - [ ] Protocol message types
  - [ ] Client-side cache with LRU eviction
  - [ ] CachedRect/CachedRectInit decoders
  - [ ] Integration and testing

### Planned 📋
- [ ] Advanced encodings (Tight, ZRLE, Hextile, RRE)
- [ ] ARC cache eviction algorithm
- [ ] Touch gesture support
- [ ] Clipboard integration
- [ ] Connection profiles

## Documentation

- **[RUST_VIEWER_STATUS.md](RUST_VIEWER_STATUS.md)** - 👈 **START HERE** - Complete status and ContentCache implementation plan
- **[STATUS.md](STATUS.md)** - Detailed progress tracking
- **[PROGRESS.md](PROGRESS.md)** - Phase-by-phase completion metrics
- **[../CONTENTCACHE_DESIGN_IMPLEMENTATION.md](../CONTENTCACHE_DESIGN_IMPLEMENTATION.md)** - C++ ContentCache reference

## Quick Start

```bash
# Build the viewer
cd rust-vnc-viewer
cargo build --package njcvncviewer-rs

# Run the viewer (connect to display :2)
cargo run --package njcvncviewer-rs -- localhost:2

# Run with verbose logging
cargo run --package njcvncviewer-rs -- -vv localhost:2

# Run tests
cargo test

# See implementation plan
cat RUST_VIEWER_STATUS.md
```

## License

GPL-2.0-or-later
