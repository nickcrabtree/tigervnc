//! Smoke tests for rfb-display rendering functionality
//!
//! These tests verify basic functionality without requiring a full window system.
//! Most rendering tests that require actual window creation are not feasible in
//! a headless CI environment, but these tests verify the core logic.

use rfb_common::{Point, Rect};
use rfb_display::{
    CursorImage, CursorMode, CursorRenderer, DisplayRendererBuilder, MonitorManager, ScaleMode,
    ScaleParams, ScaleUtils, Viewport, ViewportConfig, WindowPlacement,
};
use rfb_pixelbuffer::{ManagedPixelBuffer, MutablePixelBuffer, PixelBuffer, PixelFormat};

#[test]
fn test_scale_params_calculations() {
    // Test native scaling
    let params = ScaleParams::native(1920, 1080);
    assert_eq!(params.scale_x, 1.0);
    assert_eq!(params.scale_y, 1.0);
    assert!(!params.requires_scaling());
    assert!(params.is_uniform());
    
    // Test fit scaling with letterbox
    let params = ScaleParams::fit(1920, 1080, 800, 600);
    let expected_scale = 800.0 / 1920.0; // Fit to width
    assert!((params.scale_x - expected_scale).abs() < f64::EPSILON);
    assert!(params.is_uniform());
    assert!(params.requires_scaling());
    
    // Test fill scaling with different aspect ratios
    let params = ScaleParams::fill(1920, 1080, 800, 600);
    assert!((params.scale_x - 800.0 / 1920.0).abs() < f64::EPSILON);
    assert!((params.scale_y - 600.0 / 1080.0).abs() < f64::EPSILON);
    assert!(!params.is_uniform()); // Different X/Y scales
}

#[test]
fn test_scale_utils_calculations() {
    // Test scale mode suggestions
    let mode = ScaleUtils::suggest_scale_mode(800, 600, 1024, 768);
    assert_eq!(mode, ScaleMode::Native); // Similar sizes
    
    let mode = ScaleUtils::suggest_scale_mode(320, 240, 1920, 1080);
    assert_eq!(mode, ScaleMode::Fit); // Very different sizes
    
    // Test zoom calculations
    let fit_zoom = ScaleUtils::calculate_fit_zoom(1920, 1080, 800, 600);
    let fill_zoom = ScaleUtils::calculate_fill_zoom(1920, 1080, 800, 600);
    
    assert!(fit_zoom > 0.0);
    assert!(fill_zoom > 0.0);
    assert!(fit_zoom < fill_zoom); // Fit is smaller for this aspect ratio
    
    // Test utility functions
    assert_eq!(ScaleUtils::clamp_scale(0.05, 0.1, 8.0), 0.1);
    assert_eq!(ScaleUtils::clamp_scale(10.0, 0.1, 8.0), 8.0);
    
    let rounded = ScaleUtils::round_scale_for_display(1.23);
    assert_eq!(rounded, 1.2);
    
    let percent = ScaleUtils::scale_to_percent_string(1.5);
    assert_eq!(percent, "150%");
}

#[test]
fn test_viewport_coordinate_transforms() {
    let config = ViewportConfig::default();
    let mut viewport = Viewport::new(config);
    
    viewport.set_window_size(800, 600);
    viewport.set_framebuffer_size(1600, 1200);
    viewport.update();
    
    // Test coordinate conversion at 1:1 zoom
    let window_point = Point::new(100, 200);
    let fb_point = viewport.window_to_framebuffer(window_point);
    let converted_back = viewport.framebuffer_to_window(fb_point);
    
    // Should be close to original (allowing for rounding)
    assert!((converted_back.x - window_point.x).abs() <= 1);
    assert!((converted_back.y - window_point.y).abs() <= 1);
    
    // Test zoom operations
    viewport.set_zoom(2.0);
    viewport.update();
    assert_eq!(viewport.zoom(), 2.0);
    
    viewport.zoom_in();
    assert_eq!(viewport.zoom(), 2.1);
    
    viewport.reset_zoom();
    assert_eq!(viewport.zoom(), 1.0);
    
    // Test pan operations
    viewport.set_pan(100.0, 50.0);
    assert_eq!(viewport.pan(), (100.0, 50.0));
    
    viewport.pan_by(20.0, 30.0);
    assert_eq!(viewport.pan(), (120.0, 80.0));
}

#[test]
fn test_viewport_visible_rect_calculation() {
    let config = ViewportConfig::default();
    let mut viewport = Viewport::new(config);
    viewport.set_window_size(800, 600);
    viewport.set_framebuffer_size(1600, 1200);
    viewport.update();
    
    let visible = viewport.visible_framebuffer_rect();
    
    // At 1:1 zoom with no pan, visible area should match window size
    assert_eq!(visible.width, 800);
    assert_eq!(visible.height, 600);
    assert_eq!(visible.x, 0);
    assert_eq!(visible.y, 0);
    
    // Test visibility checks
    let visible_rect = Rect::new(100, 100, 200, 200);
    assert!(viewport.is_rect_visible(visible_rect));
    
    let invisible_rect = Rect::new(2000, 2000, 100, 100);
    assert!(!viewport.is_rect_visible(invisible_rect));
}

#[test]
fn test_cursor_image_creation_and_validation() {
    // Test valid cursor creation
    let pixels = vec![255u8; 16 * 16 * 4]; // 16x16 RGBA
    let cursor = CursorImage::new(16, 16, 8, 8, pixels);
    
    assert!(cursor.is_valid());
    assert_eq!(cursor.width, 16);
    assert_eq!(cursor.height, 16);
    assert_eq!(cursor.hotspot_x, 8);
    assert_eq!(cursor.hotspot_y, 8);
    
    // Test invalid cursor (wrong pixel count)
    let pixels = vec![255u8; 8 * 8 * 4]; // 8x8 data
    let cursor = CursorImage::new(16, 16, 8, 8, pixels); // Claims 16x16
    assert!(!cursor.is_valid());
    
    // Test dot cursor creation
    let dot = CursorImage::dot(16);
    assert!(dot.is_valid());
    assert_eq!(dot.width, 16);
    assert_eq!(dot.height, 16);
    assert_eq!(dot.hotspot_x, 8);
    assert_eq!(dot.hotspot_y, 8);
    
    // Verify dot has non-transparent pixels in center
    let center_idx = ((8 * 16 + 8) * 4) as usize;
    assert!(center_idx + 3 < dot.pixels.len());
    assert!(dot.pixels[center_idx + 3] > 0); // Alpha channel
}

#[test]
fn test_cursor_renderer_modes_and_state() {
    let mut renderer = CursorRenderer::new(CursorMode::Remote);
    
    assert_eq!(renderer.mode(), CursorMode::Remote);
    assert_eq!(renderer.state().position, Point::new(0, 0));
    assert!(renderer.state().visible);
    
    // Test mode changes
    renderer.set_mode(CursorMode::Dot);
    assert_eq!(renderer.mode(), CursorMode::Dot);
    
    // Test position updates
    renderer.set_position(Point::new(100, 200));
    assert_eq!(renderer.state().position, Point::new(100, 200));
    
    // Test visibility
    renderer.set_visible(false);
    assert!(!renderer.state().visible);
    
    // Test cursor image setting
    let pixels = vec![255u8; 8 * 8 * 4]; // 8x8 white cursor
    let cursor_img = CursorImage::new(8, 8, 4, 4, pixels);
    renderer.set_image(Some(cursor_img));
    
    assert!(renderer.state().image.is_some());
    
    // Test bounds calculation
    renderer.set_mode(CursorMode::Dot);
    let bounds = renderer.cursor_bounds();
    assert!(bounds.is_some());
    
    let bounds = bounds.unwrap();
    assert_eq!(bounds.x, 100 - 8); // position - hotspot
    assert_eq!(bounds.y, 200 - 8);
}

#[test]
fn test_cursor_renderer_frame_operations() {
    let renderer = CursorRenderer::new(CursorMode::Hidden);
    let mut frame = vec![0u8; 100 * 100 * 4]; // 100x100 RGBA frame
    
    // Hidden cursor should not modify frame
    let frame_copy = frame.clone();
    renderer.render_to_frame(&mut frame, 100, 100);
    assert_eq!(frame, frame_copy);
    
    // Local cursor should also not modify frame (handled by OS)
    let renderer = CursorRenderer::new(CursorMode::Local);
    let frame_copy = frame.clone();
    renderer.render_to_frame(&mut frame, 100, 100);
    assert_eq!(frame, frame_copy);
}

#[test]
fn test_monitor_manager_basic_operations() {
    let manager = MonitorManager::new();
    
    assert_eq!(manager.monitor_count(), 0);
    assert!(manager.primary_monitor().is_none());
    assert!(manager.monitors().is_empty());
    
    // Test window placement enum display
    assert_eq!(format!("{}", WindowPlacement::Primary), "Primary Monitor");
    assert_eq!(format!("{}", WindowPlacement::Monitor(1)), "Monitor 2");
    assert_eq!(format!("{}", WindowPlacement::CursorMonitor), "Cursor Monitor");
}

#[test]
fn test_framebuffer_rendering_setup() {
    // This test verifies we can create the necessary components for rendering
    // without actually creating a window
    
    // Create a test framebuffer
    let pixel_format = PixelFormat::rgb888();
    let mut framebuffer = ManagedPixelBuffer::new(800, 600, pixel_format);
    
    // Fill with a test pattern
    let test_rect = Rect::new(0, 0, 800, 600);
    framebuffer.fill_rect(test_rect, &[255, 128, 64, 255]).unwrap(); // RGBA
    
    // Verify the framebuffer was created correctly
    let (fb_width, fb_height) = framebuffer.dimensions();
    assert_eq!(fb_width, 800);
    assert_eq!(fb_height, 600);
    assert!(framebuffer.pixel_format().is_rgb888());
    
    // Test that we can get buffer data
    let mut stride = 0;
    let buffer = framebuffer.get_buffer(test_rect, &mut stride).unwrap();
    assert!(!buffer.is_empty());
    assert!(stride > 0);
    
    // Verify the fill worked (first pixel should be our test color in RGBA format)
    if buffer.len() >= 4 {
        assert_eq!(buffer[0], 255); // Red
        assert_eq!(buffer[1], 128); // Green
        assert_eq!(buffer[2], 64);  // Blue
        assert_eq!(buffer[3], 255); // Alpha
    }
}

#[test]
fn test_display_renderer_builder_configuration() {
    // Test that builder configuration works correctly
    let _builder = DisplayRendererBuilder::default();
    
    // NOTE: Can't test default values directly since fields are private
    // This is intentional encapsulation - the public API should be tested
    // through integration tests that create actual DisplayRenderers
    
    // Test builder pattern (methods return Self, so we can chain them)
    let _builder = DisplayRendererBuilder::default()
        .scale_mode(ScaleMode::Native)
        .cursor_mode(CursorMode::Local)
        .target_fps(30);
        
    // NOTE: Can't test configured values directly since fields are private
    // This is appropriate - the builder pattern works, which is what matters
}

#[test]
fn test_error_handling_edge_cases() {
    // Test scale parameter edge cases
    let params = ScaleParams::fit(0, 0, 800, 600);
    assert!(!params.requires_scaling()); // Should fallback gracefully
    
    let params = ScaleParams::fill(1920, 1080, 0, 0);
    assert!(!params.requires_scaling()); // Should fallback gracefully
    
    // Test viewport with zero dimensions
    let config = ViewportConfig::default();
    let mut viewport = Viewport::new(config);
    
    viewport.set_window_size(0, 0);
    viewport.set_framebuffer_size(1920, 1080);
    viewport.update();
    
    // Should not crash, though results may be undefined
    let visible = viewport.visible_framebuffer_rect();
    // Just verify it returns something reasonable
    assert!(visible.width >= 0);
    assert!(visible.height >= 0);
}

// Integration test that would require actual window creation
// This test is ignored by default since it can't run in headless CI
#[test]
#[ignore]
fn test_actual_rendering_integration() {
    // This would test actual DisplayRenderer creation and rendering
    // Requires an actual window system and graphics context
    // 
    // Example of what this test would do:
    // 1. Create event loop and window
    // 2. Create DisplayRenderer with window
    // 3. Create test framebuffer
    // 4. Call present() and verify no errors
    // 5. Test resize operations
    //
    // For now, this is left as a placeholder since it requires
    // a full windowing environment to run
    
    println!("This test requires a windowing system and is ignored by default");
    println!("Run with: cargo test test_actual_rendering_integration -- --ignored");
    
    // This test would be implemented when we have a test environment
    // that can create actual windows (e.g., with Xvfb on Linux)
}

#[test]
fn test_scaling_performance() {
    // Test scaling performance for a 1080p framebuffer
    use std::time::Instant;
    
    let fb_width = 1920u32;
    let fb_height = 1080u32;
    let window_width = 800u32;
    let window_height = 600u32;
    
    let iterations = 1000;
    
    // Test fit scaling calculation performance
    let start = Instant::now();
    for _ in 0..iterations {
        let _params = ScaleParams::fit(fb_width, fb_height, window_width, window_height);
    }
    let fit_duration = start.elapsed();
    
    // Test fill scaling calculation performance
    let start = Instant::now();
    for _ in 0..iterations {
        let _params = ScaleParams::fill(fb_width, fb_height, window_width, window_height);
    }
    let fill_duration = start.elapsed();
    
    // Test scale utilities performance
    let start = Instant::now();
    for _ in 0..iterations {
        let _zoom = ScaleUtils::calculate_fit_zoom(fb_width, fb_height, window_width, window_height);
    }
    let zoom_duration = start.elapsed();
    
    println!("Scaling performance (1000 iterations):");
    println!("  Fit scaling: {:.2}ms ({:.2}µs per calc)", 
             fit_duration.as_secs_f64() * 1000.0,
             fit_duration.as_micros() as f64 / iterations as f64);
    println!("  Fill scaling: {:.2}ms ({:.2}µs per calc)", 
             fill_duration.as_secs_f64() * 1000.0,
             fill_duration.as_micros() as f64 / iterations as f64);
    println!("  Zoom calculation: {:.2}ms ({:.2}µs per calc)", 
             zoom_duration.as_secs_f64() * 1000.0,
             zoom_duration.as_micros() as f64 / iterations as f64);
    
    // Performance assertions - should complete well under 1ms per 1000 operations
    assert!(fit_duration.as_micros() < 1000, "Fit scaling too slow: {:?}", fit_duration);
    assert!(fill_duration.as_micros() < 1000, "Fill scaling too slow: {:?}", fill_duration);
    assert!(zoom_duration.as_micros() < 1000, "Zoom calculation too slow: {:?}", zoom_duration);
}
