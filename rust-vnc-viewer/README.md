# Rust VNC Viewer

**Desktop-focused** high-performance VNC viewer with **ContentCache protocol support** for 97-99% bandwidth reduction and excellent **fullscreen/multi-monitor** experience.

Note: This README is the canonical documentation for the Rust viewer. Historical phase/progress reports have been removed to reduce noise. For roadmap and usage, see docs/ROADMAP.md and docs/cli/USAGE.md.

## 🚀 Key Features

- **ContentCache Protocol**: 97-99% bandwidth reduction for repeated content
- **Complete RFB Implementation**: All standard encodings (Raw, CopyRect, RRE, Hextile, Tight, ZRLE)
- **Desktop-Optimized**: Fullscreen and multi-monitor support for desktop workflows
- **CLI-Configured**: Command-line driven configuration, no settings GUI
- **Async Architecture**: Tokio-based for high performance and responsiveness
- **Cross-platform GUI**: egui-based interface with smooth rendering
- **Production Ready**: Comprehensive error handling, logging, and testing

## Project Structure

This is a Cargo workspace containing multiple crates:

- **`rfb-common`** - Common types (geometry, configuration, cursors)
- **`rfb-pixelbuffer`** - Pixel buffer abstraction and management  
- **`rfb-protocol`** - RFB protocol implementation (network, I/O, messages)
- **`rfb-encodings`** - Encoding/decoding implementations with ContentCache support
- **`rfb-client`** - High-level async VNC client library with framebuffer sharing
- **`platform-input`** - Cross-platform input handling (keyboard, mouse, gestures)
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

🎆 **Major Milestone: All Standard VNC Encodings Complete!** 🎆

✅ **Phase 9A Complete** - All standard VNC encodings (Tight, ZRLE, Hextile, RRE) with production quality!  
✅ **Phase 8A Complete** - ContentCache protocol for 97-99% bandwidth reduction!  
✅ **Phase 7 Complete** - GUI integration with framebuffer rendering  
✅ **Phase 6 Complete** - Platform input handling (keyboard, mouse, gestures)

**Current Status**: Production-ready VNC client with full encoding support  
**Last Updated**: 2025-10-24  
**Overall Progress**: ~90% (All standard encodings + ContentCache + GUI complete)
**Tests**: All workspace unit/integration/doc tests passing

### Completed ✅
- [x] **Phase 1-3**: Core protocol libraries (230+ tests)
  - [x] rfb-pixelbuffer: Pixel format handling and buffer management
  - [x] rfb-protocol: RFB messages, I/O streams, handshake
  - [x] rfb-encodings: All standard encodings + ContentCache decoders
- [x] **Phase 4**: rfb-client crate (async client library)
  - [x] Connection management and event loop
  - [x] Framebuffer sharing with GUI
  - [x] Error handling and reconnection
- [x] **Phase 5**: rfb-display crate (rendering pipeline)
  - [x] GPU-accelerated rendering with scaling
  - [x] Viewport management and cursor handling
- [x] **Phase 6**: platform-input crate (input handling)
  - [x] Comprehensive keyboard mapping (X11 keysyms)
  - [x] Mouse events with throttling and emulation
  - [x] Touch gestures and momentum scrolling
  - [x] Configurable keyboard shortcuts
- [x] **Phase 7**: GUI Integration
  - [x] Complete egui-based viewer application
  - [x] Framebuffer access and rendering
  - [x] Input event forwarding
- [x] **Phase 8A**: ContentCache Protocol ⭐
  - [x] CachedRect/CachedRectInit message types
  - [x] Client-side cache with LRU eviction
  - [x] 97-99% bandwidth reduction for repeated content
  - [x] Full integration with decoder registry

### Just Completed ✅
- [x] **Phase 9A**: Advanced Encodings (**COMPLETE!**)
  - [x] Tight encoding (JPEG + zlib, most advanced)
  - [x] ZRLE encoding (zlib + RLE with 64x64 tiling)  
  - [x] Hextile encoding (16x16 tiles with sub-rectangles)
  - [x] RRE encoding (Rise-and-Run-length encoding)
  - [x] 94+ tests passing, production-quality implementation

### Next Priority 🎯 (Milestone M1/M2)
- [ ] **Fullscreen enhancements** (M1 - HIGH PRIORITY)
  - [x] F11 toggle, CLI `--fullscreen` option
  - [ ] Borderless/exclusive modes with DPI awareness
  - [ ] Scaling policies (fit, fill, 1:1) with aspect ratio control
- [ ] **Multi-monitor support** (M2 - HIGH PRIORITY)  
  - [x] Monitor enumeration and CLI selection parsing (`--monitor primary|index|name`)
  - [x] Runtime hotkeys (Ctrl+Alt+←/→, Ctrl+Alt+0-9, Ctrl+Alt+P)
  - [ ] Mixed DPI handling and smooth transitions

### Recently Completed ✅
- [x] **Phase 9B-Clipboard**: Bidirectional clipboard integration

## Documentation

### Current Priorities
- **[docs/ROADMAP.md](docs/ROADMAP.md)** - 🎯 **Prioritized roadmap** with fullscreen/multi-monitor focus
- **[docs/cli/USAGE.md](docs/cli/USAGE.md)** - 📋 **CLI usage guide** for desktop-focused configuration
- **[docs/spec/fullscreen-and-multimonitor.md](docs/spec/fullscreen-and-multimonitor.md)** - 🔧 **Technical specification** for M1/M2 features
- **[docs/SEP/SEP-0001-out-of-scope.md](docs/SEP/SEP-0001-out-of-scope.md)** - 🚫 **Out-of-scope features** (touch, settings UI, screenshots)

### Implementation Notes
- All standard encodings implemented; ContentCache protocol supported
- For priorities and next milestones, see docs/ROADMAP.md
- For CLI usage, see docs/cli/USAGE.md

### Technical References  
- **[../CONTENTCACHE_DESIGN_IMPLEMENTATION.md](../CONTENTCACHE_DESIGN_IMPLEMENTATION.md)** - C++ ContentCache reference
- **[../WARP.md](../WARP.md)** - Build and test environment guide

## Quick Start

```bash
# Build the desktop viewer
cd rust-vnc-viewer
cargo build --release

# Basic connection (windowed)
cargo run --release --package njcvncviewer-rs -- localhost:2

# Fullscreen on primary monitor
cargo run --release --package njcvncviewer-rs -- --fullscreen --monitor primary localhost:2

# Multi-monitor: fullscreen on second monitor with fit scaling
cargo run --release --package njcvncviewer-rs -- --fullscreen --monitor 1 --scale fit localhost:2

# With password from environment (secure)
VNC_PASSWORD=secret cargo run --release --package njcvncviewer-rs -- server:5901

# Run all tests (320+ tests across all crates)
cargo test

# Verbose logging for debugging
cargo run --package njcvncviewer-rs -- -vv localhost:2
```

### Testing with WARP Safety Rules

⚠️ **Important**: Use test server only (per [WARP.md](../WARP.md) safety rules)

```bash
# Safe: Connect to test server Xnjcvnc :2 (port 5902)
cargo run -- localhost:2

# ❌ NEVER: Do not connect to production servers :1 or :3
# These are production servers for unrelated work
```

## Performance

With ContentCache protocol enabled:
- **Bandwidth**: 97-99% reduction for repeated content
- **Latency**: Sub-second page loads vs 10-30 second refreshes
- **Memory**: 2GB default cache, configurable
- **CPU**: Zero decode cost for cache hits

## Architecture

```
┌─────────────────────────────────────────────┐
│ njcvncviewer-rs (GUI Application)           │
├─────────────────────────────────────────────┤
│ platform-input │ rfb-display │ rfb-client  │
│ (Input)        │ (Rendering) │ (Protocol)  │
├─────────────────────────────────────────────┤  
│ rfb-encodings (Decoders + ContentCache)    │
├─────────────────────────────────────────────┤
│ rfb-protocol (Messages + I/O)              │
├─────────────────────────────────────────────┤
│ rfb-pixelbuffer │ rfb-common                │
│ (Pixels)        │ (Types)                   │
└─────────────────────────────────────────────┘
```

## Out-of-Scope Features

The following features are **explicitly out-of-scope** for the desktop-focused viewer (see [SEP-0001](docs/SEP/SEP-0001-out-of-scope.md)):

- **🚫 Touch/Gesture Support**: Desktop-only; use trackpad scrolling
- **🚫 Settings UI/Profiles**: Use CLI configuration and environment variables  
- **🚫 Screenshot Capture**: Use OS tools (`gnome-screenshot`, `grim`, `scrot`, etc.)

### Alternatives

**Screenshots**: Use native OS tools instead:
```bash
# X11: Capture VNC window
gnome-screenshot --window --file vnc-session.png

# Wayland: Capture active window  
grim -g "$(swaymsg -t get_tree | jq -r '.. | select(.focused?) | .rect | "\(.x),\(.y) \(.width)x\(.height)"')" vnc.png
```

**Configuration**: Use command-line flags:
```bash
cargo run -- --connect vnc://server:5901 --fullscreen --monitor primary --scale fit
```

## License

GPL-2.0-or-later
