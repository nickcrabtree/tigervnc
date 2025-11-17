# Implementation Plan: Feature-Complete rvncviewer (Phases 4–8)

**Last Updated**: 2025-10-09 19:59 UTC  
**Status**: Phase 3 COMPLETE ✅ - Foundation ready  
**Goal**: Build a production-ready VNC viewer binary with full feature parity to C++ vncviewer

---

## 🎉 Phases 1-3: COMPLETE!

### ✅ Phase 1: Core Types (rfb-pixelbuffer)
- **LOC**: 1,416 (code + docs + tests)
- **Tests**: 19 passing
- **Features**: PixelFormat, PixelBuffer traits, ManagedPixelBuffer

### ✅ Phase 2: Network & Protocol (rfb-protocol)
- **LOC**: 3,502 (206% of target)
- **Tests**: 118 passing
- **Features**: Socket abstractions, RFB I/O streams, state machine, message types, handshake

### ✅ Phase 3: Encodings (rfb-encodings)
- **LOC**: 5,437 (155% of target)
- **Tests**: 93 passing
- **Features**: All 7 encodings (Raw, CopyRect, RRE, Hextile, Tight, ZRLE)

**Foundation Stats**:
- Total LOC: ~10,800
- Total Tests: 233 passing ✅
- Core Protocol: 98% complete
- Ready for application development!

---

## 🚀 Phase 4: Core Connection & Event Loop (rfb-client crate)

**Goal**: Create a high-level async VNC client library that handles connection lifecycle, framebuffer updates, and error recovery.

### Scope

- **Async connection management** using existing rfb-protocol building blocks
- **Tokio runtime-driven event loop** with proper task coordination
- **Framebuffer update processing pipeline**: decode → compose → dispatch
- **Robust error handling** with fail-fast policy (no defensive fallbacks)
- **Reconnection logic** with configurable retry policies
- **Command-line argument support** via clap (feature-gated for reusability)
- **Configuration file support** (TOML format) for persistent preferences
- **Security types**: None, VNC password, TLS (via rustls)

### Estimated Effort
- **LOC**: 1,200–1,800
- **Time**: 6–10 dev days
- **Dependencies**: tokio, bytes, futures, rustls, clap (optional), serde, toml, tracing

### Files to Create

```
rfb-client/
├── Cargo.toml                      # Dependencies and features
├── src/
│   ├── lib.rs                      # Public API, feature flags, re-exports
│   ├── connection.rs               # RfbConnection: handshake, security negotiation
│   ├── transport.rs                # TCP/TLS transport via tokio + rustls
│   ├── protocol.rs                 # Message reading/writing (uses rfb-protocol)
│   ├── event_loop.rs               # Tokio task coordination, channel plumbing
│   ├── framebuffer.rs              # Framebuffer state management
│   ├── messages.rs                 # Typed event enums (FramebufferUpdate, Bell, etc.)
│   ├── errors.rs                   # Error types using thiserror
│   ├── config.rs                   # Configuration model with serde
│   └── args.rs                     # CLI argument parsing (feature = "cli")
├── tests/
│   └── integration.rs              # Handshake tests with local VNC server
└── examples/
    └── headless_connect.rs         # Debug tool: connect and log events
```

### Key Dependencies

```toml
[dependencies]
rfb-common = { workspace = true }
rfb-pixelbuffer = { workspace = true }
rfb-protocol = { workspace = true }
rfb-encodings = { workspace = true }

tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "io-util", "time"] }
bytes = "1"
futures = "0.3"
tokio-util = { version = "0.7", features = ["codec"] }
thiserror = "1"
anyhow = "1"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
rustls = "0.23"
tokio-rustls = "0.25"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
flume = "0.11"  # or crossbeam-channel

# Optional CLI support
clap = { version = "4", features = ["derive", "env"], optional = true }

[features]
cli = ["dep:clap"]
```

### Test Requirements

1. **Unit tests**:
   - Protocol message serialization/deserialization round-trips
   - Configuration validation (required fields, range checks)
   - Reconnection policy edge cases
   
2. **Integration tests**:
   - Connect to local TigerVNC or x11vnc server
   - Complete handshake sequence
   - Receive and decode framebuffer updates
   - Error path validation (wrong password, server disconnect)
   
3. **Concurrency tests**:
   - Event loop task cancellation
   - Channel backpressure handling
   - Graceful shutdown

### Success Criteria

- ✅ Connects to VNC servers (TigerVNC, RealVNC, x11vnc)
- ✅ Completes RFB handshake with version negotiation
- ✅ Security negotiation works (None, VNCAuth, TLS)
- ✅ Sets encoding preferences and requests updates
- ✅ Receives and dispatches framebuffer updates via channels
- ✅ Clear, contextual error messages (no silent failures)
- ✅ Reconnection logic works within fail-fast policy
- ✅ Configuration loads from file and CLI args
- ✅ No UI dependency (headless operation supported)
- ✅ Zero clippy warnings

---

## 🎨 Phase 5: Display & Rendering (rfb-display crate)

**Goal**: Efficient framebuffer-to-screen rendering using modern graphics APIs with proper scaling, viewport, and cursor support.

### Scope

- **Efficient rendering** on macOS using winit + pixels (wgpu/Metal backend)
- **Multiple scaling modes**: fit window, fill window, 1:1 native
- **Viewport management**: pan, zoom, scroll
- **Cursor rendering**: local cursor, remote cursor, dot cursor modes
- **Multi-monitor support** via winit monitor APIs
- **High DPI awareness** for Retina displays
- **Performance target**: 60 fps for 1080p framebuffers

### Estimated Effort
- **LOC**: 900–1,400
- **Time**: 5–8 dev days
- **Dependencies**: winit, pixels, raw-window-handle, image

### Files to Create

```
rfb-display/
├── Cargo.toml
├── src/
│   ├── lib.rs                      # Renderer facade, public API
│   ├── renderer.rs                 # pixels/wgpu context, present pipeline
│   ├── viewport.rs                 # Pan/zoom/scroll logic, scale strategies
│   ├── cursor.rs                   # Soft cursor composition (local, remote, dot)
│   ├── scaling.rs                  # Nearest vs linear filtering, DPI handling
│   └── monitor.rs                  # Multi-monitor layout, window placement
└── tests/
    └── render_smoke.rs             # Offscreen smoke tests (best-effort)
```

### Key Dependencies

```toml
[dependencies]
rfb-common = { workspace = true }
rfb-pixelbuffer = { workspace = true }

winit = "0.29"
pixels = "0.13"
raw-window-handle = "0.6"
image = "0.25"  # For cursor bitmaps, screenshots
```

### Test Requirements

1. **Unit tests**:
   - Scaling math (fit/fill/1:1 calculations)
   - Viewport coordinate transforms
   - Cursor composition logic
   
2. **Integration tests** (where feasible):
   - Render synthetic framebuffers offscreen
   - Verify dimension/scale changes
   - Screenshot-based regression (best-effort on macOS)

### Success Criteria

- ✅ Smooth 60 fps rendering for 1080p frames on macOS Metal
- ✅ Correct scaling: fit, fill, and 1:1 native modes
- ✅ Viewport pan/zoom/scroll works smoothly
- ✅ Window resizing updates without artifacts
- ✅ Cursor modes switch correctly (local/remote/dot)
- ✅ Multi-monitor window placement via configuration
- ✅ High DPI/Retina display support
- ✅ Zero clippy warnings

---

## ⌨️ Phase 6: Input Handling (platform-input crate)

**Goal**: Capture keyboard, mouse, and touch input from the local system and translate to RFB protocol events.

### Scope

- **Keyboard capture** and translation to RFB keysyms
- **Mouse/pointer events**: buttons, scroll wheel, motion
- **Touch/gesture support** for macOS trackpads (via winit)
- **Keyboard shortcuts**: toggle fullscreen, view-only, scaling, etc.
- **Middle-button emulation**: left+right chord or configurable
- **Pointer event throttling** to prevent network flooding

### Estimated Effort
- **LOC**: 600–900
- **Time**: 4–6 dev days
- **Dependencies**: winit, bitflags, phf (optional for static keymaps)

### Files to Create

```
platform-input/
├── Cargo.toml
├── src/
│   ├── lib.rs                      # InputDispatcher public API
│   ├── keyboard.rs                 # Keycode → keysym mapping, modifiers, IME
│   ├── mouse.rs                    # Button/scroll events, throttling
│   ├── gestures.rs                 # Pinch-to-zoom, two-finger scroll
│   └── shortcuts.rs                # Configurable shortcut mappings
└── tests/
    └── keymap.rs                   # Key mapping table tests
```

### Key Dependencies

```toml
[dependencies]
rfb-common = { workspace = true }

winit = "0.29"
bitflags = "2"
phf = { version = "0.11", optional = true }  # For static key maps
```

### Test Requirements

1. **Unit tests**:
   - Key mapping tables (ASCII, function keys, modifiers → RFB keysyms)
   - Modifier state tracking (Shift, Ctrl, Alt, Cmd)
   - Pointer throttling rate limiting
   
2. **Integration tests**:
   - Simulate winit input events
   - Assert correct RFB event sequences produced
   - Verify rate-limit enforcement

### Success Criteria

- ✅ Correct key translations for common keyboard layouts
- ✅ Accurate modifier key handling (Shift, Ctrl, Alt, Cmd)
- ✅ Smooth pointer and scroll behavior with throttling
- ✅ Gesture-based zoom/scroll integrated with viewport
- ✅ Middle-button emulation works and is configurable
- ✅ Keyboard shortcuts trigger correct actions
- ✅ Zero clippy warnings

---

## 🖥️ Phase 7: GUI Integration (rvncviewer binary)

**Goal**: Build the complete VNC viewer application with dialogs, menus, and desktop window using modern Rust GUI framework.

### Scope

- **GUI framework**: egui via eframe (replaces FLTK from C++ version)
- **Connection dialog**: host, port, password, encoding options
- **Options/preferences dialog**: scaling, input, reconnection settings
- **Menu bar**: File, View, Options, Help
- **Desktop window**: container hosting the rfb-display surface
- **Status bar**: connection stats (FPS, latency, bandwidth, updates/sec)
- **Fullscreen mode support**
- **About dialog** with version info
- **Persistent preferences** saved to user config directory

### Estimated Effort
- **LOC**: 700–1,100
- **Time**: 5–9 dev days
- **Dependencies**: eframe, egui, winit, clap, directories, serde, toml

### Files to Create

```
rvncviewer/
├── Cargo.toml
├── src/
│   ├── main.rs                     # Bootstrap: logging, args, runtime
│   ├── app.rs                      # egui App trait implementation
│   └── ui/
│       ├── mod.rs
│       ├── connection_dialog.rs    # Server connection UI
│       ├── options_dialog.rs       # Preferences UI
│       ├── about.rs                # About dialog
│       ├── menubar.rs              # Application menu
│       ├── statusbar.rs            # Connection statistics
│       └── desktop.rs              # Desktop window container
└── assets/
    └── icon.png                    # Application icon
```

### Key Dependencies

```toml
[dependencies]
rfb-client = { path = "../rfb-client", features = ["cli"] }
rfb-display = { path = "../rfb-display" }
platform-input = { path = "../platform-input" }

eframe = "0.27"
egui = "0.27"
winit = "0.29"
clap = { version = "4", features = ["derive", "env"] }
directories = "5"  # For config directory paths
serde = { version = "1", features = ["derive"] }
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### Test Requirements

1. **UI smoke tests** (where feasible):
   - Verify dialogs open and close
   - Snapshot basic layouts
   
2. **Integration tests**:
   - Connect via UI to local VNC server
   - Exercise connection and options dialogs
   - Verify scaling modes work
   
3. **CLI tests**:
   - Verify clap args bridge to rfb-client config
   - Test command-line connection flow

### Success Criteria

- ✅ Launch rvncviewer and display connection dialog
- ✅ Connect to VNC servers via dialog or CLI arguments
- ✅ Stable GUI rendering of remote desktop with smooth updates
- ✅ Fullscreen mode works correctly
- ✅ Scaling modes functional (fit, fill, 1:1)
- ✅ Menus and dialogs responsive and functional
- ✅ Status bar shows accurate connection statistics
- ✅ Preferences persist across sessions
- ✅ Keyboard shortcuts work
- ✅ No regressions in rendering or input
- ✅ Zero clippy warnings

---

## 🎯 Phase 8: Desktop-Focused Features (UPDATED PRIORITIES)

**Goal**: Excellent fullscreen and multi-monitor experience for desktop VNC usage. Advanced features are deprioritized per [SEP-0001](docs/SEP/SEP-0001-out-of-scope.md).

### New Scope (Desktop-Focused)

#### High Priority: Fullscreen & Multi-Monitor (M1/M2)
- **Fullscreen improvements**: F11 toggle, CLI start option, borderless vs exclusive modes
- **Multi-monitor support**: Monitor enumeration, selection, hotkey navigation
- **Scaling enhancements**: Fit/Fill/1:1 policies with DPI awareness
- **Monitor management**: Primary detection, name/index selection, mixed DPI handling

#### Medium Priority: Polish (M3)
- **Window state memory**: Remember size/position per connection
- **Connection management**: Recent connections (CLI-based)
- **Performance monitoring**: Bandwidth/latency metrics display

#### Out-of-Scope (Explicitly Removed)
Per [SEP-0001](docs/SEP/SEP-0001-out-of-scope.md), the following are **permanently out-of-scope**:
- **Touch/Gesture support**: Desktop-only focus
- **Settings UI/Profiles**: Use CLI configuration
- **Screenshot functionality**: Use OS tools (gnome-screenshot, grim, etc.)

### Estimated Effort (Updated)
- **M1 (Fullscreen)**: 1-2 weeks, ~800-1,200 LOC
- **M2 (Multi-monitor)**: 1-2 weeks, ~600-1,000 LOC  
- **Dependencies**: winit (primary), egui/eframe, platform-specific APIs

### Modules to Create/Extend (M1/M2 Focus)

```
njcvncviewer-rs/src/
├── display/
│   ├── mod.rs                      # DisplayManager trait and Monitor model
│   ├── winit_backend.rs            # Winit-based monitor enumeration
│   └── monitor_selection.rs        # Primary/index/name selection logic
├── fullscreen/
│   ├── mod.rs                      # FullscreenController and state
│   ├── transitions.rs              # Enter/exit/toggle logic
│   └── hotkeys.rs                  # F11, Ctrl+Alt+Arrow navigation
├── scaling/
│   ├── mod.rs                      # Scaling policies (fit/fill/1:1)
│   ├── calculations.rs             # Viewport and aspect ratio math
│   └── dpi.rs                      # DPI handling for mixed environments
└── cli.rs                          # Extended CLI args (--fullscreen, --monitor)

# Files to Remove/Avoid
# - No src/touch.rs or src/gestures/
# - No src/ui/settings/ or src/profiles/  
# - No src/screenshot.rs or recording features
```

### Key Dependencies (Updated)

```toml
# Primary windowing and monitor management
winit = "0.28"                      # Cross-platform window/monitor APIs
egui = "0.27"                       # GUI framework
eframe = "0.27"                     # egui app framework

# CLI and configuration
clap = { version = "4", features = ["derive"] }  # Command-line parsing
serde = { version = "1", features = ["derive"] }   # Config serialization

# Removed dependencies:
# - No touch/gesture specific crates
# - No screenshot/image processing crates 
# - No GUI settings frameworks
```

### Test Requirements (Updated)

1. **Monitor enumeration tests**:
   - Detect single/dual/triple monitor setups
   - Handle primary monitor detection
   - Test monitor by index/name selection
   
2. **Fullscreen transition tests**:
   - Enter/exit fullscreen reliably
   - F11 and Ctrl+Alt+F hotkeys
   - State preservation across transitions
   
3. **Scaling calculation tests**:
   - Fit/Fill/1:1 math with various aspect ratios
   - DPI scaling for high-resolution displays
   - Letterboxing and centering logic
   
4. **Multi-monitor tests**:
   - Move fullscreen between monitors
   - Ctrl+Alt+Arrow navigation
   - Mixed DPI environment handling
   
5. **CLI argument tests**:
   - `--fullscreen`, `--monitor`, `--scale` parsing
   - Invalid monitor fallback behavior
   - Environment variable integration

### Success Criteria (Updated)

**M1 (Fullscreen)**:
- ✅ F11 toggle works reliably across X11/Wayland
- ✅ CLI `--fullscreen` starts in fullscreen mode
- ✅ Borderless fullscreen with exclusive fallback
- ✅ DPI-aware scaling on high-resolution monitors
- ✅ Smooth transitions without flicker (<200ms)
- ✅ Scaling policies (fit/fill/1:1) work correctly

**M2 (Multi-monitor)**:
- ✅ Accurate monitor enumeration and selection
- ✅ CLI `--monitor primary|index|name` works
- ✅ Ctrl+Alt+Arrow hotkeys move between monitors
- ✅ Mixed DPI environments handled gracefully
- ✅ Monitor disconnect/reconnect recovery
- ✅ Clear error messages for invalid selections

**General**:
- ✅ Cross-platform consistency (X11/Wayland)
- ✅ Zero clippy warnings (no `allow(warnings)` or `allow(deprecated)` in production code)
  - Enforced via local runs of `cargo clippy -- -D warnings` (e.g., in pre-commit hooks)
- ✅ Clear configuration and error messages
- ✅ No defensive fallbacks (fail-fast policy maintained)

---

## 📋 Cross-Cutting Concerns

### Logging
- **Framework**: tracing with tracing-subscriber
- **Configuration**: env-configurable levels (RUST_LOG)
- **Structured logs**: use spans for request/connection tracking
- **Performance**: minimize allocations in hot paths

### Configuration Precedence
1. Command-line arguments (highest priority)
2. User configuration file (`~/.config/rvncviewer/config.toml`)
3. Built-in defaults (lowest priority)
- **Validation**: fail-fast on invalid values (no silent fallbacks)

### Error Handling
- **Policy**: Fail fast with clear, actionable error messages
- **No defensive fallbacks**: If something is wrong, exit with error
- **Context**: Use anyhow::Context for error chain propagation
- **User-facing**: Convert technical errors to user-friendly messages in GUI

### Performance Optimization
- **Hot path allocations**: Reuse buffers in rendering and decoding
- **Zero-copy where possible**: Use bytes::Bytes for network data
- **Profiling**: Use cargo-flamegraph to identify bottlenecks
- **Target**: 60 fps for 1080p, sub-100ms latency on local network

### Packaging (Post-Phase 8)
- **macOS app bundle**: Create .app with icon and Info.plist
- **CLI binary**: Install via cargo install or Homebrew
- **Documentation**: User guide and man page
- **Distribution**: GitHub releases with signed binaries

---

## 📅 Timeline Summary

Estimated timeline for single developer (can parallelize by crate):

| Phase | Description | Days | Cumulative |
|-------|-------------|------|------------|
| Phase 4 | Core Connection & Event Loop | 6-10 | 6-10 |
| Phase 5 | Display & Rendering | 5-8 | 11-18 |
| Phase 6 | Input Handling | 4-6 | 15-24 |
| Phase 7 | GUI Integration | 5-9 | 20-33 |
| Phase 8 | Advanced Features | 10-20 | 30-53 |

**Total**: 30-53 dev days (~6-11 weeks for single developer)

### Parallelization Opportunities
- Phase 5 and 6 can be developed simultaneously
- Phase 8 features can be implemented independently
- Multiple developers can work on different crates concurrently

---

## ✅ Definition of Done

The rvncviewer binary will be considered feature-complete when:

1. ✅ All phases 4-8 complete with passing tests
2. ✅ Feature parity with C++ vncviewer on macOS achieved
3. ✅ All workspace tests passing (unit + integration)
4. ✅ Zero clippy warnings across workspace
5. ✅ Documentation complete (README, user guide, API docs)
6. ✅ No defensive fallbacks (fail-fast policy maintained throughout)
7. ✅ Clear, contextual error messages for all error paths
8. ✅ Performance targets met (60 fps @ 1080p, <100ms latency)
9. ✅ Packaging and distribution ready (macOS app bundle + CLI)
10. ✅ Code review and security audit complete

---

## 📖 Reference Implementation

The C++ vncviewer implementation can be found in:
- `/Users/nickc/code/tigervnc/vncviewer/` - Main viewer code
- `/Users/nickc/code/tigervnc/common/rfb/` - Protocol implementation

Key files for reference:
- `vncviewer/vncviewer.cxx` - Main application loop
- `vncviewer/CConn.cxx` - Connection management
- `vncviewer/DesktopWindow.cxx` - Desktop rendering
- `vncviewer/Viewport.cxx` - Viewport handling
- `vncviewer/parameters.cxx` - Configuration parameters

---

**Ready to begin Phase 4!** 🚀

See PROGRESS.md for current status and completed phases.
