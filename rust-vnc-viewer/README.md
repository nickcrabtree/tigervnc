# Rust VNC Viewer

A pure Rust implementation of a VNC (Virtual Network Computing) viewer client.

## Project Structure

This is a Cargo workspace containing multiple crates:

- **`rfb-common`** - Common types (geometry, configuration, cursors)
- **`rfb-pixelbuffer`** - Pixel buffer abstraction and management  
- **`rfb-protocol`** - RFB protocol implementation (network, I/O, messages)
- **`rfb-encodings`** - Encoding/decoding implementations (Raw, CopyRect, Tight, ZRLE, Hextile, RRE)
- **`rfb-client`** - High-level async VNC client library with connection management
- **`njcvncviewer-rs`** - Main GUI viewer application (egui-based)

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

✅ **Working Viewer** - Functional VNC client with GUI!

**Current Status**: Two implementations available
**Last Updated**: 2025-10-23  
**Overall Progress**: ~70% (Foundation complete + working GUI viewer)

### Completed ✅
- [x] **Phase 1**: rfb-pixelbuffer crate (complete, 19 tests)
- [x] **Phase 2**: rfb-protocol crate (complete, 118 tests)
- [x] **Phase 3**: rfb-encodings crate (complete, 93 tests)
  - [x] Raw, CopyRect, RRE, Hextile, Tight, ZRLE encodings
- [x] **rfb-client**: High-level async client library
  - [x] Connection management and event loop
  - [x] Transport layer (TCP/TLS)
  - [x] Configuration and error handling
- [x] **njcvncviewer-rs**: GUI viewer application
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
- [ ] ARC cache eviction algorithm (upgrade from LRU)
- [ ] Touch gesture support
- [ ] Clipboard integration
- [ ] Connection profiles and improved UI

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
