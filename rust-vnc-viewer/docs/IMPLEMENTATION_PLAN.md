# Implementation Plan - Desktop-Focused VNC Viewer

Implementation roadmap for fullscreen and multi-monitor support in the Rust VNC viewer.

## Overview

Focus on delivering excellent **fullscreen** and **multi-monitor** support for desktop VNC workflows. Features outside this scope are explicitly out-of-scope per [SEP-0001](SEP/SEP-0001-out-of-scope.md).

## Module Architecture

### Proposed Module Structure

```
njcvncviewer-rs/src/
├── main.rs                         # Entry point, CLI parsing, app bootstrap
├── app.rs                          # Main application state and event loop
├── cli.rs                          # Extended CLI parsing (--fullscreen, --monitor, --scale)
├── display/
│   ├── mod.rs                      # DisplayManager trait and Monitor model
│   ├── winit_backend.rs            # Winit-based monitor enumeration implementation
│   └── monitor_selection.rs        # Primary/index/name selection logic
├── fullscreen/
│   ├── mod.rs                      # FullscreenController and state management
│   ├── transitions.rs              # Enter/exit/toggle logic with state preservation
│   └── hotkeys.rs                  # F11, Ctrl+Alt+Arrow, Ctrl+Alt+0-9 navigation
├── scaling/
│   ├── mod.rs                      # Scaling policies (fit/fill/1:1)
│   ├── calculations.rs             # Viewport and aspect ratio mathematics
│   └── dpi.rs                      # DPI handling for mixed environments
└── config.rs                       # Configuration management (CLI + env vars)
```

### Files to Remove/Avoid

**Explicitly excluded** to maintain focus:
- ❌ `src/touch.rs` or `src/gestures/` - Touch support out-of-scope
- ❌ `src/ui/settings/` or `src/profiles/` - Settings UI out-of-scope
- ❌ `src/screenshot.rs` or recording features - Use OS tools instead
- ❌ Configuration UI components - CLI-only configuration

### Core Types and Traits

```rust
// display/mod.rs
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
    pub position: (i32, i32),
}

// fullscreen/mod.rs
pub struct FullscreenController {
    current_state: FullscreenState,
    windowed_state: Option<WindowedState>,
    display_manager: Arc<dyn DisplayManager>,
}

#[derive(Debug, Clone)]
pub enum FullscreenState {
    Windowed,
    Fullscreen { monitor: Monitor, mode: FullscreenMode },
}

#[derive(Debug, Clone)]
pub enum FullscreenMode {
    Borderless,
    Exclusive,
}

// scaling/mod.rs
#[derive(Debug, Clone, Copy)]
pub enum ScalingPolicy {
    Fit,      // Scale to fit with letterboxing
    Fill,     // Scale to fill (may crop/stretch)
    Native,   // 1:1 pixel mapping
}

pub struct ScalingCalculator {
    policy: ScalingPolicy,
    keep_aspect: bool,
}
```

## Task Breakdown

### Task 1: CLI Enhancement (0.5-1 day)

**Scope**: Extend CLI argument parsing for fullscreen/multi-monitor options

**Files**: 
- `src/cli.rs` - New CLI parser with fullscreen/monitor options
- `src/main.rs` - Integration with app initialization

**CLI Options to Add**:
```bash
--fullscreen, -F           # Start in fullscreen mode
--monitor, -m SELECTOR     # Monitor: primary|index|name
--scale POLICY             # Scaling: fit|fill|1:1 
--keep-aspect BOOL         # Preserve aspect ratio (default: true)
--cursor MODE              # Cursor: local|remote (default: local)
```

**Implementation**:
```rust
#[derive(Parser, Debug)]
pub struct CliArgs {
    // ... existing args
    
    #[arg(short = 'F', long)]
    pub fullscreen: bool,
    
    #[arg(short = 'm', long, value_name = "SELECTOR")]
    pub monitor: Option<String>,
    
    #[arg(long, value_enum, default_value = "fit")]
    pub scale: ScalingPolicy,
    
    #[arg(long, default_value = "true")]
    pub keep_aspect: bool,
}
```

**Tests**:
- CLI parsing for all new flags and combinations
- Error handling for invalid monitor selectors
- Environment variable integration

### Task 2: Monitor Enumeration (1 day)

**Scope**: Cross-platform monitor detection and selection

**Files**:
- `src/display/mod.rs` - DisplayManager trait and Monitor types
- `src/display/winit_backend.rs` - Winit implementation
- `src/display/monitor_selection.rs` - Selection logic

**Monitor Selection Algorithm**:
1. **Primary**: First choice if `primary` specified
2. **Index**: Zero-based index in deterministic order
3. **Name**: Case-insensitive substring match
4. **Fallback**: Primary monitor if target not found

**Deterministic Ordering**:
- Primary monitor always index 0
- Secondary monitors sorted by position (left→right, top→bottom)
- Virtual displays sorted last

**Implementation**:
```rust
impl DisplayManager for WinitDisplayManager {
    fn list_monitors(&self) -> Vec<Monitor> {
        let mut monitors: Vec<_> = self.event_loop
            .available_monitors()
            .enumerate()
            .map(|(i, handle)| Monitor {
                handle: handle.clone(),
                index: i,
                name: handle.name().unwrap_or_else(|| format!("Monitor {}", i)),
                resolution: handle.size().into(),
                scale_factor: handle.scale_factor(),
                is_primary: i == 0, // Winit primary detection
                position: handle.position().into(),
            })
            .collect();
            
        // Sort: primary first, then by position
        monitors.sort_by_key(|m| (if m.is_primary { 0 } else { 1 }, m.position));
        monitors
    }
}
```

**Tests**:
- Mock DisplayManager with 1/2/3 monitor configurations
- Primary detection in various scenarios
- Name/index selection edge cases

### Task 3: Fullscreen Implementation (1-2 days)

**Scope**: Reliable fullscreen entry/exit with state management

**Files**:
- `src/fullscreen/mod.rs` - FullscreenController
- `src/fullscreen/transitions.rs` - State transition logic  
- `src/fullscreen/hotkeys.rs` - Keyboard shortcut handling

**State Management**:
```rust
struct WindowedState {
    size: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
    decorations: bool,
}

impl FullscreenController {
    pub fn enter_fullscreen(
        &mut self, 
        window: &Window, 
        target_monitor: Option<Monitor>
    ) -> Result<(), FullscreenError> {
        // 1. Store current windowed state
        let windowed_state = WindowedState {
            size: window.inner_size(),
            position: window.outer_position()?,
            decorations: window.decorations(),
        };
        
        // 2. Select target monitor
        let monitor = target_monitor
            .or_else(|| self.display_manager.primary_monitor())
            .ok_or(FullscreenError::NoMonitorAvailable)?;
            
        // 3. Configure fullscreen
        window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor.handle.clone()))));
        
        // 4. Update state
        self.current_state = FullscreenState::Fullscreen { 
            monitor: monitor.clone(), 
            mode: FullscreenMode::Borderless 
        };
        self.windowed_state = Some(windowed_state);
        
        info!("Entered fullscreen on monitor: {}", monitor.name);
        Ok(())
    }
}
```

**Hotkey Integration**:
- F11: Primary fullscreen toggle
- Ctrl+Alt+F: Alternative toggle
- Ctrl+Alt+←/→: Move to prev/next monitor
- Ctrl+Alt+0-9: Jump to monitor by index
- Esc: Exit fullscreen (optional)

**Tests**:
- State transition correctness
- Monitor switching without artifacts
- State preservation across transitions

### Task 4: Scaling Implementation (1 day)

**Scope**: Viewport scaling with aspect ratio preservation

**Files**:
- `src/scaling/mod.rs` - ScalingCalculator and policies
- `src/scaling/calculations.rs` - Mathematical viewport calculations
- `src/scaling/dpi.rs` - DPI scaling for mixed environments

**Scaling Algorithm**:
```rust
impl ScalingCalculator {
    pub fn calculate_viewport(
        &self,
        remote_size: (u32, u32),
        window_size: (u32, u32),
        dpi_scale: f64,
    ) -> ViewportInfo {
        let effective_window = (
            (window_size.0 as f64 / dpi_scale) as u32,
            (window_size.1 as f64 / dpi_scale) as u32,
        );
        
        match self.policy {
            ScalingPolicy::Fit => self.calculate_fit(remote_size, effective_window),
            ScalingPolicy::Fill => self.calculate_fill(remote_size, effective_window),
            ScalingPolicy::Native => self.calculate_native(remote_size, effective_window),
        }
    }
    
    fn calculate_fit(&self, remote: (u32, u32), window: (u32, u32)) -> ViewportInfo {
        let scale_x = window.0 as f32 / remote.0 as f32;
        let scale_y = window.1 as f32 / remote.1 as f32;
        let scale = scale_x.min(scale_y);
        
        let scaled_size = (
            (remote.0 as f32 * scale) as u32,
            (remote.1 as f32 * scale) as u32,
        );
        
        let offset = (
            (window.0 - scaled_size.0) / 2,
            (window.1 - scaled_size.1) / 2,
        );
        
        ViewportInfo { scale, offset, scaled_size }
    }
}
```

**Tests**:
- Scaling calculations for various aspect ratios
- DPI handling with different scale factors
- Letterboxing and centering accuracy

### Task 5: Integration and Testing (0.5-1 day)

**Scope**: End-to-end integration and comprehensive testing

**Integration Points**:
- CLI → App initialization with fullscreen/monitor preferences
- App → FullscreenController for state management
- App → ScalingCalculator for viewport updates
- Event handling for hotkeys and monitor changes

**Test Strategy**:

**Unit Tests**:
```rust
#[test]
fn test_monitor_selection_by_index() {
    let manager = MockDisplayManager::with_monitors(3);
    assert_eq!(manager.get_monitor_by_index(1).unwrap().index, 1);
    assert!(manager.get_monitor_by_index(99).is_none());
}

#[test] 
fn test_scaling_fit_calculation() {
    let calc = ScalingCalculator::new(ScalingPolicy::Fit, true);
    let viewport = calc.calculate_viewport((800, 600), (1920, 1080), 1.0);
    assert_eq!(viewport.scale, 1.8); // 1080/600
    assert_eq!(viewport.offset, (280, 0)); // Centered horizontally
}
```

**Integration Tests**:
- Window creation and fullscreen transitions
- Monitor enumeration on test systems
- CLI argument parsing end-to-end

**Manual QA** (following WARP safety rules):
- Single monitor: F11 toggle, scaling modes
- Dual monitors: Ctrl+Alt+Arrow navigation, index selection
- Mixed DPI: Scaling correctness across different DPI monitors
- Server testing: Only use Xnjcvnc :2, never production :1 or :3

## Dependencies

### Required Crates
```toml
# Window management and monitor APIs
winit = "0.28"                      # Cross-platform windowing
egui = "0.27"                       # GUI framework  
eframe = "0.27"                     # egui application framework

# CLI and configuration
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# Logging and error handling
tracing = "0.1"
anyhow = "1.0"
thiserror = "1.0"
```

### Platform Dependencies
- **X11**: EWMH support for fullscreen and monitor detection
- **Wayland**: wl_output protocol for monitor enumeration
- **winit**: Abstraction layer over platform differences

## Error Handling

### Error Types
```rust
#[derive(Debug, thiserror::Error)]
pub enum FullscreenError {
    #[error("No monitor available for fullscreen")]
    NoMonitorAvailable,
    
    #[error("Monitor '{0}' not found")]
    MonitorNotFound(String),
    
    #[error("Failed to enter fullscreen: {0}")]
    TransitionFailed(String),
    
    #[error("Platform not supported: {0}")]
    PlatformUnsupported(String),
}
```

### Graceful Degradation
1. **Monitor not found**: Log warning, use primary monitor
2. **Exclusive fullscreen unsupported**: Fall back to borderless
3. **Monitor disconnected**: Detect and move to available monitor
4. **DPI detection failed**: Use 1.0 scale factor with warning

## Success Criteria

### Milestone M1 (Fullscreen)
- [ ] F11 toggle works reliably on X11 and Wayland
- [ ] CLI `--fullscreen` starts in fullscreen mode
- [ ] Scaling policies (fit/fill/1:1) render correctly
- [ ] DPI-aware scaling on high-resolution monitors
- [ ] Smooth transitions without visual artifacts
- [ ] State preservation across fullscreen/windowed transitions

### Milestone M2 (Multi-monitor)
- [ ] Accurate enumeration of 2-4 monitor setups
- [ ] CLI `--monitor primary|0|1|name` selection works
- [ ] Hotkey navigation (Ctrl+Alt+Arrow, Ctrl+Alt+0-9)
- [ ] Mixed DPI environments handled gracefully
- [ ] Monitor disconnect/reconnect recovery
- [ ] Clear error messages for invalid monitor selections

### Code Quality
- [ ] Zero clippy warnings
- [ ] Comprehensive unit test coverage (>90%)
- [ ] Integration tests for all major workflows
- [ ] Clear error messages and logging
- [ ] Performance: <200ms fullscreen transitions, <1ms scaling calculations

## Timeline

| Task | Duration | Dependencies | 
|------|----------|--------------|
| CLI Enhancement | 0.5-1 day | - |
| Monitor Enumeration | 1 day | CLI complete |
| Fullscreen Implementation | 1-2 days | Monitor enumeration |
| Scaling Implementation | 1 day | Fullscreen basics |
| Integration & Testing | 0.5-1 day | All tasks |

**Total Estimate**: 4-6.5 days for both M1 and M2 milestones.

---

**Next Steps**: Begin with Task 1 (CLI Enhancement) and proceed sequentially. Each task builds on the previous, enabling incremental testing and validation.

**See Also**: [ROADMAP.md](ROADMAP.md), [CLI Usage](cli/USAGE.md), [Fullscreen & Multi-Monitor Spec](spec/fullscreen-and-multimonitor.md), [SEP-0001](SEP/SEP-0001-out-of-scope.md)