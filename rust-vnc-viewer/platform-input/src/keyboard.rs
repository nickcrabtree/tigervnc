use tracing::trace;
use winit::event::{ElementState, KeyboardInput, VirtualKeyCode};

/// X11 keysym values
pub mod keysyms {
    // X11 keysym constants
    pub const XK_BackSpace: u32 = 0xff08;
    pub const XK_Tab: u32 = 0xff09;
    pub const XK_Return: u32 = 0xff0d;
    pub const XK_Escape: u32 = 0xff1b;
    pub const XK_Insert: u32 = 0xff63;
    pub const XK_Delete: u32 = 0xffff;
    pub const XK_Home: u32 = 0xff50;
    pub const XK_End: u32 = 0xff57;
    pub const XK_Page_Up: u32 = 0xff55;
    pub const XK_Page_Down: u32 = 0xff56;
    pub const XK_Left: u32 = 0xff51;
    pub const XK_Up: u32 = 0xff52;
    pub const XK_Right: u32 = 0xff53;
    pub const XK_Down: u32 = 0xff54;
    pub const XK_F1: u32 = 0xffbe;
    pub const XK_F2: u32 = 0xffbf;
    pub const XK_F3: u32 = 0xffc0;
    pub const XK_F4: u32 = 0xffc1;
    pub const XK_F5: u32 = 0xffc2;
    pub const XK_F6: u32 = 0xffc3;
    pub const XK_F7: u32 = 0xffc4;
    pub const XK_F8: u32 = 0xffc5;
    pub const XK_F9: u32 = 0xffc6;
    pub const XK_F10: u32 = 0xffc7;
    pub const XK_F11: u32 = 0xffc8;
    pub const XK_F12: u32 = 0xffc9;
    pub const XK_Shift_L: u32 = 0xffe1;
    pub const XK_Shift_R: u32 = 0xffe2;
    pub const XK_Control_L: u32 = 0xffe3;
    pub const XK_Control_R: u32 = 0xffe4;
    pub const XK_Alt_L: u32 = 0xffe9;
    pub const XK_Alt_R: u32 = 0xffea;
    pub const XK_Super_L: u32 = 0xffeb; // Left Windows/Command key
    pub const XK_Super_R: u32 = 0xffec; // Right Windows/Command key
    pub const XK_Menu: u32 = 0xff67;
    pub const XK_Num_Lock: u32 = 0xff7f;
    pub const XK_Caps_Lock: u32 = 0xffe5;
    pub const XK_Scroll_Lock: u32 = 0xff14;
    pub const XK_Print: u32 = 0xff61;
}
use keysyms::*;

/// Map a winit KeyboardInput to (X11 keysym, down?) suitable for RFB KeyEvent.
pub fn map_keyboard_input(input: &KeyboardInput) -> Option<(u32, bool)> {
    let down = matches!(input.state, ElementState::Pressed);
    let vk = input.virtual_keycode?;
    Some((map_virtual_keycode_to_keysym(vk), down))
}

/// Map a winit VirtualKeyCode to X11 keysym.
pub fn map_virtual_keycode_to_keysym(vk: VirtualKeyCode) -> u32 {
    use VirtualKeyCode as VK;
    match vk {
        // ASCII letters and digits
        VK::A => 'a' as u32,
        VK::B => 'b' as u32,
        VK::C => 'c' as u32,
        VK::D => 'd' as u32,
        VK::E => 'e' as u32,
        VK::F => 'f' as u32,
        VK::G => 'g' as u32,
        VK::H => 'h' as u32,
        VK::I => 'i' as u32,
        VK::J => 'j' as u32,
        VK::K => 'k' as u32,
        VK::L => 'l' as u32,
        VK::M => 'm' as u32,
        VK::N => 'n' as u32,
        VK::O => 'o' as u32,
        VK::P => 'p' as u32,
        VK::Q => 'q' as u32,
        VK::R => 'r' as u32,
        VK::S => 's' as u32,
        VK::T => 't' as u32,
        VK::U => 'u' as u32,
        VK::V => 'v' as u32,
        VK::W => 'w' as u32,
        VK::X => 'x' as u32,
        VK::Y => 'y' as u32,
        VK::Z => 'z' as u32,

        VK::Key0 => '0' as u32,
        VK::Key1 => '1' as u32,
        VK::Key2 => '2' as u32,
        VK::Key3 => '3' as u32,
        VK::Key4 => '4' as u32,
        VK::Key5 => '5' as u32,
        VK::Key6 => '6' as u32,
        VK::Key7 => '7' as u32,
        VK::Key8 => '8' as u32,
        VK::Key9 => '9' as u32,

        // Whitespace and controls
        VK::Space => 0x0020,
        VK::Return => XK_Return,
        VK::Escape => XK_Escape,
        VK::Back => XK_BackSpace,
        VK::Tab => XK_Tab,
        VK::Delete => XK_Delete,
        VK::Insert => XK_Insert,
        VK::Home => XK_Home,
        VK::End => XK_End,
        VK::PageUp => XK_Page_Up,
        VK::PageDown => XK_Page_Down,

        // Arrows
        VK::Left => XK_Left,
        VK::Up => XK_Up,
        VK::Right => XK_Right,
        VK::Down => XK_Down,

        // Function keys
        VK::F1 => XK_F1,
        VK::F2 => XK_F2,
        VK::F3 => XK_F3,
        VK::F4 => XK_F4,
        VK::F5 => XK_F5,
        VK::F6 => XK_F6,
        VK::F7 => XK_F7,
        VK::F8 => XK_F8,
        VK::F9 => XK_F9,
        VK::F10 => XK_F10,
        VK::F11 => XK_F11,
        VK::F12 => XK_F12,

        // Modifiers (left variants)
        VK::LShift => XK_Shift_L,
        VK::RShift => XK_Shift_R,
        VK::LControl => XK_Control_L,
        VK::RControl => XK_Control_R,
        VK::LAlt => XK_Alt_L,
        VK::RAlt => XK_Alt_R,
        VK::LWin => XK_Super_L, // Super
        VK::RWin => XK_Super_R,

        // Punctuation (common subset)
        VK::Minus => '-' as u32,
        VK::Equals => '=' as u32,
        VK::Grave => '`' as u32,
        VK::LBracket => '[' as u32,
        VK::RBracket => ']' as u32,
        VK::Backslash => '\\' as u32,
        VK::Semicolon => ';' as u32,
        VK::Apostrophe => '\'' as u32,
        VK::Comma => ',' as u32,
        VK::Period => '.' as u32,
        VK::Slash => '/' as u32,

        // Fallback: use zero (ignored by server)
        _ => 0,
    }
}

/// Stores state of key modifiers and handles key repeat rate limiting.
#[derive(Debug)]
pub struct KeyMapper {
    // Track modifiers state
    shift: bool,
    control: bool,
    alt: bool,
    super_key: bool, // Windows/Command
    caps_lock: bool,
    num_lock: bool,

    // Track last seen key states to handle auto-repeat
    last_key: Option<u32>,
    throttle_repeats: bool,
    repeat_delay_ms: u64,
    last_press_time: std::time::Instant,
}

impl KeyMapper {
    /// Create a new key mapper with default settings.
    pub fn new() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
            caps_lock: false,
            num_lock: false,
            last_key: None,
            throttle_repeats: true,
            repeat_delay_ms: 50, // 50ms = 20 keys/sec maximum rate
            last_press_time: std::time::Instant::now(),
        }
    }

    /// Enable or disable key repeat throttling.
    pub fn set_throttle_repeats(&mut self, enable: bool) {
        self.throttle_repeats = enable;
    }

    /// Set repeat rate limit in milliseconds.
    pub fn set_repeat_delay(&mut self, delay_ms: u64) {
        self.repeat_delay_ms = delay_ms;
    }

    /// Process a keyboard input, returning a keysym and down state.
    /// May return None if the key should be ignored (e.g., throttled repeat).
    pub fn process_key(&mut self, input: &KeyboardInput) -> Option<(u32, bool)> {
        let down = matches!(input.state, ElementState::Pressed);
        let vk = input.virtual_keycode?;

        // Map virtual keycode to keysym
        let keysym = map_virtual_keycode_to_keysym(vk);

        // Update modifier state
        match (vk, down) {
            (VirtualKeyCode::LShift | VirtualKeyCode::RShift, true) => self.shift = true,
            (VirtualKeyCode::LShift | VirtualKeyCode::RShift, false) => self.shift = false,
            (VirtualKeyCode::LControl | VirtualKeyCode::RControl, true) => self.control = true,
            (VirtualKeyCode::LControl | VirtualKeyCode::RControl, false) => self.control = false,
            (VirtualKeyCode::LAlt | VirtualKeyCode::RAlt, true) => self.alt = true,
            (VirtualKeyCode::LAlt | VirtualKeyCode::RAlt, false) => self.alt = false,
            (VirtualKeyCode::LWin | VirtualKeyCode::RWin, true) => self.super_key = true,
            (VirtualKeyCode::LWin | VirtualKeyCode::RWin, false) => self.super_key = false,
            (VirtualKeyCode::Capital, true) => self.caps_lock = !self.caps_lock,
            (VirtualKeyCode::Numlock, true) => self.num_lock = !self.num_lock,
            _ => {}
        }

        // Handle throttling for key repeats
        let now = std::time::Instant::now();
        if self.throttle_repeats && down {
            // Check if it's the same key pressed again (auto-repeat)
            if self.last_key == Some(keysym) {
                let elapsed = now.duration_since(self.last_press_time);
                if elapsed.as_millis() < self.repeat_delay_ms as u128 {
                    // Too soon, throttle this repeat
                    trace!("Throttling repeated key: {}", keysym);
                    return None;
                }
            }
            self.last_key = Some(keysym);
            self.last_press_time = now;
        } else if !down {
            // Key release - clear last key if it matches
            if self.last_key == Some(keysym) {
                self.last_key = None;
            }
        }

        // Return mapped keysym and down state
        Some((keysym, down))
    }

    /// Get current modifier state as a bitmask (for use in protocol messages).
    pub fn modifier_mask(&self) -> u8 {
        let mut mask = 0;
        if self.shift {
            mask |= 1
        }
        if self.control {
            mask |= 4
        }
        if self.alt {
            mask |= 8
        }
        if self.super_key {
            mask |= 64
        }
        mask
    }

    /// Returns true if given modifier is active.
    pub fn is_modifier_active(&self, modifier: Modifier) -> bool {
        match modifier {
            Modifier::Shift => self.shift,
            Modifier::Control => self.control,
            Modifier::Alt => self.alt,
            Modifier::Super => self.super_key,
            Modifier::CapsLock => self.caps_lock,
            Modifier::NumLock => self.num_lock,
        }
    }
}

impl Default for KeyMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Keyboard modifiers for shortcuts and key combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Super, // Windows/Command key
    CapsLock,
    NumLock,
}

/// Exportable helper: maps winit key event (press) to keysym.
pub fn map_key_event_to_keysym(vk: VirtualKeyCode) -> u32 {
    map_virtual_keycode_to_keysym(vk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::{ElementState, KeyboardInput, VirtualKeyCode};

    #[test]
    fn test_ascii_letters_and_return() {
        let ev = KeyboardInput {
            scancode: 0,
            state: ElementState::Pressed,
            virtual_keycode: Some(VirtualKeyCode::A),
            modifiers: Default::default(),
        };
        assert_eq!(map_keyboard_input(&ev), Some(('a' as u32, true)));
        let ret = KeyboardInput {
            scancode: 0,
            state: ElementState::Pressed,
            virtual_keycode: Some(VirtualKeyCode::Return),
            modifiers: Default::default(),
        };
        assert_eq!(map_keyboard_input(&ret), Some((0xFF0D, true)));
    }
}
