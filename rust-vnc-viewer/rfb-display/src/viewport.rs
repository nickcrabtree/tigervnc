//! Viewport management for pan, zoom, and scroll operations.
//!
//! The viewport system manages the transformation between the VNC framebuffer coordinate
//! space and the window coordinate space. It handles panning (scrolling), zooming, and
//! different scaling strategies.

use rfb_common::{Point, Rect};
use std::fmt;
use tracing::{debug, trace};

/// Configuration for viewport behavior
#[derive(Debug, Clone)]
pub struct ViewportConfig {
    /// Minimum zoom level (1.0 = native size)
    pub min_zoom: f64,
    /// Maximum zoom level  
    pub max_zoom: f64,
    /// Zoom increment/decrement step
    pub zoom_step: f64,
    /// Enable smooth scrolling
    pub smooth_scrolling: bool,
    /// Pan speed multiplier
    pub pan_speed: f64,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            min_zoom: 0.1,
            max_zoom: 8.0,
            zoom_step: 0.1,
            smooth_scrolling: true,
            pan_speed: 1.0,
        }
    }
}

/// Current state of pan and zoom operations
#[derive(Debug, Clone, PartialEq)]
pub struct PanZoomState {
    /// Current zoom level (1.0 = native size)
    pub zoom: f64,
    /// Pan offset in framebuffer pixels
    pub pan_x: f64,
    pub pan_y: f64,
}

impl Default for PanZoomState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

/// Current viewport state including transformations
#[derive(Debug, Clone)]
pub struct ViewportState {
    /// Window dimensions in pixels
    pub window_width: u32,
    pub window_height: u32,
    /// Framebuffer dimensions in pixels
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    /// Current pan/zoom state
    pub pan_zoom: PanZoomState,
    /// Calculated offset for rendering (in pixels)
    pub offset_x: i32,
    pub offset_y: i32,
    /// Calculated scale factors for rendering
    pub scale_x: f64,
    pub scale_y: f64,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            window_width: 800,
            window_height: 600,
            framebuffer_width: 800,
            framebuffer_height: 600,
            pan_zoom: PanZoomState::default(),
            offset_x: 0,
            offset_y: 0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

impl fmt::Display for ViewportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Viewport(win={}x{}, fb={}x{}, zoom={:.2}, pan={:.1},{:.1}, offset={},{})",
            self.window_width, self.window_height,
            self.framebuffer_width, self.framebuffer_height,
            self.pan_zoom.zoom, self.pan_zoom.pan_x, self.pan_zoom.pan_y,
            self.offset_x, self.offset_y
        )
    }
}

/// Viewport manager for handling pan, zoom, and coordinate transformations
pub struct Viewport {
    config: ViewportConfig,
    state: ViewportState,
    dirty: bool,
}

impl Viewport {
    /// Create a new viewport with the specified configuration
    pub fn new(config: ViewportConfig) -> Self {
        debug!("Creating viewport with config: {:?}", config);
        Self {
            config,
            state: ViewportState::default(),
            dirty: true,
        }
    }
    
    /// Set the window size (called on window resize)
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        if self.state.window_width != width || self.state.window_height != height {
            debug!("Viewport window size changed to {}x{}", width, height);
            self.state.window_width = width;
            self.state.window_height = height;
            self.dirty = true;
        }
    }
    
    /// Set the framebuffer size (called when VNC server changes size)
    pub fn set_framebuffer_size(&mut self, width: u32, height: u32) {
        if self.state.framebuffer_width != width || self.state.framebuffer_height != height {
            debug!("Viewport framebuffer size changed to {}x{}", width, height);
            self.state.framebuffer_width = width;
            self.state.framebuffer_height = height;
            self.dirty = true;
        }
    }
    
    /// Get the current viewport state, recalculating if needed
    pub fn state(&self) -> &ViewportState {
        &self.state
    }
    
    /// Get mutable viewport state and mark as dirty
    pub fn state_mut(&mut self) -> &mut ViewportState {
        self.dirty = true;
        &mut self.state
    }
    
    /// Update viewport calculations if dirty
    pub fn update(&mut self) {
        if self.dirty {
            self.recalculate_transforms();
            self.dirty = false;
        }
    }
    
    /// Set zoom level (clamped to configured min/max)
    pub fn set_zoom(&mut self, zoom: f64) {
        let clamped_zoom = zoom.clamp(self.config.min_zoom, self.config.max_zoom);
        if (self.state.pan_zoom.zoom - clamped_zoom).abs() > f64::EPSILON {
            debug!("Zoom changed from {:.2} to {:.2}", self.state.pan_zoom.zoom, clamped_zoom);
            self.state.pan_zoom.zoom = clamped_zoom;
            self.dirty = true;
        }
    }
    
    /// Zoom in by the configured step
    pub fn zoom_in(&mut self) {
        self.set_zoom(self.state.pan_zoom.zoom + self.config.zoom_step);
    }
    
    /// Zoom out by the configured step
    pub fn zoom_out(&mut self) {
        self.set_zoom(self.state.pan_zoom.zoom - self.config.zoom_step);
    }
    
    /// Reset zoom to 1.0 (native size)
    pub fn reset_zoom(&mut self) {
        self.set_zoom(1.0);
    }
    
    /// Set pan offset in framebuffer coordinates
    pub fn set_pan(&mut self, x: f64, y: f64) {
        if (self.state.pan_zoom.pan_x - x).abs() > f64::EPSILON
            || (self.state.pan_zoom.pan_y - y).abs() > f64::EPSILON
        {
            trace!("Pan changed to ({:.1}, {:.1})", x, y);
            self.state.pan_zoom.pan_x = x;
            self.state.pan_zoom.pan_y = y;
            self.dirty = true;
        }
    }
    
    /// Pan by the specified delta in framebuffer coordinates
    pub fn pan_by(&mut self, dx: f64, dy: f64) {
        let new_x = self.state.pan_zoom.pan_x + dx * self.config.pan_speed;
        let new_y = self.state.pan_zoom.pan_y + dy * self.config.pan_speed;
        self.set_pan(new_x, new_y);
    }
    
    /// Reset pan to center the framebuffer
    pub fn center(&mut self) {
        let center_x = (self.state.framebuffer_width as f64) / 2.0 - (self.state.window_width as f64) / (2.0 * self.state.pan_zoom.zoom);
        let center_y = (self.state.framebuffer_height as f64) / 2.0 - (self.state.window_height as f64) / (2.0 * self.state.pan_zoom.zoom);
        self.set_pan(center_x, center_y);
    }
    
    /// Reset both zoom and pan to defaults
    pub fn reset(&mut self) {
        debug!("Resetting viewport to defaults");
        self.state.pan_zoom = PanZoomState::default();
        self.center();
    }
    
    /// Convert window coordinates to framebuffer coordinates
    pub fn window_to_framebuffer(&self, window_point: Point) -> Point {
        let fb_x = (window_point.x as f64 / self.state.scale_x + self.state.pan_zoom.pan_x) as i32;
        let fb_y = (window_point.y as f64 / self.state.scale_y + self.state.pan_zoom.pan_y) as i32;
        Point::new(fb_x, fb_y)
    }
    
    /// Convert framebuffer coordinates to window coordinates
    pub fn framebuffer_to_window(&self, fb_point: Point) -> Point {
        let win_x = ((fb_point.x as f64 - self.state.pan_zoom.pan_x) * self.state.scale_x) as i32;
        let win_y = ((fb_point.y as f64 - self.state.pan_zoom.pan_y) * self.state.scale_y) as i32;
        Point::new(win_x, win_y)
    }
    
    /// Get the visible rectangle in framebuffer coordinates
    pub fn visible_framebuffer_rect(&self) -> Rect {
        let top_left = self.window_to_framebuffer(Point::new(0, 0));
        let bottom_right = self.window_to_framebuffer(Point::new(
            self.state.window_width as i32,
            self.state.window_height as i32,
        ));
        
        // Clamp to actual framebuffer bounds
        let x = top_left.x.max(0);
        let y = top_left.y.max(0);
        let right = bottom_right.x.min(self.state.framebuffer_width as i32);
        let bottom = bottom_right.y.min(self.state.framebuffer_height as i32);
        
        Rect::new(x, y, (right - x).max(0) as u32, (bottom - y).max(0) as u32)
    }
    
    /// Check if a framebuffer rectangle is visible in the current viewport
    pub fn is_rect_visible(&self, rect: Rect) -> bool {
        let visible = self.visible_framebuffer_rect();
        visible.intersects(rect)
    }
    
    /// Get the current zoom level
    pub fn zoom(&self) -> f64 {
        self.state.pan_zoom.zoom
    }
    
    /// Get the current pan offset
    pub fn pan(&self) -> (f64, f64) {
        (self.state.pan_zoom.pan_x, self.state.pan_zoom.pan_y)
    }
    
    /// Check if viewport needs recalculation
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    
    /// Recalculate transformation parameters based on current state
    fn recalculate_transforms(&mut self) {
        // Calculate effective scale factors
        self.state.scale_x = self.state.pan_zoom.zoom;
        self.state.scale_y = self.state.pan_zoom.zoom;
        
        // Calculate rendering offsets
        // This determines where the framebuffer origin appears in the window
        self.state.offset_x = -(self.state.pan_zoom.pan_x * self.state.scale_x) as i32;
        self.state.offset_y = -(self.state.pan_zoom.pan_y * self.state.scale_y) as i32;
        
        trace!("Recalculated transforms: {}", self.state);
    }
}

impl fmt::Debug for Viewport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Viewport")
            .field("config", &self.config)
            .field("state", &self.state)
            .field("dirty", &self.dirty)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_creation() {
        let config = ViewportConfig::default();
        let viewport = Viewport::new(config);
        
        assert_eq!(viewport.zoom(), 1.0);
        assert_eq!(viewport.pan(), (0.0, 0.0));
        assert!(viewport.is_dirty());
    }

    #[test]
    fn test_zoom_operations() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        
        // Test zoom in
        viewport.zoom_in();
        assert_eq!(viewport.zoom(), 1.1);
        
        // Test zoom out
        viewport.zoom_out();
        assert_eq!(viewport.zoom(), 1.0);
        
        // Test zoom limits
        viewport.set_zoom(10.0); // Above max
        assert_eq!(viewport.zoom(), 8.0); // Clamped to max
        
        viewport.set_zoom(0.05); // Below min
        assert_eq!(viewport.zoom(), 0.1); // Clamped to min
    }

    #[test]
    fn test_pan_operations() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        
        // Test direct pan
        viewport.set_pan(100.0, 50.0);
        assert_eq!(viewport.pan(), (100.0, 50.0));
        
        // Test pan by delta
        viewport.pan_by(20.0, 30.0);
        assert_eq!(viewport.pan(), (120.0, 80.0));
    }

    #[test]
    fn test_coordinate_conversion() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        viewport.set_window_size(800, 600);
        viewport.set_framebuffer_size(1600, 1200);
        viewport.update(); // Force recalculation
        
        // Test at 1:1 zoom (no scaling)
        let window_point = Point::new(100, 200);
        let fb_point = viewport.window_to_framebuffer(window_point);
        let converted_back = viewport.framebuffer_to_window(fb_point);
        
        // Should be close to original (allowing for rounding)
        assert!((converted_back.x - window_point.x).abs() <= 1);
        assert!((converted_back.y - window_point.y).abs() <= 1);
    }

    #[test]
    fn test_visible_rect_calculation() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        viewport.set_window_size(800, 600);
        viewport.set_framebuffer_size(1600, 1200);
        viewport.update();
        
        let visible = viewport.visible_framebuffer_rect();
        
        // At 1:1 zoom with no pan, visible area should be window size
        assert_eq!(visible.width, 800);
        assert_eq!(visible.height, 600);
        assert_eq!(visible.x, 0);
        assert_eq!(visible.y, 0);
    }

    #[test]
    fn test_rect_visibility() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        viewport.set_window_size(800, 600);
        viewport.set_framebuffer_size(1600, 1200);
        viewport.update();
        
        // Rectangle in top-left should be visible
        let visible_rect = Rect::new(100, 100, 200, 200);
        assert!(viewport.is_rect_visible(visible_rect));
        
        // Rectangle far off-screen should not be visible
        let invisible_rect = Rect::new(2000, 2000, 100, 100);
        assert!(!viewport.is_rect_visible(invisible_rect));
    }

    #[test]
    fn test_center_operation() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        viewport.set_window_size(800, 600);
        viewport.set_framebuffer_size(1600, 1200);
        
        viewport.center();
        viewport.update();
        
        // After centering, the framebuffer center should map to window center
        let fb_center = Point::new(800, 600); // Half of framebuffer size
        let window_center = viewport.framebuffer_to_window(fb_center);
        
        // Should be close to window center (allowing for rounding)
        assert!((window_center.x - 400).abs() <= 1); // Half of window width
        assert!((window_center.y - 300).abs() <= 1); // Half of window height
    }

    #[test]
    fn test_viewport_state_display() {
        let state = ViewportState {
            window_width: 1024,
            window_height: 768,
            framebuffer_width: 1920,
            framebuffer_height: 1080,
            pan_zoom: PanZoomState {
                zoom: 1.5,
                pan_x: 100.0,
                pan_y: 200.0,
            },
            offset_x: -150,
            offset_y: -300,
            scale_x: 1.5,
            scale_y: 1.5,
        };
        
        let display_str = format!("{}", state);
        assert!(display_str.contains("1024x768"));
        assert!(display_str.contains("1920x1080"));
        assert!(display_str.contains("1.50"));
        assert!(display_str.contains("100.0,200.0"));
        assert!(display_str.contains("-150,-300"));
    }

    #[test]
    fn test_reset_viewport() {
        let config = ViewportConfig::default();
        let mut viewport = Viewport::new(config);
        
        // Change zoom and pan
        viewport.set_zoom(2.0);
        viewport.set_pan(500.0, 300.0);
        
        // Reset should restore defaults
        viewport.reset();
        assert_eq!(viewport.zoom(), 1.0);
        // Pan will be set to center, not necessarily (0,0)
    }
}