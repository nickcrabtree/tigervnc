# Fullscreen and Multi-Monitor Specification

Technical specification for reliable fullscreen display and multi-monitor support in the Rust VNC Viewer.

## Goals

- **Reliable fullscreen**: Seamless fullscreen mode on a chosen monitor
- **Monitor enumeration**: Accurate detection and selection of available displays  
- **Cross-platform**: Consistent behavior on X11 and Wayland
- **Performance**: Smooth transitions without flicker or delays

## Non-Goals

Per [SEP-0001](../SEP/SEP-0001-out-of-scope.md), the following are explicitly **out-of-scope**:
- Touch/gesture support
- Settings UI for monitor configuration  
- Built-in screenshot functionality

## Architecture

### Windowing Backend

**Primary**: winit crate for cross-platform window management
- Monitor enumeration via `EventLoop::available_monitors()` (implemented)
- Fullscreen control via `Window::set_fullscreen()` (pending integration via eframe)
- DPI handling via `MonitorHandle::scale_factor()` (enumeration implemented; applied scaling pending)

### Core Abstractions

#### DisplayManager
```rust
pub trait DisplayManager {
    fn list_monitors(&self) -> Vec<Monitor>;
    fn primary_monitor(&self) -> Option<Monitor>;
    fn get_monitor_by_index(&self, index: usize) -> Option<Monitor>;
    fn get_monitor_by_name(&self, name_substring: &str) -> Option<Monitor>;
}

pub struct Monitor {
    pub handle: MonitorHandle,
    pub index: usize,
    pub name: String,
    pub resolution: (u32, u32),
    pub scale_factor: f64,
    pub is_primary: bool,
}
```

#### FullscreenController
```rust
pub struct FullscreenController {
    current_state: FullscreenState,
    target_monitor: Option<Monitor>,
    display_manager: Box<dyn DisplayManager>,
}

impl FullscreenController {
    fn enter_fullscreen(&mut self, window: &Window, monitor: Option<Monitor>) -> Result<()>;
    fn exit_fullscreen(&mut self, window: &Window) -> Result<()>;
    fn toggle_fullscreen(&mut self, window: &Window) -> Result<()>;
    fn move_to_monitor(&mut self, window: &Window, monitor: Monitor) -> Result<()>;
}
```

#### Rendering Pipeline
- **Single swapchain**: One rendering surface per window
- **Framebuffer scaling**: Remote VNC framebuffer scaled to client window
- **Aspect ratio preservation**: Configurable via scaling policies

## Behavior Specification

### Fullscreen Modes

1. **Borderless Fullscreen** (default)
   - Window covers entire monitor without window decorations
   - Allows desktop switching and notifications
   - Compatible with all window managers

2. **Exclusive Fullscreen** (fallback)
   - Native fullscreen with display mode change (if supported)
   - Best performance for gaming/low-latency scenarios
   - Falls back to borderless if exclusive unavailable

### Monitor Selection

Selection priority (first match wins):
1. **Primary**: Explicitly requested primary monitor
2. **Index**: Zero-based index in deterministic order  
3. **Name substring**: Case-insensitive substring match
4. **Fallback**: Primary monitor if target not found

**Monitor Ordering**: Deterministic based on:
- Primary monitor first
- Secondary monitors sorted by position (left-to-right, top-to-bottom)
- Virtual displays last

### Fullscreen Transitions

Current implementation status:
- CLI `--fullscreen` and F11/Ctrl+Alt+F toggles are wired via egui viewport fullscreen.
- Target monitor is parsed and logged; per-monitor window placement is pending due to eframe integration limits.

#### Enter Fullscreen
1. Store current window state (size, position, decorations)
2. Select target monitor (via CLI arg or primary)
3. Configure window for fullscreen on target monitor
4. Update rendering viewport and scaling
5. Log transition details

#### Exit Fullscreen  
1. Restore previous window state
2. Recalculate scaling for windowed mode
3. Update rendering viewport
4. Log transition

#### Monitor Movement
1. Exit fullscreen on current monitor
2. Enter fullscreen on target monitor
3. Maintain scaling policy across transition
4. Handle edge cases (monitor disconnected, etc.)

### Scaling Policies

#### Fit (default)
- Scale remote framebuffer to fit entirely within window
- Preserve aspect ratio with letterboxing/pillarboxing
- Center image in available space

```rust
fn calculate_fit_scale(remote: (u32, u32), window: (u32, u32)) -> (f32, (f32, f32)) {
    let scale_x = window.0 as f32 / remote.0 as f32;
    let scale_y = window.1 as f32 / remote.1 as f32;
    let scale = scale_x.min(scale_y);
    let offset = (
        (window.0 as f32 - remote.0 as f32 * scale) / 2.0,
        (window.1 as f32 - remote.1 as f32 * scale) / 2.0,
    );
    (scale, offset)
}
```

#### Fill
- Scale remote framebuffer to fill entire window
- May crop or stretch based on `keep_aspect` setting
- No letterboxing/pillarboxing

#### 1:1 (Native)
- No scaling, 1:1 pixel mapping
- Enable panning if remote larger than window
- Optimal for pixel-perfect display

## Platform Implementation

### X11 (Linux)
- **Fullscreen**: Use EWMH `_NET_WM_STATE_FULLSCREEN`
- **Monitor detection**: XRandR extension for monitor enumeration
- **Primary monitor**: `_NET_WM_FULLSCREEN_MONITORS` or XRandR primary
- **DPI**: Per-monitor DPI via XRandR

### Wayland (Linux)
- **Fullscreen**: `set_fullscreen()` with specific `wl_output`
- **Monitor detection**: Via wl_output globals
- **Compositor limitations**: Some may override fullscreen behavior
- **DPI**: Per-output scale factors

### Monitor Spanning (Future)
Complex feature deferred to post-v1.0:
- Requires careful coordinate system mapping
- Window manager dependent on X11
- Compositor dependent on Wayland
- Significant complexity for limited benefit

## Error Handling

### Graceful Degradation
1. **Monitor not found**: Log warning, use primary monitor
2. **Exclusive fullscreen unsupported**: Fall back to borderless
3. **Monitor disconnected**: Detect and move to available monitor
4. **Invalid scaling parameters**: Use safe defaults with warning

### Error Recovery
- Window state corruption: Reset to windowed mode
- Rendering failures: Attempt surface recreation
- Monitor enumeration failure: Use last known configuration

## Telemetry and Logging

### Startup Logging
```
INFO  display: Detected 2 monitors
INFO  display:   Monitor 0: "HDMI-A-1" 1920x1080 @1.0x (primary)
INFO  display:   Monitor 1: "DP-1" 2560x1440 @1.5x
INFO  fullscreen: Entering fullscreen on monitor 0 (HDMI-A-1)
INFO  scaling: Using fit scaling: 0.85x with offset (120, 60)
```

### Runtime Events
```
DEBUG fullscreen: Toggling fullscreen (F11 pressed)
INFO  fullscreen: Moving to monitor 1 (Ctrl+Alt+Right)
WARN  display: Requested monitor "HDMI-2" not found, using primary
ERROR display: Monitor enumeration failed, using cached list
```

## Testing Strategy

### Unit Tests
- Monitor selection logic with mock displays
- Scaling calculations for various aspect ratios
- Fullscreen state transitions

### Integration Tests  
- Window creation and fullscreen toggling
- Monitor enumeration on test systems
- Keyboard shortcut handling

### Manual QA Matrix

| Scenario | Single Monitor | Dual Monitor | Mixed DPI |
|----------|---------------|--------------|-----------|
| Start windowed | ✓ | ✓ | ✓ |
| Start fullscreen | ✓ | ✓ | ✓ |
| Toggle F11 | ✓ | ✓ | ✓ |
| Ctrl+Alt+Arrow | N/A | ✓ | ✓ |
| Scale fit | ✓ | ✓ | ✓ |
| Scale fill | ✓ | ✓ | ✓ |
| Scale 1:1 | ✓ | ✓ | ✓ |

## Acceptance Criteria

### Functional
- [ ] Enumerate all available monitors with correct metadata
- [ ] Enter fullscreen on specified monitor via CLI
- [ ] Toggle fullscreen via F11 key
- [ ] Move fullscreen between monitors via hotkeys
- [ ] Handle monitor disconnect/reconnect gracefully
- [ ] Scale remote framebuffer according to policy
- [ ] Maintain aspect ratio when specified

### Performance
- [ ] Fullscreen transition < 200ms
- [ ] Monitor enumeration < 50ms
- [ ] Scaling calculations < 1ms
- [ ] No visible flicker during transitions

### Usability
- [ ] Clear error messages for invalid monitor selection
- [ ] Consistent behavior across X11/Wayland
- [ ] Keyboard shortcuts work reliably
- [ ] Logging provides useful debugging information

## Known Limitations

### X11
- Multi-monitor spanning depends on window manager support
- Some older WM may not support EWMH properly
- Virtual displays may report incorrect DPI

### Wayland
- Compositor-specific behavior variations
- Limited control over fullscreen implementation
- Monitor hotplug detection may vary

### Hardware
- Some displays report incorrect EDID information
- Multi-GPU systems may have enumeration quirks
- High-DPI scaling varies by desktop environment

---

**See Also**: [CLI Usage](../cli/USAGE.md), [SEP-0001 Out-of-Scope](../SEP/SEP-0001-out-of-scope.md)