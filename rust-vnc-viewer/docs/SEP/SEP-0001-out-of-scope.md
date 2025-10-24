# SEP-0001: Out-of-Scope Features for Rust VNC Viewer v0.x

**Status**: Active  
**Created**: 2025-10-24  
**Authors**: Development Team  

## Summary

This document explicitly defines features that are **out-of-scope** for the Rust VNC Viewer MVP (v0.x). These features are designated as "Someone Else's Problem" (SEP) to focus engineering resources on core desktop VNC functionality: fullscreen mode and multi-monitor support.

## Non-Goals (Firm, with Rationale)

### 1. Touch Support / Gesture Processing

**Scope**: Touch input handling, pinch-to-zoom, swipe gestures, multi-touch events.

**Rationale**: 
- Desktop-only target audience; focus on keyboard and mouse input
- Touch interfaces add significant complexity without clear desktop benefit
- Existing gesture support in `platform-input` crate is adequate for basic trackpad scrolling
- Mobile/tablet VNC use cases are better served by dedicated mobile clients

**Impact**: Remove touch gesture processing from Phase 9B roadmap.

### 2. Connection Profiles and Settings UI

**Scope**: GUI dialogs for saving/managing connection profiles, preferences UI, settings storage/management.

**Rationale**:
- CLI-based configuration is more suitable for desktop power users
- GUI settings add UI complexity without proportional value
- Command-line flags and environment variables provide sufficient configuration
- Reduces maintenance burden and keeps the viewer lean

**Impact**: Remove connection profile UI and settings dialogs from implementation plans.

### 3. Screenshot Functionality

**Scope**: Built-in screenshot capture, frame saving, recording features.

**Rationale**:
- OS-native screenshot tools are superior and more familiar to users
- VNC-specific screenshots don't provide unique value over system tools
- Reduces codebase complexity and dependency burden
- Users can capture VNC windows using standard screenshot utilities

**Impact**: Remove screenshot features from Phase 9B roadmap.

## Alternatives and Workarounds

### Screenshots
Use OS-native tools:
- **X11**: `gnome-screenshot`, `scrot`, `xwd`, `import` (ImageMagick)
- **Wayland**: `grim`, `grimshot`, `gnome-screenshot`

Example commands:
```bash
# Capture VNC window
gnome-screenshot --window --file vnc-session.png

# X11: Capture specific window
xwd -name "VNC Viewer" > vnc.xwd

# Wayland: Capture active window  
grim -g "$(swaymsg -t get_tree | jq -r '.. | select(.focused?) | .rect | "\(.x),\(.y) \(.width)x\(.height)"')" vnc.png
```

### Configuration
Use command-line flags and environment variables:
```bash
# Connection with options
cargo run -- --connect vnc://server:5901 --fullscreen --monitor primary --scale fit

# Environment-based password
VNC_PASSWORD=secret cargo run -- vnc://server:5901

# Config via args
cargo run -- --monitor 1 --cursor remote --scale 1:1
```

### Touch/Gestures
For trackpad users, basic two-finger scrolling is handled by the windowing system. Advanced gestures are not needed for desktop VNC usage.

## Reconsideration Criteria (Future)

These features may be reconsidered if:

### Touch Support
- Target platforms expand to include tablets or touch-enabled desktops
- Significant user demand emerges for touch-specific VNC workflows
- Desktop environments standardize touch interactions that benefit VNC

### Settings UI
- CLI configuration becomes a demonstrated usability blocker
- Post-v1.0 assessment shows clear need for graphical configuration
- Sufficient engineering capacity exists for UI maintenance

### Screenshot Functionality  
- Unique VNC-specific screenshot features emerge (e.g., ContentCache-aware captures)
- OS screenshot tools prove insufficient for VNC-specific workflows
- Integration with VNC protocol provides clear advantages over system tools

## Impact

### Positive Impact
- **Focus**: Engineering resources concentrated on fullscreen and multi-monitor features
- **Simplicity**: Reduced codebase complexity and maintenance burden
- **Performance**: Fewer dependencies and background processes
- **Reliability**: Fewer potential failure modes and edge cases

### Engineering Changes Required
1. Remove references to touch/gesture/screenshot/profiles from:
   - `README.md` feature lists
   - Phase 9B specifications  
   - Implementation plans and roadmaps
2. Delete or mark as out-of-scope:
   - Touch processing code stubs
   - Settings UI mockups/designs
   - Screenshot-related dependencies
3. Update CLI to reject removed feature flags with clear error messages

## Related Documents

- [ROADMAP.md](../ROADMAP.md) - Project roadmap with fullscreen/multi-monitor prioritization
- [CLI Usage](../cli/USAGE.md) - Command-line configuration reference  
- [Fullscreen & Multi-monitor Spec](../spec/fullscreen-and-multimonitor.md) - Technical specification
- [Implementation Plan](../IMPLEMENTATION_PLAN.md) - Development task breakdown

## Appendix: Terminology

**SEP (Someone Else's Problem)**: A conscious decision to exclude features from scope, typically because:
- They can be better addressed by external tools or systems
- They add complexity disproportionate to their value
- They distract from core objectives
- They require specialized expertise or resources

The term emphasizes that excluded features may have legitimate value, but are explicitly not the responsibility of this project.

---

*This document represents a firm commitment to scope management. Changes require explicit approval from project maintainers and compelling justification.*