//! Keyboard shortcuts and hotkey configuration for VNC viewer.
//!
//! This module provides configurable keyboard shortcuts for common VNC viewer
//! operations like fullscreen toggle, scaling mode changes, view-only mode, etc.

use crate::keyboard::Modifier;
use std::collections::HashMap;
use winit::event::{VirtualKeyCode, KeyboardInput, ElementState};
use tracing::trace;

/// Actions that can be triggered by keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    /// Toggle fullscreen mode
    ToggleFullscreen,
    /// Toggle view-only mode (disable input)
    ToggleViewOnly,
    /// Cycle through scaling modes (Native -> Fit -> Fill -> Native)
    CycleScalingMode,
    /// Set scaling to native (1:1)
    ScaleNative,
    /// Set scaling to fit window
    ScaleFit,
    /// Set scaling to fill window
    ScaleFill,
    /// Zoom in
    ZoomIn,
    /// Zoom out
    ZoomOut,
    /// Reset zoom to 100%
    ResetZoom,
    /// Center viewport
    CenterViewport,
    /// Show/hide connection info overlay
    ToggleConnectionInfo,
    /// Take screenshot
    TakeScreenshot,
    /// Disconnect from server
    Disconnect,
    /// Show preferences dialog
    ShowPreferences,
    /// Show help/about dialog
    ShowHelp,
    /// Send Ctrl+Alt+Del to remote
    SendCtrlAltDel,
    /// Refresh screen
    RefreshScreen,
    /// Toggle clipboard sync
    ToggleClipboard,
}

/// A keyboard shortcut definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// Required modifiers (Ctrl, Alt, etc.)
    pub modifiers: Vec<Modifier>,
    /// The key that must be pressed
    pub key: VirtualKeyCode,
    /// Description for UI/help
    pub description: String,
}

impl Shortcut {
    /// Create a new shortcut.
    pub fn new(modifiers: Vec<Modifier>, key: VirtualKeyCode, description: &str) -> Self {
        Self {
            modifiers,
            key,
            description: description.to_string(),
        }
    }
    
    /// Create shortcut with single modifier.
    pub fn with_modifier(modifier: Modifier, key: VirtualKeyCode, description: &str) -> Self {
        Self::new(vec![modifier], key, description)
    }
    
    /// Create shortcut with no modifiers.
    pub fn key_only(key: VirtualKeyCode, description: &str) -> Self {
        Self::new(vec![], key, description)
    }

    /// Check if this shortcut matches the current key combination.
    pub fn matches(&self, key: VirtualKeyCode, active_modifiers: &[Modifier]) -> bool {
        if self.key != key {
            return false;
        }
        
        // Check that all required modifiers are present
        for required_mod in &self.modifiers {
            if !active_modifiers.contains(required_mod) {
                return false;
            }
        }
        
        // Check that no extra modifiers are present (strict matching)
        active_modifiers.len() == self.modifiers.len()
    }
}

/// Configuration and management of keyboard shortcuts.
#[derive(Debug, Clone)]
pub struct ShortcutsConfig {
    /// Mapping from shortcut to action
    shortcuts: HashMap<Shortcut, ShortcutAction>,
    /// Whether shortcuts are enabled
    enabled: bool,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        let mut config = Self {
            shortcuts: HashMap::new(),
            enabled: true,
        };
        
        config.load_defaults();
        config
    }
}

impl ShortcutsConfig {
    /// Create a new shortcuts configuration.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Enable or disable shortcuts globally.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Check if shortcuts are enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    /// Add or update a shortcut.
    pub fn add_shortcut(&mut self, shortcut: Shortcut, action: ShortcutAction) {
        self.shortcuts.insert(shortcut, action);
    }
    
    /// Remove a shortcut.
    pub fn remove_shortcut(&mut self, shortcut: &Shortcut) {
        self.shortcuts.remove(shortcut);
    }
    
    /// Get all shortcuts for a specific action.
    pub fn shortcuts_for_action(&self, action: ShortcutAction) -> Vec<&Shortcut> {
        self.shortcuts.iter()
            .filter(|(_, &a)| a == action)
            .map(|(s, _)| s)
            .collect()
    }
    
    /// Get all configured shortcuts.
    pub fn all_shortcuts(&self) -> &HashMap<Shortcut, ShortcutAction> {
        &self.shortcuts
    }
    
    /// Process a keyboard input and return the triggered action, if any.
    pub fn process_key_input(
        &self, 
        input: &KeyboardInput, 
        active_modifiers: &[Modifier]
    ) -> Option<ShortcutAction> {
        if !self.enabled || !matches!(input.state, ElementState::Pressed) {
            return None;
        }
        
        let key = input.virtual_keycode?;
        
        for (shortcut, &action) in &self.shortcuts {
            if shortcut.matches(key, active_modifiers) {
                trace!("Shortcut triggered: {:?} -> {:?}", shortcut, action);
                return Some(action);
            }
        }
        
        None
    }
    
    /// Load default shortcuts (can be overridden).
    pub fn load_defaults(&mut self) {
        use VirtualKeyCode::*;
        use Modifier::*;
        
        // Fullscreen
        self.add_shortcut(
            Shortcut::key_only(F11, "Toggle fullscreen"),
            ShortcutAction::ToggleFullscreen
        );
        self.add_shortcut(
            Shortcut::with_modifier(Alt, Return, "Toggle fullscreen"),
            ShortcutAction::ToggleFullscreen
        );
        
        // Scaling
        self.add_shortcut(
            Shortcut::with_modifier(Control, Key0, "Reset zoom to 100%"),
            ShortcutAction::ResetZoom
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Equals, "Zoom in"),
            ShortcutAction::ZoomIn
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Minus, "Zoom out"),
            ShortcutAction::ZoomOut
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Key1, "Native scaling (1:1)"),
            ShortcutAction::ScaleNative
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Key2, "Fit to window"),
            ShortcutAction::ScaleFit
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Key3, "Fill window"),
            ShortcutAction::ScaleFill
        );
        
        // View control
        self.add_shortcut(
            Shortcut::with_modifier(Control, R, "Refresh screen"),
            ShortcutAction::RefreshScreen
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, I, "Toggle connection info"),
            ShortcutAction::ToggleConnectionInfo
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, V, "Toggle view-only mode"),
            ShortcutAction::ToggleViewOnly
        );
        
        // Special key combinations
        self.add_shortcut(
            Shortcut::new(vec![Control, Alt], Delete, "Send Ctrl+Alt+Del"),
            ShortcutAction::SendCtrlAltDel
        );
        
        // Application control
        self.add_shortcut(
            Shortcut::with_modifier(Control, Q, "Disconnect"),
            ShortcutAction::Disconnect
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, Comma, "Show preferences"),
            ShortcutAction::ShowPreferences
        );
        self.add_shortcut(
            Shortcut::key_only(F1, "Show help"),
            ShortcutAction::ShowHelp
        );
        
        // Screenshots and clipboard
        self.add_shortcut(
            Shortcut::key_only(F12, "Take screenshot"),
            ShortcutAction::TakeScreenshot
        );
        self.add_shortcut(
            Shortcut::with_modifier(Control, C, "Toggle clipboard sync"),
            ShortcutAction::ToggleClipboard
        );
        
        // Viewport
        self.add_shortcut(
            Shortcut::with_modifier(Control, Home, "Center viewport"),
            ShortcutAction::CenterViewport
        );
    }
    
    /// Get a human-readable description of a key combination.
    pub fn format_key_combination(shortcut: &Shortcut) -> String {
        let mut parts = Vec::new();
        
        for modifier in &shortcut.modifiers {
            let name = match modifier {
                Modifier::Control => "Ctrl",
                Modifier::Alt => "Alt",
                Modifier::Shift => "Shift",
                Modifier::Super => "Cmd", // or "Win" on Windows
                Modifier::CapsLock => "Caps",
                Modifier::NumLock => "Num",
            };
            parts.push(name);
        }
        
        let key_name = format_key_name(shortcut.key);
        parts.push(&key_name);
        
        parts.join("+")
    }
}

/// Format a key name for display.
fn format_key_name(key: VirtualKeyCode) -> String {
    use VirtualKeyCode::*;
    match key {
        F1 => "F1".to_string(),
        F2 => "F2".to_string(),
        F3 => "F3".to_string(),
        F4 => "F4".to_string(),
        F5 => "F5".to_string(),
        F6 => "F6".to_string(),
        F7 => "F7".to_string(),
        F8 => "F8".to_string(),
        F9 => "F9".to_string(),
        F10 => "F10".to_string(),
        F11 => "F11".to_string(),
        F12 => "F12".to_string(),
        Key0 => "0".to_string(),
        Key1 => "1".to_string(),
        Key2 => "2".to_string(),
        Key3 => "3".to_string(),
        Key4 => "4".to_string(),
        Key5 => "5".to_string(),
        Key6 => "6".to_string(),
        Key7 => "7".to_string(),
        Key8 => "8".to_string(),
        Key9 => "9".to_string(),
        A => "A".to_string(),
        B => "B".to_string(),
        C => "C".to_string(),
        D => "D".to_string(),
        E => "E".to_string(),
        F => "F".to_string(),
        G => "G".to_string(),
        H => "H".to_string(),
        I => "I".to_string(),
        J => "J".to_string(),
        K => "K".to_string(),
        L => "L".to_string(),
        M => "M".to_string(),
        N => "N".to_string(),
        O => "O".to_string(),
        P => "P".to_string(),
        Q => "Q".to_string(),
        R => "R".to_string(),
        S => "S".to_string(),
        T => "T".to_string(),
        U => "U".to_string(),
        V => "V".to_string(),
        W => "W".to_string(),
        X => "X".to_string(),
        Y => "Y".to_string(),
        Z => "Z".to_string(),
        Return => "Enter".to_string(),
        Back => "Backspace".to_string(),
        Delete => "Del".to_string(),
        Insert => "Ins".to_string(),
        Home => "Home".to_string(),
        End => "End".to_string(),
        PageUp => "PgUp".to_string(),
        PageDown => "PgDn".to_string(),
        Left => "←".to_string(),
        Right => "→".to_string(),
        Up => "↑".to_string(),
        Down => "↓".to_string(),
        Space => "Space".to_string(),
        Tab => "Tab".to_string(),
        Escape => "Esc".to_string(),
        Comma => ",".to_string(),
        Period => ".".to_string(),
        Slash => "/".to_string(),
        Semicolon => ";".to_string(),
        Apostrophe => "'".to_string(),
        LBracket => "[".to_string(),
        RBracket => "]".to_string(),
        Backslash => "\\".to_string(),
        Grave => "`".to_string(),
        Minus => "-".to_string(),
        Equals => "=".to_string(),
        _ => format!("{:?}", key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_shortcut_matching() {
        let shortcut = Shortcut::new(
            vec![Modifier::Control, Modifier::Shift],
            VirtualKeyCode::A,
            "Test shortcut"
        );
        
        // Should match exact combination
        assert!(shortcut.matches(
            VirtualKeyCode::A,
            &[Modifier::Control, Modifier::Shift]
        ));
        
        // Should not match with extra modifier
        assert!(!shortcut.matches(
            VirtualKeyCode::A,
            &[Modifier::Control, Modifier::Shift, Modifier::Alt]
        ));
        
        // Should not match with missing modifier
        assert!(!shortcut.matches(
            VirtualKeyCode::A,
            &[Modifier::Control]
        ));
        
        // Should not match different key
        assert!(!shortcut.matches(
            VirtualKeyCode::B,
            &[Modifier::Control, Modifier::Shift]
        ));
    }
    
    #[test]
    fn test_default_shortcuts() {
        let config = ShortcutsConfig::default();
        
        // Should have F11 for fullscreen
        let f11_shortcuts = config.shortcuts_for_action(ShortcutAction::ToggleFullscreen);
        assert!(!f11_shortcuts.is_empty());
        
        // Should have multiple shortcuts for some actions
        assert!(f11_shortcuts.len() >= 1);
    }
    
    #[test]
    fn test_format_key_combination() {
        let shortcut = Shortcut::new(
            vec![Modifier::Control, Modifier::Alt],
            VirtualKeyCode::Delete,
            "Ctrl+Alt+Del"
        );
        
        let formatted = ShortcutsConfig::format_key_combination(&shortcut);
        assert_eq!(formatted, "Ctrl+Alt+Del");
    }
}
