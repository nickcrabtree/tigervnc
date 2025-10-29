# Rust VNC Viewer - Roadmap

Development roadmap prioritizing fullscreen and multi-monitor support for desktop-focused VNC viewer.

## Milestone Overview

- **M0** (Current): Core VNC functionality stable
- **M1** (Priority): Enhanced fullscreen support
- **M2** (Priority): Multi-monitor support
- **M3** (Priority): PersistentCache protocol (immediately after M2)
- **M4+**: Windowed UX polish and advanced features

## Milestone M0: Foundation (COMPLETED)

**Status**: ✅ Complete  
**Scope**: Core desktop VNC viewer with basic functionality

### Completed Features
- ✅ Complete RFB protocol implementation (all standard encodings)
- ✅ ContentCache protocol (97-99% bandwidth reduction)
- ✅ Cross-platform GUI (egui/eframe)
- ✅ Basic keyboard/mouse input handling
- ✅ Bidirectional clipboard synchronization
- ✅ CLI-based configuration
- ✅ Production-ready error handling and logging

### Architecture Achievements
- ✅ Modular crate structure (8 crates, 320+ tests)
- ✅ Async tokio-based networking
- ✅ GPU-accelerated rendering pipeline
- ✅ Comprehensive test coverage

## Milestone M1: Enhanced Fullscreen Support (NEXT)

**Priority**: HIGH  
**Timeline**: 1-2 weeks  
**Goal**: Excellent single-monitor fullscreen experience

### Features

#### Core Fullscreen
- [x] **F11 toggle**: Reliable fullscreen entry/exit via F11 (implemented in rvncviewer)
- [x] **CLI start option**: `--fullscreen` flag for immediate fullscreen (implemented)
- [ ] **Borderless vs exclusive**: Intelligent mode selection with fallback (pending)
- [ ] **State preservation**: Remember and restore windowed position/size (pending)

#### DPI and Scaling
- [x] **Per-monitor DPI**: Detection wired via winit enumeration (used for logging; application pending)
- [ ] **High-DPI support**: Crisp rendering on Retina/4K displays
- [ ] **Scaling policies**: Fit (letterbox), Fill, 1:1 with quality preservation
- [ ] **Aspect ratio**: Configurable aspect ratio preservation

#### Keyboard Shortcuts
- [x] **F11**: Primary fullscreen toggle
- [x] **Ctrl+Alt+F**: Alternative fullscreen toggle
- [ ] **Esc**: Optional fullscreen exit (configurable)
- [ ] **F1**: Connection info overlay in fullscreen

### Acceptance Criteria
- [ ] Smooth fullscreen transitions (<200ms)
- [ ] No flicker or visual artifacts
- [ ] Correct scaling on common monitor types
- [ ] Reliable state transitions (windowed ↔ fullscreen)
- [ ] Cross-platform consistency (X11/Wayland)

### Manual QA Checklist
- [ ] Standard 1920x1080 monitor
- [ ] 4K monitor at 150% scaling
- [ ] Ultrawide 21:9 monitor
- [ ] Remote desktop larger than local screen
- [ ] Remote desktop smaller than local screen

## Milestone M2: Multi-Monitor Support (PRIORITY)

**Priority**: HIGH  
**Timeline**: 1-2 weeks (after M1)  
**Goal**: Seamless multi-monitor fullscreen experience

### Features

#### Monitor Enumeration
- [x] **Detect all monitors**: Enumerate available displays with metadata (rvncviewer/display)
- [x] **Primary detection**: Identify primary monitor reliably  
- [x] **Monitor metadata**: Name, resolution, DPI (position pending)
- [ ] **Deterministic ordering**: Consistent monitor indexing across runs (basic ordering implemented)

#### Monitor Selection
- [x] **CLI selection**: `--monitor primary|index|name` option (parsed and stored)
- [ ] **Runtime switching**: Hotkeys to move between monitors
- [x] **Fallback handling**: Graceful handling of missing monitors (defaults to primary)
- [ ] **Hotplug support**: Monitor connect/disconnect detection

#### Multi-Monitor Navigation
- [x] **Ctrl+Alt+←/→**: Move fullscreen to prev/next monitor
- [x] **Ctrl+Alt+0-9**: Jump to monitor by index
- [x] **Ctrl+Alt+P**: Jump to primary monitor
- [ ] **Visual feedback**: Brief overlay showing target monitor (pending)

### Acceptance Criteria
- [ ] Accurate enumeration of 2-4 monitor setups
- [ ] Smooth movement between monitors without artifacts
- [ ] Mixed DPI handling (different scaling factors)
- [ ] Persistent monitor preferences across sessions
- [ ] Clear error messages for invalid selections

### Manual QA Matrix
| Configuration | Enumeration | Selection | Hotkeys | Mixed DPI |
|---------------|-------------|-----------|---------|-----------|
| Dual 1080p | ✓ | ✓ | ✓ | N/A |
| Dual mixed DPI | ✓ | ✓ | ✓ | ✓ |
| Triple setup | ✓ | ✓ | ✓ | ✓ |
| Portrait mode | ✓ | ✓ | ✓ | ✓ |

### Stretch Goals
- [ ] **Monitor memory**: Remember last used monitor per connection
- [ ] **Position awareness**: Logical monitor positioning for navigation
- [ ] **Span support**: Documentation for multi-monitor spanning (implementation deferred)

## Milestone M3: PersistentCache Protocol (PRIORITY)

**Priority**: HIGH  
**Timeline**: 1-2 weeks (after M2)  
**Goal**: Persistent, hash-based caching for cross-session and cross-server cache hits

### Overview

The C++ TigerVNC viewer recently implemented the **PersistentCache** protocol, a new RFB extension that uses content hashes (SHA-256) as stable cache keys. This enables:
- **Cross-session persistence**: Cache survives client restarts
- **Cross-server hits**: Same content on different servers reuses cached data
- **97-99% bandwidth reduction**: Even better than ContentCache for repeated content

### Features

#### Protocol Foundation
- [ ] **Protocol constants**: Pseudo-encoding `-321`, encodings `102` and `103`
- [ ] **Message types**: `254` (PersistentCacheQuery), `253` (PersistentHashList)
- [ ] **Content hashing**: SHA-256 truncated to 128 bits (16 bytes)
- [ ] **Wire format**: Variable-length hash encoding for efficiency

#### Cache Storage
- [ ] **GlobalClientPersistentCache**: Hash-indexed storage with ARC eviction
- [ ] **ARC algorithm**: Adaptive replacement with T1, T2, B1, B2 lists
- [ ] **Size management**: Configurable limit (default: 2GB), byte-accurate accounting
- [ ] **Statistics**: Hits, misses, evictions, memory usage tracking

#### Client Protocol
- [ ] **Decoders**: PersistentCachedRect (102), PersistentCachedRectInit (103)
- [ ] **Query batching**: Efficient miss handling with batch requests
- [ ] **Hash list**: Initial synchronization of known hashes
- [ ] **Negotiation**: Prefer PersistentCache over ContentCache in SetEncodings

#### Disk Persistence
- [ ] **File format**: `~/.cache/tigervnc/persistentcache.dat` with 64-byte header
- [ ] **Load/Save**: Automatic persistence on startup/shutdown
- [ ] **Corruption handling**: Graceful recovery from invalid cache files
- [ ] **Checksum**: SHA-256 verification for integrity

### Implementation Phases

| Phase | Duration | Description |
|-------|----------|--------------|
| PC-1 | 0.5 day | Protocol constants, ContentHash utility |
| PC-2 | 2 days | GlobalClientPersistentCache with ARC |
| PC-3 | 1 day | Client protocol messages and decoders |
| PC-4 | 1 day | Integration and negotiation |
| PC-5 | 1-2 days | Disk persistence implementation |
| PC-6 | 1 day | Testing and validation |

### Dependencies

**Cargo dependencies:**
```toml
sha2 = "0.10"          # SHA-256 hashing
byteorder = "1"        # Binary I/O
indexmap = "2"         # Ordered maps for ARC
directories = "5"      # XDG cache directory
```

### Acceptance Criteria
- [ ] Protocol negotiation prefers PersistentCache when both are available
- [ ] Hash computation matches C++ ContentHash::computeRect
- [ ] ARC eviction maintains configured size limits
- [ ] Disk persistence survives restarts without data loss
- [ ] Cross-session hits verified with e2e test framework (:999)
- [ ] Performance: <1ms hash computation, <200ms disk load/save

### Testing Strategy
- **Unit tests**: Hash computation, ARC eviction, disk round-trip
- **Integration tests**: Mock server with encoding 102/103 sequences
- **Cross-session**: Connect, disconnect, reconnect - verify cache hits
- **Performance**: Benchmark hashing and I/O operations
- **Safety**: Only use Xnjcvnc :2 per WARP.md (never Xtigervnc :1 or :3)

### Technical Notes

**Critical: Stride handling**
```rust
// Stride is in PIXELS, not bytes!
let stride_bytes = stride_pixels * bytes_per_pixel;
```

**Hash stability**: Must match C++ implementation exactly for interoperability.

**Protocol preference**: Include both `-321` and `-320` in SetEncodings, server chooses PersistentCache if supported.

### Related Documentation
- C++ design: `/home/nickc/code/tigervnc/PERSISTENTCACHE_DESIGN.md`
- Rust implementation guide: `docs/protocol/PERSISTENTCACHE_RUST.md` (to be created)
- ARC algorithm: `/home/nickc/code/tigervnc/ARC_ALGORITHM.md`

## Milestone M4: Windowed UX Polish (FUTURE)

**Priority**: MEDIUM  
**Timeline**: Post-M2  
**Goal**: Enhanced windowed mode experience

### Features
- [ ] **Window state memory**: Remember size/position per connection
- [ ] **Smart initial sizing**: Intelligent default window dimensions
- [ ] **Minimize to tray**: Optional system tray integration
- [ ] **Connection management**: Recent connections, favorites
- [ ] **Status indicators**: Connection quality, latency display
- [ ] **Theme support**: Dark/light mode preference

## Milestone M5: Advanced Features (FUTURE)

**Priority**: LOW  
**Timeline**: Post-M4  
**Goal**: Power user features and optimizations

### Features
- [ ] **Performance monitoring**: Detailed bandwidth/latency metrics
- [ ] **Connection profiles**: Save/load connection configurations (CLI-based)
- [ ] **Encoding preferences**: Per-connection encoding selection
- [ ] **Security enhancements**: TLS support, certificate management
- [ ] **Accessibility**: Screen reader support, high-contrast mode

## Out-of-Scope Features

The following features are **explicitly excluded** from all milestones per [SEP-0001](SEP/SEP-0001-out-of-scope.md):

### Permanently Out-of-Scope
- **Touch/Gesture support**: Desktop-only focus; use trackpad scrolling
- **Settings UI/Profiles GUI**: Use CLI configuration and environment variables
- **Screenshot functionality**: Use OS-native tools (gnome-screenshot, grim, etc.)

### Rationale
These features add complexity without proportional value for desktop users. CLI configuration and OS integration provide better solutions.

## Dependencies and Risks

### Technical Dependencies
- **winit**: Cross-platform window management and monitor enumeration
- **egui/eframe**: GUI framework for consistent behavior
- **Platform support**: X11 and Wayland compatibility

### Risk Mitigation
- **X11/Wayland differences**: Comprehensive testing on both platforms
- **Monitor API variations**: Fallback strategies for unsupported features
- **Hardware quirks**: Graceful handling of unusual monitor configurations

### Testing Strategy
- **Unit tests**: Monitor selection logic, scaling calculations
- **Integration tests**: Fullscreen transitions, multi-monitor movement
- **Manual QA**: Real hardware testing with various monitor configurations
- **VNC server compatibility**: Test with the e2e framework (displays :998/:999)

## Timeline Summary

| Milestone | Duration | Dependencies | Status |
|-----------|----------|--------------|---------|
| M0 | - | - | ✅ Complete |
| M1 | 1-2 weeks | winit, testing | 🎯 Next |
| M2 | 1-2 weeks | M1 complete | 📋 Planned |
| M3 | 1-2 weeks | M2 complete | 📋 Planned |
| M4 | 2-3 weeks | M3 complete | 💭 Future |
| M5 | TBD | M4 complete | 💭 Future |

**Total estimate for M1+M2+M3**: 3-6 weeks for core fullscreen, multi-monitor, and PersistentCache functionality.

## Success Metrics

### M1 Success
- [ ] Fullscreen "just works" on single monitor systems
- [ ] Zero reported issues with common desktop environments
- [ ] User feedback confirms smooth, reliable experience

### M2 Success  
- [ ] Multi-monitor users can easily select and switch monitors
- [ ] Hotkey navigation feels intuitive and responsive
- [ ] Mixed DPI environments handled correctly

### M3 Success
- [ ] PersistentCache protocol fully functional with hash-based caching
- [ ] Cross-session persistence verified through restart testing
- [ ] Performance meets targets (<1ms hashing, <200ms I/O)
- [ ] Cache interoperability with C++ viewer confirmed

### Overall Project Success
- [ ] Competitive feature parity with C++ vncviewer for desktop use
- [ ] Demonstrably better performance (ContentCache benefits)
- [ ] Positive user adoption in desktop VNC scenarios

---

**Related Documents**: [SEP-0001 Out-of-Scope](SEP/SEP-0001-out-of-scope.md), [CLI Usage](cli/USAGE.md), [Fullscreen & Multi-Monitor Spec](spec/fullscreen-and-multimonitor.md)