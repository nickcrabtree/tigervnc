# M1 Fullscreen and Multi-Monitor Testing Guide

**Version**: 2025-10-24  
**Milestone**: M1 (Fullscreen enhancements)  
**Status**: Features implemented; per-monitor placement pending

## Overview

This guide covers testing procedures for the fullscreen and multi-monitor features added in Milestone M1.

## Prerequisites

### Hardware Requirements
- **Minimum**: Single monitor for fullscreen testing
- **Recommended**: 2-3 monitors for multi-monitor feature testing
- **Ideal**: Mixed DPI setup (e.g., 1080p + 4K) to test scaling

### Software Requirements
- Ubuntu/Linux with X11 or Wayland
- TigerVNC server running (see [WARP.md](../../WARP.md) for safe server configuration)
- Rust toolchain (1.70+)

### Server Safety
Per [WARP.md](../../WARP.md), only connect to the test server:
```bash
# SAFE: Test server Xnjcvnc :2 (port 5902)
ssh nickc@birdsurvey.hopto.org "ps aux | grep 'Xnjcvnc :2'"

# Never test against production servers :1 or :3
```

## Build and Setup

### 1. Build the Viewer
```bash
cd ~/code/tigervnc/rust-vnc-viewer
cargo build --release --package rvncviewer
```

Expected: Clean build with warnings only (no errors)

### 2. Verify Binary
```bash
./target/release/rvncviewer --version
./target/release/rvncviewer --help
```

Expected: Version info and help text with `--fullscreen`, `--monitor` options

## Test Cases

### TC1: Monitor Enumeration

**Objective**: Verify monitors are detected at startup

**Steps**:
```bash
RUST_LOG=rvncviewer=info ./target/release/rvncviewer
```

**Expected Output**:
```
INFO rvncviewer::display: Detected 2 monitor(s)
INFO rvncviewer::display:   Monitor 0: 'DP-1' 1920x1080 @1.0x (primary)
INFO rvncviewer::display:   Monitor 1: 'HDMI-A-1' 2560x1440 @1.5x
```

**Validation**:
- [ ] Monitor count matches actual hardware
- [ ] Primary monitor identified correctly
- [ ] DPI scale factors are reasonable (1.0, 1.25, 1.5, 2.0 typical)
- [ ] Monitor names match `xrandr` output (X11) or compositor (Wayland)

---

### TC2: CLI Fullscreen

**Objective**: Verify `--fullscreen` flag starts viewer in fullscreen

**Steps**:
```bash
./target/release/rvncviewer --fullscreen localhost:2
```

**Expected Behavior**:
- [ ] Viewer starts in fullscreen mode immediately
- [ ] No window borders visible
- [ ] Covers entire screen
- [ ] Connection dialog appears (if no server specified)

**Notes**:
- Default monitor is primary (window manager decides)
- Per-monitor placement pending (known limitation)

---

### TC3: CLI Monitor Selection

**Objective**: Verify `--monitor` option parses and logs target

**Steps**:
```bash
# Test 1: Primary
RUST_LOG=rvncviewer=info ./target/release/rvncviewer --fullscreen --monitor primary localhost:2

# Test 2: By index
RUST_LOG=rvncviewer=info ./target/release/rvncviewer --fullscreen --monitor 1 localhost:2

# Test 3: By name substring
RUST_LOG=rvncviewer=info ./target/release/rvncviewer --fullscreen --monitor HDMI localhost:2
```

**Expected Output** (Test 1):
```
INFO rvncviewer::fullscreen: Fullscreen target monitor: 0 'DP-1', 1920x1080 @1.0x
```

**Validation**:
- [ ] Target monitor logged correctly
- [ ] Fallback to primary if selector not found
- [ ] Warning logged for invalid selectors

**Known Limitation**:
- Fullscreen may not actually move to target monitor (placement pending)
- Target is stored and used for hotkey navigation

---

### TC4: F11 Fullscreen Toggle

**Objective**: Verify F11 toggles fullscreen mode

**Steps**:
1. Start viewer in windowed mode: `./target/release/rvncviewer localhost:2`
2. Press `F11`
3. Wait for fullscreen transition
4. Press `F11` again

**Expected Behavior**:
- [ ] First F11: Transitions to fullscreen (<500ms)
- [ ] Second F11: Returns to windowed mode
- [ ] Window size/position preserved on exit (if implemented)
- [ ] No flicker or artifacts during transition

---

### TC5: Ctrl+Alt+F Fullscreen Toggle

**Objective**: Verify alternative fullscreen hotkey

**Steps**:
1. Start viewer: `./target/release/rvncviewer localhost:2`
2. Press `Ctrl+Alt+F`

**Expected Behavior**:
- [ ] Toggles fullscreen (same as F11)
- [ ] Works consistently across platforms

---

### TC6: Monitor Navigation (Left/Right)

**Objective**: Verify Ctrl+Alt+Arrow keys cycle through monitors

**Prerequisites**: 2+ monitors

**Steps**:
1. Start in fullscreen: `./target/release/rvncviewer --fullscreen localhost:2`
2. Press `Ctrl+Alt+→` (right arrow)
3. Observe log output and behavior
4. Press `Ctrl+Alt+←` (left arrow)

**Expected Logs**:
```
INFO rvncviewer::fullscreen: Switched to monitor 1: 'HDMI-A-1'
INFO rvncviewer::fullscreen: Switched to monitor 0: 'DP-1'
```

**Expected Behavior**:
- [ ] Logs indicate monitor switch
- [ ] Cycles through monitors (wraps around at end)
- [ ] No crash or hang

**Known Limitation**:
- Window may not actually move to target monitor (placement pending)

---

### TC7: Direct Monitor Jump (Ctrl+Alt+0-9)

**Objective**: Verify numeric hotkeys jump to specific monitors

**Prerequisites**: 2+ monitors (index 0, 1, etc.)

**Steps**:
1. Start in fullscreen: `./target/release/rvncviewer --fullscreen localhost:2`
2. Press `Ctrl+Alt+1`
3. Observe log output
4. Press `Ctrl+Alt+0`

**Expected Logs**:
```
INFO rvncviewer::fullscreen: Jumped to monitor 1: 'HDMI-A-1'
INFO rvncviewer::fullscreen: Jumped to monitor 0: 'DP-1'
```

**Validation**:
- [ ] Hotkey triggers monitor switch
- [ ] Invalid indices logged with warning
- [ ] State persists across toggles

---

### TC8: Jump to Primary (Ctrl+Alt+P)

**Objective**: Verify primary monitor hotkey

**Steps**:
1. Start fullscreen on secondary: `./target/release/rvncviewer --fullscreen --monitor 1 localhost:2`
2. Press `Ctrl+Alt+P`

**Expected Logs**:
```
INFO rvncviewer::fullscreen: Jumped to primary monitor: 'DP-1'
```

**Validation**:
- [ ] Switches to primary monitor
- [ ] Works even if already on primary (idempotent)

---

### TC9: VNC_PASSWORD Environment Variable

**Objective**: Verify password from environment

**Steps**:
```bash
# Set password in environment
export VNC_PASSWORD=testpass123

# Connect (password should not appear in ps output)
./target/release/rvncviewer localhost:2
```

**Validation**:
- [ ] Connection succeeds without `--password` flag
- [ ] Password not visible in shell history
- [ ] `ps aux | grep rvncviewer` does not show password

**Security Check**:
```bash
# In another terminal while viewer is running
ps aux | grep rvncviewer | grep -v grep
```
Expected: Password NOT visible in command line

---

### TC10: Fallback Behavior

**Objective**: Verify graceful fallback for invalid selectors

**Steps**:
```bash
RUST_LOG=rvncviewer=info ./target/release/rvncviewer --fullscreen --monitor 99 localhost:2
```

**Expected Output**:
```
WARN rvncviewer::fullscreen: Monitor index 99 not found
INFO rvncviewer::fullscreen: Fullscreen target monitor: 0 'DP-1', 1920x1080 @1.0x (primary)
```

**Validation**:
- [ ] Warning logged for invalid selector
- [ ] Falls back to primary monitor
- [ ] Viewer still starts in fullscreen

---

## Manual QA Checklist

### Single Monitor Setup
- [ ] TC1: Monitor enumeration (1 monitor expected)
- [ ] TC2: CLI fullscreen works
- [ ] TC4: F11 toggle works
- [ ] TC5: Ctrl+Alt+F toggle works
- [ ] TC9: VNC_PASSWORD works

### Dual Monitor Setup
- [ ] TC1: Monitor enumeration (2 monitors expected)
- [ ] TC3: CLI monitor selection (primary, 0, 1, name)
- [ ] TC6: Left/Right navigation cycles between monitors
- [ ] TC7: Ctrl+Alt+0/1 direct jumps
- [ ] TC8: Ctrl+Alt+P returns to primary

### Triple+ Monitor Setup
- [ ] TC1: Monitor enumeration (all monitors detected)
- [ ] TC6: Left/Right navigation cycles through all monitors
- [ ] TC7: Ctrl+Alt+0-9 for monitors 0-9
- [ ] Monitor ordering is deterministic across runs

### Mixed DPI Setup
- [ ] TC1: Monitor enumeration shows different scale factors
- [ ] TC6: Navigation between monitors logs DPI correctly
- [ ] Rendering quality consistent across monitors (visual check)

## Known Issues and Limitations

### Per-Monitor Fullscreen Placement
**Issue**: Window may not move to target monitor when hotkeys are pressed  
**Status**: Pending eframe/egui integration  
**Workaround**: Target is logged and stored; placement will work once integrated  
**Tracking**: M1 high-priority item

### DPI-Aware Scaling
**Issue**: Detected DPI not applied to rendering calculations  
**Status**: Enumeration complete; scaling logic pending  
**Impact**: May see blurriness on high-DPI monitors in fullscreen  
**Tracking**: M1 high-priority item

### State Preservation
**Issue**: Window size/position not saved before fullscreen  
**Status**: Not implemented  
**Impact**: Exiting fullscreen may not restore original window state  
**Tracking**: M1 medium-priority item

### Wayland Compositor Variations
**Issue**: Some Wayland compositors override fullscreen behavior  
**Status**: Expected; Wayland spec allows compositor control  
**Workaround**: Use X11 or compositor with standard fullscreen support  
**Tracking**: Platform-specific; documented limitation

## Performance Metrics

### Fullscreen Transition Latency
**Target**: <200ms for smooth transitions  
**Measurement**: Visual observation or `perf` tooling  
**Acceptable**: <500ms on older hardware

### Monitor Enumeration Time
**Target**: <50ms at startup  
**Measurement**: Log timestamps  
**Acceptable**: <100ms

### Hotkey Response Time
**Target**: Immediate (<50ms perceived delay)  
**Measurement**: User perception  
**Acceptable**: <100ms

## Regression Testing

After each change to display/fullscreen modules, re-run:
- TC1 (enumeration)
- TC2 (CLI fullscreen)
- TC4 (F11 toggle)
- TC6 (navigation, if multi-monitor available)

## Bug Reporting Template

When filing issues, include:
```
**Environment**:
- OS: Ubuntu 22.04 LTS
- Display Server: X11 / Wayland
- Monitors: 2x 1920x1080 @ 1.0x scale
- Rust version: 1.70.0
- Build: cargo build --release

**Test Case**: TC6 (Monitor Navigation)

**Steps to Reproduce**:
1. Start fullscreen: ./target/release/rvncviewer --fullscreen localhost:2
2. Press Ctrl+Alt+→

**Expected**: Switch to monitor 1
**Actual**: [describe behavior]

**Logs**:
[paste relevant log output]

**Screenshots/Video**: [if applicable]
```

## Next Steps

After completing M1 testing:
1. Address per-monitor placement (high priority)
2. Implement DPI-aware scaling
3. Add state preservation
4. Move to M2 testing (advanced multi-monitor features)

---

**Related Documents**:
- [ROADMAP.md](../ROADMAP.md) - M1/M2 milestones
- [fullscreen-and-multimonitor.md](../spec/fullscreen-and-multimonitor.md) - Technical spec
- [USAGE.md](../cli/USAGE.md) - CLI reference
