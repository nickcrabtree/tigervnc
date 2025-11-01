//! platform-input: Map winit events to rfb_client::ClientCommand
//!
//! This crate provides InputDispatcher, which translates window/input events
//! into VNC client commands suitable for sending to the server.

mod keyboard;
mod mouse;
mod shortcuts;

// Gesture support for trackpads and touch devices
mod gestures;

use rfb_client::ClientCommand;
use winit::event::{MouseScrollDelta, WindowEvent};

pub use gestures::{GestureAction, GestureConfig, GestureEvent, GestureProcessor};
pub use keyboard::{keysyms, map_key_event_to_keysym, KeyMapper, Modifier};
pub use mouse::{ButtonMask, MouseState, ThrottleConfig};
pub use shortcuts::{Shortcut, ShortcutAction, ShortcutsConfig};

/// Coordinate mapper for translating window coordinates to framebuffer coords.
/// Defaults to identity mapping (clamped to u16).
pub type CoordMapper = Box<dyn Fn(i32, i32) -> (u16, u16) + Send + Sync>;

/// Input dispatcher state and helpers.
pub struct InputDispatcher {
    mouse: MouseState,
    coord_mapper: CoordMapper,
}

impl Default for InputDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl InputDispatcher {
    /// Create a new dispatcher with identity coordinate mapping.
    pub fn new() -> Self {
        Self {
            mouse: MouseState::default(),
            coord_mapper: Box::new(|x, y| (x.max(0) as u16, y.max(0) as u16)),
        }
    }

    /// Override coordinate mapper (e.g. to account for viewport scaling/pan).
    pub fn set_coord_mapper<F>(&mut self, f: F)
    where
        F: Fn(i32, i32) -> (u16, u16) + Send + Sync + 'static,
    {
        self.coord_mapper = Box::new(f);
    }

    /// Handle a winit WindowEvent and return zero or more VNC client commands.
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> Vec<ClientCommand> {
        use winit::event::{ElementState, MouseButton, WindowEvent::*};

        let mut out = Vec::new();
        match event {
            CursorMoved { position, .. } => {
                let (x, y) = (position.x as i32, position.y as i32);
                self.mouse.set_pos(x, y);
                let (fx, fy) = (self.coord_mapper)(x, y);
                out.push(ClientCommand::Pointer {
                    x: fx,
                    y: fy,
                    buttons: self.mouse.buttons.bits(),
                });
            }
            MouseInput { state, button, .. } => {
                let (x, y) = self.mouse.pos();
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => {
                        self.mouse.buttons.insert(ButtonMask::LEFT)
                    }
                    (ElementState::Released, MouseButton::Left) => {
                        self.mouse.buttons.remove(ButtonMask::LEFT)
                    }
                    (ElementState::Pressed, MouseButton::Middle) => {
                        self.mouse.buttons.insert(ButtonMask::MIDDLE)
                    }
                    (ElementState::Released, MouseButton::Middle) => {
                        self.mouse.buttons.remove(ButtonMask::MIDDLE)
                    }
                    (ElementState::Pressed, MouseButton::Right) => {
                        self.mouse.buttons.insert(ButtonMask::RIGHT)
                    }
                    (ElementState::Released, MouseButton::Right) => {
                        self.mouse.buttons.remove(ButtonMask::RIGHT)
                    }
                    _ => {}
                }
                let (fx, fy) = (self.coord_mapper)(x, y);
                out.push(ClientCommand::Pointer {
                    x: fx,
                    y: fy,
                    buttons: self.mouse.buttons.bits(),
                });
            }
            MouseWheel { delta, .. } => {
                // Map wheel to buttons 4 (up) and 5 (down) via press+release pairs
                let (x, y) = self.mouse.pos();
                let (fx, fy) = (self.coord_mapper)(x, y);
                let activate = |mask: u8| -> (ClientCommand, ClientCommand) {
                    (
                        ClientCommand::Pointer {
                            x: fx,
                            y: fy,
                            buttons: self.mouse.buttons.bits() | mask,
                        },
                        ClientCommand::Pointer {
                            x: fx,
                            y: fy,
                            buttons: self.mouse.buttons.bits(),
                        },
                    )
                };
                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        if *y > 0.0 {
                            let (down, up) = activate(ButtonMask::WHEEL_UP.bits());
                            out.push(down);
                            out.push(up);
                        } else if *y < 0.0 {
                            let (down, up) = activate(ButtonMask::WHEEL_DOWN.bits());
                            out.push(down);
                            out.push(up);
                        }
                    }
                    MouseScrollDelta::PixelDelta(pos) => {
                        let y = pos.y;
                        if y > 0.0 {
                            let (down, up) = activate(ButtonMask::WHEEL_UP.bits());
                            out.push(down);
                            out.push(up);
                        } else if y < 0.0 {
                            let (down, up) = activate(ButtonMask::WHEEL_DOWN.bits());
                            out.push(down);
                            out.push(up);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { input, .. } => {
                if let Some((keysym, down)) = keyboard::map_keyboard_input(input) {
                    out.push(ClientCommand::Key { key: keysym, down });
                }
            }
            ReceivedCharacter(ch) => {
                // Send printable Unicode characters as key press+release using their UCS keysym.
                if !ch.is_control() {
                    let keysym = *ch as u32;
                    out.push(ClientCommand::Key {
                        key: keysym,
                        down: true,
                    });
                    out.push(ClientCommand::Key {
                        key: keysym,
                        down: false,
                    });
                }
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::{ElementState, KeyboardInput, MouseButton, VirtualKeyCode, WindowEvent};

    #[test]
    fn test_mouse_move_generates_pointer() {
        let mut d = InputDispatcher::new();
        let cmds = d.handle_window_event(&WindowEvent::CursorMoved {
            device_id: unsafe { std::mem::transmute(0usize) },
            position: winit::dpi::PhysicalPosition::new(100.0, 200.0),
            modifiers: winit::event::ModifiersState::empty(),
        });
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            ClientCommand::Pointer { x, y, buttons } => {
                assert_eq!((*x, *y, *buttons), (100, 200, 0));
            }
            _ => panic!("expected pointer"),
        }
    }

    #[test]
    fn test_left_button_mask() {
        let mut d = InputDispatcher::new();
        // Move first
        let _ = d.handle_window_event(&WindowEvent::CursorMoved {
            device_id: unsafe { std::mem::transmute(0usize) },
            position: winit::dpi::PhysicalPosition::new(10.0, 10.0),
            modifiers: winit::event::ModifiersState::empty(),
        });
        // Press
        let cmds = d.handle_window_event(&WindowEvent::MouseInput {
            device_id: unsafe { std::mem::transmute(0usize) },
            state: ElementState::Pressed,
            button: MouseButton::Left,
            modifiers: winit::event::ModifiersState::empty(),
        });
        assert!(!cmds.is_empty());
        // Release
        let cmds2 = d.handle_window_event(&WindowEvent::MouseInput {
            device_id: unsafe { std::mem::transmute(0usize) },
            state: ElementState::Released,
            button: MouseButton::Left,
            modifiers: winit::event::ModifiersState::empty(),
        });
        assert!(!cmds2.is_empty());
    }

    #[test]
    fn test_key_mapping_basic() {
        let ev = KeyboardInput {
            scancode: 0,
            state: ElementState::Pressed,
            virtual_keycode: Some(VirtualKeyCode::Return),
            modifiers: winit::event::ModifiersState::empty(),
        };
        let mapped = keyboard::map_keyboard_input(&ev).unwrap();
        assert_eq!(mapped, (0xFF0D, true));
    }
}
