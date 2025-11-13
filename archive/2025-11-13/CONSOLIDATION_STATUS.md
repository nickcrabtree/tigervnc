# Rust Viewer Consolidation Status

**Date**: 2025-10-25  
**Status**: Phase 1 Complete - Makefile target added

## Problem Identified

The repository contained **two separate Rust VNC viewer implementations**:

1. **`njcvncviewer-rs`** (1,149 lines) - Main production viewer with full RFB protocol, ContentCache support, comprehensive features
2. **`rvncviewer`** (903 lines) - Experimental viewer created for M1 milestone fullscreen/multi-monitor feature prototyping (October 2024)

This duplication causes confusion about which viewer is canonical and creates maintenance burden.

## Phase 1: Immediate Fix (COMPLETED ✅)

### What Was Done

1. **Added `make rust_viewer` target** to top-level Makefile
   - Builds `njcvncviewer-rs` (the canonical Rust viewer)
   - Creates symlink at `build/vncviewer/njcvncviewer-rs`
   - Documented in WARP.md

2. **Verified build system**
   - Workspace builds successfully
   - Binary runs and shows version
   - Symlink created correctly

### Result

Users can now build the Rust viewer with:
```bash
make rust_viewer
```

Binary locations:
- **Actual binary**: `rust-vnc-viewer/target/release/njcvncviewer-rs`
- **Symlink**: `build/vncviewer/njcvncviewer-rs`

## Phase 2: Full Consolidation (TODO)

The full consolidation plan includes these remaining steps:

### 1. Port M1 Features from rvncviewer → njcvncviewer-rs

**Features to port:**

- **Display enumeration** (`rvncviewer/src/display/mod.rs`)
  - `enumerate_monitors()` - Lists available monitors with metadata
  - `select_monitor()` - Selects monitor by "primary", index, or name

- **Fullscreen controller** (`rvncviewer/src/fullscreen/mod.rs`)
  - `FullscreenController` - Manages fullscreen state and target monitor
  - `toggle()` - Toggle fullscreen on/off
  - `next_monitor()` / `prev_monitor()` - Navigate between monitors
  - `jump_to_monitor()` - Jump to specific monitor by index
  - `jump_to_primary()` - Jump to primary monitor

- **Keyboard hotkeys** (from `rvncviewer/src/app.rs`)
  - F11 or Ctrl+Alt+F - Toggle fullscreen
  - Ctrl+Alt+← / → - Navigate to prev/next monitor
  - Ctrl+Alt+0-9 - Jump to monitor by index
  - Ctrl+Alt+P - Jump to primary monitor

- **CLI flags**
  - `--monitor` - Select target monitor for fullscreen

### 2. Integration Design

Organize platform-specific code:
```
njcvncviewer-rs/src/
├── display/
│   └── mod.rs           # Cross-platform display enumeration (uses winit)
├── fullscreen/
│   └── mod.rs           # Cross-platform fullscreen controller
├── app.rs               # Integrate fullscreen controller + hotkeys
└── main.rs              # Add --monitor CLI flag
```

### 3. Testing Matrix

- **Linux**: Verify no regressions, basic fullscreen works
- **macOS**: Verify display enumeration, monitor selection, fullscreen navigation

### 4. Cleanup

- Remove `rvncviewer` crate from workspace
- Update all documentation to reference only `njcvncviewer-rs`
- Remove `rvncviewer` from docs:
  - `README.md`
  - `docs/ROADMAP.md`
  - `docs/testing/M1_FULLSCREEN_TESTING.md`
  - `M1_QUICKREF.md`, `M1_SUMMARY.md`, etc.

## Key Files

### Current State

**Canonical Rust Viewer**: `njcvncviewer-rs/`
- Main binary crate with full VNC client implementation
- ContentCache protocol support
- Basic fullscreen support via `--fullscreen` CLI flag

**Experimental Viewer**: `rvncviewer/`
- Contains M1 fullscreen/multi-monitor prototypes
- Should be removed after feature migration

### Build System

**Makefile** (`/home/nickc/code/tigervnc/Makefile`)
- Added `rust_viewer` target (lines 33-40)
- Builds with: `cargo build --manifest-path rust-vnc-viewer/Cargo.toml --release -p njcvncviewer-rs`

**Documentation** (`WARP.md`)
- Updated to document `make rust_viewer` target
- Clarifies C++ viewer vs Rust viewer

## TODO List

See the active TODO list (10 remaining items) for detailed consolidation steps:

1. ~~Create consolidation branch and baseline checks~~ ✅
2. ~~Locate both Rust viewer crates~~ ✅
3. ~~Audit feature differences~~ ✅
4. Design integration point in njcvncviewer-rs
5. Port display enumeration
6. Port fullscreen control
7. Align dependencies
8. End-to-end testing matrix
9. Remove rvncviewer crate
10. Update documentation
11. ~~Add Makefile rust_viewer target~~ ✅
12. ~~Verify build system~~ ✅
13. Adjust CI scripts
14. Clean up and open PR
15. Post-merge housekeeping

## References

- **Current Work**: Development on main branch (no branching required)
- **WARP.md**: Build system documentation
- **Workspace Metadata**: Use `cargo metadata --no-deps` to list all crates
- **Feature Comparison**: See `rvncviewer/src/{display,fullscreen}/mod.rs` vs `njcvncviewer-rs`

## Next Steps

When ready to complete consolidation:

1. Start with TODO item #4 (Design integration point)
2. Copy display/fullscreen modules from rvncviewer
3. Update njcvncviewer-rs/src/main.rs to add modules and --monitor flag
4. Update njcvncviewer-rs/src/app.rs to integrate fullscreen controller
5. Test on Linux and macOS
6. Remove rvncviewer crate
7. Update all documentation

**Estimated effort**: 1-2 days for full consolidation
