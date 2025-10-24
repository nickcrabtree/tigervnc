//! Cursor rendering functionality for VNC viewer.
//!
//! This module provides cursor rendering capabilities including local cursor,
//! remote cursor, and dot cursor modes. It handles cursor image composition
//! and blending with the framebuffer.

use rfb_common::{Point, Rect};
use std::fmt;
use tracing::{debug, trace, warn};

/// Cursor rendering modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMode {
    /// Hide cursor completely
    Hidden,
    /// Show local OS cursor (cursor rendered by window system)
    Local,
    /// Show remote cursor (cursor image from VNC server)
    Remote,
    /// Show simple dot cursor (small circle)
    Dot,
}

impl Default for CursorMode {
    fn default() -> Self {
        Self::Remote
    }
}

impl fmt::Display for CursorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hidden => write!(f, "Hidden"),
            Self::Local => write!(f, "Local"),
            Self::Remote => write!(f, "Remote"),
            Self::Dot => write!(f, "Dot"),
        }
    }
}

/// Cursor image data
#[derive(Debug, Clone)]
pub struct CursorImage {
    /// Cursor dimensions
    pub width: u32,
    pub height: u32,
    /// Hotspot position (relative to cursor top-left)
    pub hotspot_x: u32,
    pub hotspot_y: u32,
    /// RGBA pixel data (4 bytes per pixel)
    pub pixels: Vec<u8>,
}

impl CursorImage {
    /// Create a new cursor image
    pub fn new(width: u32, height: u32, hotspot_x: u32, hotspot_y: u32, pixels: Vec<u8>) -> Self {
        let expected_len = (width * height * 4) as usize;
        if pixels.len() != expected_len {
            warn!(
                "Cursor image pixel data length mismatch: expected {}, got {}",
                expected_len,
                pixels.len()
            );
        }
        
        Self {
            width,
            height,
            hotspot_x,
            hotspot_y,
            pixels,
        }
    }
    
    /// Create a default dot cursor (small white circle with black border)
    pub fn dot(size: u32) -> Self {
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let center = (size / 2) as i32;
        let radius = (size / 2) as i32;
        
        for y in 0..size as i32 {
            for x in 0..size as i32 {
                let dx = x - center;
                let dy = y - center;
                let distance_sq = dx * dx + dy * dy;
                let radius_sq = radius * radius;
                
                let pixel_idx = ((y * size as i32 + x) * 4) as usize;
                
                if pixel_idx + 3 < pixels.len() {
                    if distance_sq <= radius_sq {
                        // Inner circle - white
                        if distance_sq <= (radius - 1) * (radius - 1) {
                            pixels[pixel_idx] = 255;     // R
                            pixels[pixel_idx + 1] = 255; // G
                            pixels[pixel_idx + 2] = 255; // B
                            pixels[pixel_idx + 3] = 255; // A
                        }
                        // Border - black
                        else {
                            pixels[pixel_idx] = 0;       // R
                            pixels[pixel_idx + 1] = 0;   // G
                            pixels[pixel_idx + 2] = 0;   // B
                            pixels[pixel_idx + 3] = 255; // A
                        }
                    }
                    // Outside - transparent
                    else {
                        pixels[pixel_idx] = 0;       // R
                        pixels[pixel_idx + 1] = 0;   // G
                        pixels[pixel_idx + 2] = 0;   // B
                        pixels[pixel_idx + 3] = 0;   // A (transparent)
                    }
                }
            }
        }
        
        Self::new(size, size, size / 2, size / 2, pixels)
    }
    
    /// Check if the cursor image is valid
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.pixels.len() == (self.width * self.height * 4) as usize
    }
}

/// Current cursor state
#[derive(Debug, Clone)]
pub struct CursorState {
    /// Current cursor position in window coordinates
    pub position: Point,
    /// Current cursor image (if any)
    pub image: Option<CursorImage>,
    /// Whether cursor is visible
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            position: Point::new(0, 0),
            image: None,
            visible: true,
        }
    }
}

/// Cursor renderer for compositing cursors onto the framebuffer
pub struct CursorRenderer {
    mode: CursorMode,
    state: CursorState,
    dot_cursor: CursorImage,
}

impl CursorRenderer {
    /// Create a new cursor renderer with the specified mode
    pub fn new(mode: CursorMode) -> Self {
        debug!("Creating cursor renderer with mode: {}", mode);
        
        Self {
            mode,
            state: CursorState::default(),
            dot_cursor: CursorImage::dot(16), // 16x16 dot cursor
        }
    }
    
    /// Set the cursor mode
    pub fn set_mode(&mut self, mode: CursorMode) {
        if self.mode != mode {
            debug!("Cursor mode changed from {} to {}", self.mode, mode);
            self.mode = mode;
        }
    }
    
    /// Get the current cursor mode
    pub fn mode(&self) -> CursorMode {
        self.mode
    }
    
    /// Set the cursor position in window coordinates
    pub fn set_position(&mut self, position: Point) {
        if self.state.position != position {
            trace!("Cursor position changed to {:?}", position);
            self.state.position = position;
        }
    }
    
    /// Set the cursor image (from VNC server)
    pub fn set_image(&mut self, image: Option<CursorImage>) {
        if image.is_some() {
            if let Some(ref img) = image {
                if !img.is_valid() {
                    warn!("Invalid cursor image provided, ignoring");
                    return;
                }
                debug!("Cursor image updated: {}x{} hotspot=({}, {})", 
                       img.width, img.height, img.hotspot_x, img.hotspot_y);
            }
        }
        self.state.image = image;
    }
    
    /// Set cursor visibility
    pub fn set_visible(&mut self, visible: bool) {
        if self.state.visible != visible {
            debug!("Cursor visibility changed to {}", visible);
            self.state.visible = visible;
        }
    }
    
    /// Render the cursor to the frame buffer
    pub fn render_to_frame(&self, frame: &mut [u8], frame_width: u32, frame_height: u32) {
        // Don't render if cursor is hidden or invisible
        if self.mode == CursorMode::Hidden || !self.state.visible {
            return;
        }
        
        // Local cursor is handled by the window system, not rendered here
        if self.mode == CursorMode::Local {
            return;
        }
        
        let cursor_image = match self.mode {
            CursorMode::Remote => {
                // Use remote cursor image if available
                self.state.image.as_ref()
            }
            CursorMode::Dot => {
                // Use dot cursor
                Some(&self.dot_cursor)
            }
            _ => return, // Already handled above
        };
        
        if let Some(cursor) = cursor_image {
            self.render_cursor_image(cursor, frame, frame_width, frame_height);
        }
    }
    
    /// Render a cursor image to the frame buffer
    fn render_cursor_image(
        &self,
        cursor: &CursorImage,
        frame: &mut [u8],
        frame_width: u32,
        frame_height: u32,
    ) {
        // Calculate cursor render position (top-left corner)
        let cursor_x = self.state.position.x - cursor.hotspot_x as i32;
        let cursor_y = self.state.position.y - cursor.hotspot_y as i32;
        
        // Calculate clipping bounds
        let start_x = cursor_x.max(0) as u32;
        let start_y = cursor_y.max(0) as u32;
        let end_x = (cursor_x + cursor.width as i32).min(frame_width as i32) as u32;
        let end_y = (cursor_y + cursor.height as i32).min(frame_height as i32) as u32;
        
        // Render visible portion of cursor
        for y in start_y..end_y {
            for x in start_x..end_x {
                let cursor_src_x = (x as i32 - cursor_x) as u32;
                let cursor_src_y = (y as i32 - cursor_y) as u32;
                
                if cursor_src_x < cursor.width && cursor_src_y < cursor.height {
                    let src_idx = ((cursor_src_y * cursor.width + cursor_src_x) * 4) as usize;
                    let dst_idx = ((y * frame_width + x) * 4) as usize;
                    
                    if src_idx + 3 < cursor.pixels.len() && dst_idx + 3 < frame.len() {
                        let src_r = cursor.pixels[src_idx] as u16;
                        let src_g = cursor.pixels[src_idx + 1] as u16;
                        let src_b = cursor.pixels[src_idx + 2] as u16;
                        let src_a = cursor.pixels[src_idx + 3] as u16;
                        
                        if src_a > 0 {
                            if src_a == 255 {
                                // Fully opaque - direct copy
                                frame[dst_idx] = src_r as u8;     // R (but frame is BGRA)
                                frame[dst_idx + 1] = src_g as u8; // G
                                frame[dst_idx + 2] = src_b as u8; // B
                                frame[dst_idx + 3] = 255;         // A
                            } else {
                                // Alpha blending
                                let dst_r = frame[dst_idx] as u16;
                                let dst_g = frame[dst_idx + 1] as u16;
                                let dst_b = frame[dst_idx + 2] as u16;
                                
                                let inv_alpha = 255 - src_a;
                                let blended_r = (src_r * src_a + dst_r * inv_alpha) / 255;
                                let blended_g = (src_g * src_a + dst_g * inv_alpha) / 255;
                                let blended_b = (src_b * src_a + dst_b * inv_alpha) / 255;
                                
                                frame[dst_idx] = blended_r as u8;
                                frame[dst_idx + 1] = blended_g as u8;
                                frame[dst_idx + 2] = blended_b as u8;
                                frame[dst_idx + 3] = 255; // Frame alpha is always opaque
                            }
                        }
                    }
                }
            }
        }
    }
    
    /// Get the current cursor state
    pub fn state(&self) -> &CursorState {
        &self.state
    }
    
    /// Get the bounds of the cursor in window coordinates
    pub fn cursor_bounds(&self) -> Option<Rect> {
        let cursor_image = match self.mode {
            CursorMode::Remote => self.state.image.as_ref(),
            CursorMode::Dot => Some(&self.dot_cursor),
            _ => return None,
        };
        
        if let Some(cursor) = cursor_image {
            let x = self.state.position.x - cursor.hotspot_x as i32;
            let y = self.state.position.y - cursor.hotspot_y as i32;
            Some(Rect::new(x, y, cursor.width, cursor.height))
        } else {
            None
        }
    }
}

impl fmt::Debug for CursorRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorRenderer")
            .field("mode", &self.mode)
            .field("state", &self.state)
            .field("dot_cursor", &format!("{}x{}", self.dot_cursor.width, self.dot_cursor.height))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_mode_display() {
        assert_eq!(format!("{}", CursorMode::Hidden), "Hidden");
        assert_eq!(format!("{}", CursorMode::Local), "Local");
        assert_eq!(format!("{}", CursorMode::Remote), "Remote");
        assert_eq!(format!("{}", CursorMode::Dot), "Dot");
    }

    #[test]
    fn test_cursor_image_creation() {
        let pixels = vec![255u8; 4 * 4 * 4]; // 4x4 RGBA
        let cursor = CursorImage::new(4, 4, 2, 2, pixels);
        
        assert_eq!(cursor.width, 4);
        assert_eq!(cursor.height, 4);
        assert_eq!(cursor.hotspot_x, 2);
        assert_eq!(cursor.hotspot_y, 2);
        assert!(cursor.is_valid());
    }

    #[test]
    fn test_cursor_image_validation() {
        // Valid image
        let pixels = vec![255u8; 8 * 8 * 4]; // 8x8 RGBA
        let cursor = CursorImage::new(8, 8, 4, 4, pixels);
        assert!(cursor.is_valid());
        
        // Invalid image (wrong pixel count)
        let pixels = vec![255u8; 4 * 4 * 4]; // 4x4 RGBA data
        let cursor = CursorImage::new(8, 8, 4, 4, pixels); // Claims to be 8x8
        assert!(!cursor.is_valid());
        
        // Zero size
        let cursor = CursorImage::new(0, 0, 0, 0, vec![]);
        assert!(!cursor.is_valid());
    }

    #[test]
    fn test_dot_cursor_creation() {
        let dot = CursorImage::dot(16);
        
        assert_eq!(dot.width, 16);
        assert_eq!(dot.height, 16);
        assert_eq!(dot.hotspot_x, 8);
        assert_eq!(dot.hotspot_y, 8);
        assert!(dot.is_valid());
        
        // Check that the dot has some non-transparent pixels in the center
        let center_idx = ((8 * 16 + 8) * 4) as usize;
        assert!(center_idx + 3 < dot.pixels.len());
        assert!(dot.pixels[center_idx + 3] > 0); // Alpha should be non-zero at center
    }

    #[test]
    fn test_cursor_renderer_creation() {
        let renderer = CursorRenderer::new(CursorMode::Remote);
        
        assert_eq!(renderer.mode(), CursorMode::Remote);
        assert_eq!(renderer.state().position, Point::new(0, 0));
        assert!(renderer.state().visible);
        assert!(renderer.state().image.is_none());
    }

    #[test]
    fn test_cursor_mode_changes() {
        let mut renderer = CursorRenderer::new(CursorMode::Local);
        
        assert_eq!(renderer.mode(), CursorMode::Local);
        
        renderer.set_mode(CursorMode::Dot);
        assert_eq!(renderer.mode(), CursorMode::Dot);
    }

    #[test]
    fn test_cursor_position_updates() {
        let mut renderer = CursorRenderer::new(CursorMode::Remote);
        
        let new_pos = Point::new(100, 200);
        renderer.set_position(new_pos);
        
        assert_eq!(renderer.state().position, new_pos);
    }

    #[test]
    fn test_cursor_visibility() {
        let mut renderer = CursorRenderer::new(CursorMode::Remote);
        
        assert!(renderer.state().visible);
        
        renderer.set_visible(false);
        assert!(!renderer.state().visible);
        
        renderer.set_visible(true);
        assert!(renderer.state().visible);
    }

    #[test]
    fn test_cursor_image_setting() {
        let mut renderer = CursorRenderer::new(CursorMode::Remote);
        
        let pixels = vec![255u8; 8 * 8 * 4]; // 8x8 white cursor
        let cursor_img = CursorImage::new(8, 8, 4, 4, pixels);
        
        renderer.set_image(Some(cursor_img));
        
        assert!(renderer.state().image.is_some());
        let img = renderer.state().image.as_ref().unwrap();
        assert_eq!(img.width, 8);
        assert_eq!(img.height, 8);
    }

    #[test]
    fn test_cursor_bounds_calculation() {
        let mut renderer = CursorRenderer::new(CursorMode::Dot);
        renderer.set_position(Point::new(100, 200));
        
        let bounds = renderer.cursor_bounds();
        assert!(bounds.is_some());
        
        let bounds = bounds.unwrap();
        // Dot cursor is 16x16 with hotspot at (8, 8)
        assert_eq!(bounds.x, 100 - 8); // position - hotspot_x
        assert_eq!(bounds.y, 200 - 8); // position - hotspot_y
        assert_eq!(bounds.width, 16);
        assert_eq!(bounds.height, 16);
    }

    #[test]
    fn test_hidden_cursor_bounds() {
        let renderer = CursorRenderer::new(CursorMode::Hidden);
        assert!(renderer.cursor_bounds().is_none());
        
        let renderer = CursorRenderer::new(CursorMode::Local);
        assert!(renderer.cursor_bounds().is_none());
    }

    #[test]
    fn test_render_to_frame_hidden() {
        let renderer = CursorRenderer::new(CursorMode::Hidden);
        let mut frame = vec![0u8; 100 * 100 * 4]; // 100x100 RGBA frame
        
        // Rendering hidden cursor should not modify frame
        let frame_copy = frame.clone();
        renderer.render_to_frame(&mut frame, 100, 100);
        assert_eq!(frame, frame_copy);
    }

    #[test]
    fn test_render_to_frame_local() {
        let renderer = CursorRenderer::new(CursorMode::Local);
        let mut frame = vec![0u8; 100 * 100 * 4]; // 100x100 RGBA frame
        
        // Rendering local cursor should not modify frame (handled by OS)
        let frame_copy = frame.clone();
        renderer.render_to_frame(&mut frame, 100, 100);
        assert_eq!(frame, frame_copy);
    }

    #[test]
    fn test_invalid_cursor_image_rejection() {
        let mut renderer = CursorRenderer::new(CursorMode::Remote);
        
        // Try to set an invalid cursor image
        let pixels = vec![255u8; 4 * 4 * 4]; // 4x4 data
        let invalid_cursor = CursorImage::new(8, 8, 4, 4, pixels); // Claims 8x8
        
        renderer.set_image(Some(invalid_cursor));
        
        // Should remain None due to validation failure
        assert!(renderer.state().image.is_none());
    }
}