use bitflags::bitflags;
use winit::event::ModifiersState;
use std::time::{Duration, Instant};
use tracing::trace;

bitflags! {
    /// VNC pointer button mask (bits).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ButtonMask: u8 {
        const LEFT  = 1 << 0; // Button 1
        const MIDDLE= 1 << 1; // Button 2
        const RIGHT = 1 << 2; // Button 3
        const WHEEL_UP   = 1 << 3; // Button 4 (scroll up)
        const WHEEL_DOWN = 1 << 4; // Button 5 (scroll down)
        const WHEEL_LEFT = 1 << 5; // Button 6 (horizontal scroll left)
        const WHEEL_RIGHT = 1 << 6; // Button 7 (horizontal scroll right)
    }
}

/// Configuration for pointer event throttling.
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Enable pointer movement throttling
    pub enabled: bool,
    /// Minimum time between pointer movement events (milliseconds)
    pub min_interval_ms: u64,
    /// Maximum distance to travel before forcing an event (pixels)
    pub max_distance: f64,
    /// Enable middle-button emulation (Left + Right click)
    pub middle_button_emulation: bool,
    /// Time window for middle-button emulation (milliseconds)
    pub emulation_timeout_ms: u64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_interval_ms: 16, // ~60fps
            max_distance: 5.0,
            middle_button_emulation: false,
            emulation_timeout_ms: 500,
        }
    }
}

/// Tracks mouse state for building pointer commands.
#[derive(Debug, Clone)]
pub struct MouseState {
    x: i32,
    y: i32,
    pub buttons: ButtonMask,
    pub modifiers: ModifiersState,
    
    // Throttling state
    config: ThrottleConfig,
    last_position_sent: Option<(i32, i32)>,
    last_move_time: Instant,
    
    // Middle button emulation state
    left_pressed_time: Option<Instant>,
    right_pressed_time: Option<Instant>,
    emulation_active: bool,
}

impl Default for MouseState {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseState {
    /// Create a new mouse state with default throttling configuration.
    pub fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            buttons: ButtonMask::empty(),
            modifiers: ModifiersState::empty(),
            config: ThrottleConfig::default(),
            last_position_sent: None,
            last_move_time: Instant::now(),
            left_pressed_time: None,
            right_pressed_time: None,
            emulation_active: false,
        }
    }

    /// Create with custom throttling configuration.
    pub fn with_config(config: ThrottleConfig) -> Self {
        let mut state = Self::new();
        state.config = config;
        state
    }

    /// Set throttling configuration.
    pub fn set_config(&mut self, config: ThrottleConfig) {
        self.config = config;
    }

    /// Update position and return whether a pointer event should be sent.
    pub fn set_pos(&mut self, x: i32, y: i32) -> bool {
        self.x = x;
        self.y = y;
        self.should_send_movement(x, y)
    }

    /// Get current position.
    pub fn pos(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// Set keyboard modifiers.
    pub fn set_modifiers(&mut self, m: ModifiersState) {
        self.modifiers = m;
    }

    /// Check if a movement event should be sent based on throttling rules.
    fn should_send_movement(&mut self, x: i32, y: i32) -> bool {
        if !self.config.enabled {
            return true;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_move_time);
        
        let time_threshold = Duration::from_millis(self.config.min_interval_ms);
        let time_expired = elapsed >= time_threshold;
        
        let distance_exceeded = if let Some((last_x, last_y)) = self.last_position_sent {
            let dx = (x - last_x) as f64;
            let dy = (y - last_y) as f64;
            let distance = (dx * dx + dy * dy).sqrt();
            distance > self.config.max_distance
        } else {
            true // First movement
        };

        if time_expired || distance_exceeded {
            self.last_position_sent = Some((x, y));
            self.last_move_time = now;
            true
        } else {
            false
        }
    }

    /// Handle button press/release with middle-button emulation.
    /// Returns (should_send_event, final_button_mask)
    pub fn handle_button(&mut self, button: winit::event::MouseButton, pressed: bool) -> Option<u8> {
        use winit::event::MouseButton;
        
        let now = Instant::now();
        let timeout = Duration::from_millis(self.config.emulation_timeout_ms);
        
        if self.config.middle_button_emulation {
            match (button, pressed) {
                (MouseButton::Left, true) => {
                    self.left_pressed_time = Some(now);
                    if let Some(right_time) = self.right_pressed_time {
                        if now.duration_since(right_time) < timeout {
                            // Activate middle button emulation
                            self.emulation_active = true;
                            self.buttons.insert(ButtonMask::MIDDLE);
                            self.buttons.remove(ButtonMask::LEFT | ButtonMask::RIGHT);
                            trace!("Middle button emulation activated");
                            return Some(self.buttons.bits());
                        }
                    }
                    if !self.emulation_active {
                        self.buttons.insert(ButtonMask::LEFT);
                    }
                }
                (MouseButton::Right, true) => {
                    self.right_pressed_time = Some(now);
                    if let Some(left_time) = self.left_pressed_time {
                        if now.duration_since(left_time) < timeout {
                            // Activate middle button emulation
                            self.emulation_active = true;
                            self.buttons.insert(ButtonMask::MIDDLE);
                            self.buttons.remove(ButtonMask::LEFT | ButtonMask::RIGHT);
                            trace!("Middle button emulation activated");
                            return Some(self.buttons.bits());
                        }
                    }
                    if !self.emulation_active {
                        self.buttons.insert(ButtonMask::RIGHT);
                    }
                }
                (MouseButton::Left, false) => {
                    self.left_pressed_time = None;
                    if self.emulation_active {
                        self.emulation_active = false;
                        self.buttons.remove(ButtonMask::MIDDLE);
                    } else {
                        self.buttons.remove(ButtonMask::LEFT);
                    }
                }
                (MouseButton::Right, false) => {
                    self.right_pressed_time = None;
                    if self.emulation_active {
                        self.emulation_active = false;
                        self.buttons.remove(ButtonMask::MIDDLE);
                    } else {
                        self.buttons.remove(ButtonMask::RIGHT);
                    }
                }
                (MouseButton::Middle, true) => self.buttons.insert(ButtonMask::MIDDLE),
                (MouseButton::Middle, false) => self.buttons.remove(ButtonMask::MIDDLE),
                _ => return None, // Unknown button
            }
        } else {
            // Standard button handling without emulation
            match (button, pressed) {
                (MouseButton::Left, true) => self.buttons.insert(ButtonMask::LEFT),
                (MouseButton::Left, false) => self.buttons.remove(ButtonMask::LEFT),
                (MouseButton::Right, true) => self.buttons.insert(ButtonMask::RIGHT),
                (MouseButton::Right, false) => self.buttons.remove(ButtonMask::RIGHT),
                (MouseButton::Middle, true) => self.buttons.insert(ButtonMask::MIDDLE),
                (MouseButton::Middle, false) => self.buttons.remove(ButtonMask::MIDDLE),
                _ => return None,
            }
        }
        
        Some(self.buttons.bits())
    }

    /// Force sending the next movement (bypass throttling).
    pub fn force_next_movement(&mut self) {
        self.last_position_sent = None;
        self.last_move_time = Instant::now() - Duration::from_secs(1);
    }

    /// Get button mask for wheel events.
    pub fn wheel_button_mask(&self, delta_x: f32, delta_y: f32) -> Vec<u8> {
        let mut events = Vec::new();
        let base_buttons = self.buttons.bits();
        
        // Vertical scroll
        if delta_y > 0.0 {
            events.push(base_buttons | ButtonMask::WHEEL_UP.bits());
            events.push(base_buttons); // Release
        } else if delta_y < 0.0 {
            events.push(base_buttons | ButtonMask::WHEEL_DOWN.bits());
            events.push(base_buttons); // Release
        }
        
        // Horizontal scroll (if supported)
        if delta_x > 0.0 {
            events.push(base_buttons | ButtonMask::WHEEL_RIGHT.bits());
            events.push(base_buttons); // Release
        } else if delta_x < 0.0 {
            events.push(base_buttons | ButtonMask::WHEEL_LEFT.bits());
            events.push(base_buttons); // Release
        }
        
        events
    }
}
