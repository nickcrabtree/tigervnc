# CLI Usage - Rust VNC Viewer

Command-line interface for the desktop-focused VNC viewer with fullscreen and multi-monitor support.

## Invocation Patterns

```bash
# Connection string format
rust-vnc-viewer --connect vnc://HOST[:PORT] [options]

# Positional server argument (traditional)
rust-vnc-viewer HOST[:PORT] [options]

# Examples
rust-vnc-viewer localhost:5902
rust-vnc-viewer --connect vnc://192.168.1.100:5901 --fullscreen --monitor primary
```

## Core Options

### Connection
- `--connect, -c STRING`: VNC connection string (e.g., `vnc://localhost:5902`)
- `--password STRING`: Connection password (prefer `$VNC_PASSWORD` environment variable)

### Display
- `--fullscreen, -F`: Start in fullscreen mode
- `--monitor, -m SELECTOR`: Monitor selection (see Monitor Selection below)
- `--scale POLICY`: Scaling policy: `fit` (default), `fill`, `1:1` 
- `--keep-aspect BOOL`: Preserve aspect ratio when scaling (default: `true`)
- `--cursor MODE`: Cursor rendering: `local` (default), `remote`

### Window Management
- `--width WIDTH`: Initial window width (default: 1024, ignored in fullscreen)
- `--height HEIGHT`: Initial window height (default: 768, ignored in fullscreen)

### Advanced
- `--resize POLICY`: Window resize behavior: `local-scale` (default), `request-remote`
- `--verbose, -v`: Verbose logging (repeat for more: `-v`, `-vv`, `-vvv`)

## Monitor Selection

The `--monitor` option accepts:

- `primary`: Use primary monitor (default for fullscreen)
- `INDEX`: Use monitor by zero-based index (e.g., `0`, `1`, `2`)
- `NAME_SUBSTRING`: Match monitor name containing substring (e.g., `DP-1`, `HDMI`)

Examples:
```bash
# Fullscreen on primary monitor
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor primary

# Fullscreen on second monitor 
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor 1

# Select monitor by name
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor "HDMI-A-1"
```

## Scaling Policies

- **`fit`** (default): Scale to fit window, preserving aspect ratio (letterboxing)
- **`fill`**: Scale to fill window completely (may crop or stretch)
- **`1:1`**: No scaling, 1:1 pixel mapping (panning if remote larger than window)

Examples:
```bash
# Fit with letterboxing
rust-vnc-viewer vnc://server:5901 --scale fit --keep-aspect true

# Fill entire window
rust-vnc-viewer vnc://server:5901 --scale fill

# Native 1:1 scaling
rust-vnc-viewer vnc://server:5901 --scale 1:1
```

## Keyboard Shortcuts

### Fullscreen
- `F11`: Toggle fullscreen mode
- `Ctrl+Alt+F`: Toggle fullscreen (alternative)
- `Esc`: Exit fullscreen (configurable)

### Multi-Monitor (implemented)
- `Ctrl+Alt+←` / `Ctrl+Alt+→`: Move fullscreen to previous/next monitor (cycles through list)
- `Ctrl+Alt+0/1/2...`: Jump to monitor by index (0-9)
- `Ctrl+Alt+P`: Move to primary monitor

### General
- `Ctrl+Alt+Q`: Quit viewer
- `F1`: Show/hide connection information overlay

## Environment Variables

- `VNC_PASSWORD`: Connection password (more secure than command-line; implemented via clap env)
- `RUST_LOG`: Logging level (e.g., `debug`, `trace`)

## Configuration Examples

### Basic Connection
```bash
# Simple windowed connection
rust-vnc-viewer localhost:5902

# With password from environment
VNC_PASSWORD=secret rust-vnc-viewer server.example.com:5901
```

### Fullscreen Usage
```bash
# Fullscreen on primary monitor
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor primary

# Specific monitor with fit scaling
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor 1 --scale fit

# High-DPI monitor with 1:1 scaling
rust-vnc-viewer vnc://server:5901 --fullscreen --monitor "4K" --scale 1:1
```

### Development/Testing
```bash
# Verbose logging for debugging
rust-vnc-viewer vnc://localhost:5902 -vvv

# Testing with specific window size
rust-vnc-viewer vnc://localhost:5902 --width 800 --height 600 --scale 1:1
```

## Platform-Specific Notes

### X11 (Linux)
- Fullscreen uses EWMH `_NET_WM_STATE_FULLSCREEN`
- Monitor names typically follow pattern: `DP-1`, `HDMI-A-1`, `eDP-1`
- Primary monitor detection via XRandR

### Wayland (Linux) 
- Fullscreen via wl_shell or xdg_shell protocols
- Monitor selection via wl_output
- Some compositors may override fullscreen behavior

### Multi-Monitor Caveats
- Mixed DPI environments: scaling calculated per-monitor
- Ultrawide monitors: aspect ratio handling may need `--scale fill`
- Virtual displays: may not report correct physical size

## Out-of-Scope Features

The following features are explicitly **out-of-scope** (see [SEP-0001](../SEP/SEP-0001-out-of-scope.md)):

- **Touch/Gesture support**: Use desktop environment's trackpad handling
- **Settings UI/Profiles**: Use command-line flags and environment variables
- **Screenshot capture**: Use OS tools (`gnome-screenshot`, `grim`, `scrot`, etc.)

### Screenshot Alternatives

Instead of built-in screenshots, use OS-native tools:

```bash
# X11: Capture VNC window
gnome-screenshot --window --file vnc-session.png
scrot --select vnc.png

# Wayland: Capture active window
grim -g "$(swaymsg -t get_tree | jq -r '.. | select(.focused?) | .rect | "\(.x),\(.y) \(.width)x\(.height)"')" vnc.png
```

## Error Handling

### Common Issues
- Monitor not found: Falls back to primary monitor with warning
- Exclusive fullscreen unsupported: Uses borderless fullscreen
- Connection failed: Clear error message with troubleshooting hints

### Logging
Enable verbose logging to diagnose issues:
```bash
rust-vnc-viewer vnc://server:5901 --verbose
```

Log messages include:
- Selected monitor details (name, DPI, resolution)
- Fullscreen mode transitions
- Scaling policy applications
- Connection status and errors

---

**See Also**: [SEP-0001 Out-of-Scope Features](../SEP/SEP-0001-out-of-scope.md), [Fullscreen & Multi-Monitor Specification](../spec/fullscreen-and-multimonitor.md)