# M1 Fullscreen/Multi-Monitor Quick Reference

**Version**: 2025-10-24  
**Implementation**: Phase 1 (~60% complete)

## Command-Line Options

```bash
# Basic fullscreen
rvncviewer --fullscreen localhost:999

# Target specific monitor
rvncviewer --fullscreen --monitor primary localhost:999
rvncviewer --fullscreen --monitor 1 localhost:999
rvncviewer --fullscreen --monitor HDMI localhost:999

# With password (secure)
VNC_PASSWORD=secret rvncviewer --fullscreen localhost:999

# With verbose logging
RUST_LOG=rvncviewer=info rvncviewer --fullscreen localhost:999

# All options combined
RUST_LOG=info VNC_PASSWORD=secret rvncviewer \
  --fullscreen --monitor 1 localhost:999
```

## Keyboard Shortcuts

| Shortcut | Action | Active When |
|----------|--------|-------------|
| `F11` | Toggle fullscreen | Always |
| `Ctrl+Alt+F` | Toggle fullscreen (alt) | Always |
| `Ctrl+Alt+←` | Previous monitor | Fullscreen only |
| `Ctrl+Alt+→` | Next monitor | Fullscreen only |
| `Ctrl+Alt+0` | Jump to monitor 0 | Fullscreen only |
| `Ctrl+Alt+1` | Jump to monitor 1 | Fullscreen only |
| `Ctrl+Alt+2-9` | Jump to monitor 2-9 | Fullscreen only |
| `Ctrl+Alt+P` | Jump to primary | Fullscreen only |

## Monitor Selection

| Selector | Example | Match Behavior |
|----------|---------|----------------|
| `primary` | `--monitor primary` | Primary monitor (default) |
| Index | `--monitor 0`, `--monitor 1` | Zero-based index |
| Name | `--monitor HDMI`, `--monitor DP-1` | Case-insensitive substring match |

## Build Commands

```bash
# Debug build
cd ~/code/tigervnc/rust-vnc-viewer
cargo build --package rvncviewer

# Release build (optimized)
cargo build --release --package rvncviewer

# Run directly (debug)
cargo run --package rvncviewer -- localhost:999

# Run release binary
./target/release/rvncviewer localhost:999
```

## Debugging

```bash
# View monitor enumeration
RUST_LOG=rvncviewer=info ./target/release/rvncviewer

# Full debug logs
RUST_LOG=debug ./target/release/rvncviewer localhost:999

# Trace everything
RUST_LOG=trace ./target/release/rvncviewer localhost:999
```

## Expected Log Output

```
INFO rvncviewer::display: Detected 2 monitor(s)
INFO rvncviewer::display:   Monitor 0: 'DP-1' 1920x1080 @1.0x (primary)
INFO rvncviewer::display:   Monitor 1: 'HDMI-A-1' 2560x1440 @1.5x
INFO rvncviewer::fullscreen: Fullscreen target monitor: 1 'HDMI-A-1', 2560x1440 @1.5x
INFO rvncviewer::fullscreen: Switched to monitor 0: 'DP-1'
```

## Known Issues

⚠️ **Per-Monitor Placement**: Window may not move to target monitor (eframe limitation)  
⚠️ **DPI Scaling**: Detected but not applied to rendering  
⚠️ **State Preservation**: Window state not saved across fullscreen transitions

## Quick Tests

```bash
# TC1: Verify monitor enumeration
RUST_LOG=info ./target/release/rvncviewer

# TC2: Test F11 toggle
./target/release/rvncviewer localhost:999
# Press F11 twice

# TC3: Test monitor navigation (2+ monitors required)
./target/release/rvncviewer --fullscreen localhost:999
# Press Ctrl+Alt+→, observe logs

# TC4: Test VNC_PASSWORD
VNC_PASSWORD=test ./target/release/rvncviewer localhost:999
# In another terminal: ps aux | grep rvncviewer
# Verify password is NOT visible
```

## Files Modified

### Source Code
- `rvncviewer/src/display/mod.rs` ← NEW (monitor enumeration)
- `rvncviewer/src/fullscreen/mod.rs` ← NEW (fullscreen controller)
- `rvncviewer/src/args.rs` (CLI options)
- `rvncviewer/src/app.rs` (integration, hotkeys)
- `rvncviewer/src/lib.rs` (module exports)

### Documentation
- `docs/ROADMAP.md` (M1/M2 checklists)
- `docs/spec/fullscreen-and-multimonitor.md` (status)
- `docs/cli/USAGE.md` (CLI reference)
- `docs/testing/M1_FULLSCREEN_TESTING.md` ← NEW (test guide)
- `README.md` (progress)

### Progress Reports
- `PROGRESS_2025-10-24_M1_INITIAL.md` ← NEW (detailed log)
- `M1_SUMMARY.md` ← NEW (implementation summary)
- `M1_QUICKREF.md` ← NEW (this file)

## Next Priority

1. **Per-monitor placement**: Research eframe/egui integration
2. **DPI-aware scaling**: Apply scale_factor to rendering
3. **Manual testing**: Test on dual/triple monitor setup

## Help

- Full docs: [M1_SUMMARY.md](M1_SUMMARY.md)
- Test guide: [docs/testing/M1_FULLSCREEN_TESTING.md](docs/testing/M1_FULLSCREEN_TESTING.md)
- Roadmap: [docs/ROADMAP.md](docs/ROADMAP.md)
- Build guide: [WARP.md](../WARP.md)
