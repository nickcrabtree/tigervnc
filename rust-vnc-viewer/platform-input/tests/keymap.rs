use platform_input::{keysyms::*, KeyMapper, Modifier};
use winit::event::{ElementState, VirtualKeyCode};

/// Test comprehensive keyboard mappings.
#[test]
fn test_function_keys_mapping() {
    let mut mapper = KeyMapper::new();

    let result = mapper
        .process_key(ElementState::Pressed, Some(VirtualKeyCode::F1))
        .unwrap();
    assert_eq!(result.0, XK_F1);
    assert!(result.1); // Should be pressed
}

#[test]
fn test_modifier_keys_state_tracking() {
    let mut mapper = KeyMapper::new();

    // Press Ctrl
    mapper.process_key(ElementState::Pressed, Some(VirtualKeyCode::LControl));

    // Press Alt
    mapper.process_key(ElementState::Pressed, Some(VirtualKeyCode::LAlt));

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

    // First press should go through
    let result1 = mapper.process_key(ElementState::Pressed, Some(VirtualKeyCode::A));
    assert!(result1.is_some());

    // Immediate repeat should be throttled
    let result2 = mapper.process_key(ElementState::Pressed, Some(VirtualKeyCode::A));
    assert!(result2.is_none());

    // After delay, should work again
    std::thread::sleep(std::time::Duration::from_millis(110));
    let result3 = mapper.process_key(ElementState::Pressed, Some(VirtualKeyCode::A));
    assert!(result3.is_some());
}
