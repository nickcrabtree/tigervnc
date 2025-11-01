use platform_input::{keysyms::*, KeyMapper, Modifier};
use winit::event::{ElementState, KeyboardInput, VirtualKeyCode};

/// Test comprehensive keyboard mappings.
#[test]
fn test_function_keys_mapping() {
    let input = KeyboardInput {
        scancode: 0,
        state: ElementState::Pressed,
        virtual_keycode: Some(VirtualKeyCode::F1),
        modifiers: Default::default(),
    };

    let mut mapper = KeyMapper::new();
    let result = mapper.process_key(&input).unwrap();
    assert_eq!(result.0, XK_F1);
    assert!(result.1); // Should be pressed
}

#[test]
fn test_modifier_keys_state_tracking() {
    let mut mapper = KeyMapper::new();

    // Press Ctrl
    let ctrl_press = KeyboardInput {
        scancode: 0,
        state: ElementState::Pressed,
        virtual_keycode: Some(VirtualKeyCode::LControl),
        modifiers: Default::default(),
    };
    mapper.process_key(&ctrl_press);

    // Press Alt
    let alt_press = KeyboardInput {
        scancode: 0,
        state: ElementState::Pressed,
        virtual_keycode: Some(VirtualKeyCode::LAlt),
        modifiers: Default::default(),
    };
    mapper.process_key(&alt_press);

    // Check modifier states
    assert!(mapper.is_modifier_active(Modifier::Control));
    assert!(mapper.is_modifier_active(Modifier::Alt));
    assert!(!mapper.is_modifier_active(Modifier::Shift));

    // Check bitmask
    let mask = mapper.modifier_mask();
    assert_eq!(mask & 4, 4); // Control bit
    assert_eq!(mask & 8, 8); // Alt bit
}

#[test]
fn test_key_repeat_throttling() {
    let mut mapper = KeyMapper::new();
    mapper.set_repeat_delay(100); // 100ms delay

    let key_press = KeyboardInput {
        scancode: 0,
        state: ElementState::Pressed,
        virtual_keycode: Some(VirtualKeyCode::A),
        modifiers: Default::default(),
    };

    // First press should go through
    let result1 = mapper.process_key(&key_press);
    assert!(result1.is_some());

    // Immediate repeat should be throttled
    let result2 = mapper.process_key(&key_press);
    assert!(result2.is_none());

    // After delay, should work again
    std::thread::sleep(std::time::Duration::from_millis(110));
    let result3 = mapper.process_key(&key_press);
    assert!(result3.is_some());
}
