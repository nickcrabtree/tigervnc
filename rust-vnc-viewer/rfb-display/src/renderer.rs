//! Core rendering functionality using pixels/wgpu for efficient framebuffer presentation.
//!
//! The `DisplayRenderer` is the main entry point for rendering VNC framebuffers to a window.
//! It uses the `pixels` crate which provides a simple interface over wgpu/Metal for
//! high-performance 2D rendering.

use anyhow::Context;
use pixels::{Pixels, SurfaceTexture};
use rfb_pixelbuffer::PixelBuffer;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace, warn};
use winit::{
    dpi::PhysicalSize,
    window::Window,
};

use crate::{CursorMode, CursorRenderer, ScaleMode, Viewport, ViewportConfig};

/// Errors that can occur during rendering operations
#[derive(Error, Debug)]
pub enum RenderError {
    #[error("Failed to initialize pixels surface: {0}")]
    PixelsInitError(#[from] pixels::Error),
    
    #[error("Window operation failed: {0}")]
    WindowError(String),
    
    #[error("Invalid framebuffer format: {0}")]
    InvalidFormat(String),
    
    #[error("Rendering failed: {0}")]
    RenderFailed(#[from] anyhow::Error),
    
    #[error("Viewport operation failed: {0}")]
    ViewportError(String),
}

/// Builder for configuring a DisplayRenderer
pub struct DisplayRendererBuilder {
    scale_mode: ScaleMode,
    cursor_mode: CursorMode,
    viewport_config: ViewportConfig,
    target_fps: u32,
}

impl Default for DisplayRendererBuilder {
    fn default() -> Self {
        Self {
            scale_mode: ScaleMode::Fit,
            cursor_mode: CursorMode::Remote,
            viewport_config: ViewportConfig::default(),
            target_fps: 60,
        }
    }
}

impl DisplayRendererBuilder {
    /// Set the scaling mode for the rendered framebuffer
    pub fn scale_mode(mut self, mode: ScaleMode) -> Self {
        self.scale_mode = mode;
        self
    }
    
    /// Set the cursor rendering mode
    pub fn cursor_mode(mut self, mode: CursorMode) -> Self {
        self.cursor_mode = mode;
        self
    }
    
    /// Set viewport configuration
    pub fn viewport_config(mut self, config: ViewportConfig) -> Self {
        self.viewport_config = config;
        self
    }
    
    /// Set target FPS for rendering
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.target_fps = fps;
        self
    }
    
    /// Build the DisplayRenderer with the specified window
    pub async fn build_for_window(self, window: Arc<Window>) -> Result<DisplayRenderer, RenderError> {
        DisplayRenderer::new_with_window(window, self).await
    }
}

/// Main renderer for VNC framebuffers using pixels/wgpu
pub struct DisplayRenderer {
    /// The pixels surface for rendering
    pixels: Pixels,
    /// Window being rendered to
    window: Arc<Window>,
    /// Current framebuffer dimensions
    framebuffer_size: (u32, u32),
    /// Viewport manager for pan/zoom/scroll
    viewport: Viewport,
    /// Cursor renderer
    cursor_renderer: CursorRenderer,
    /// Current scale mode
    scale_mode: ScaleMode,
    /// Target FPS
    target_fps: u32,
    /// Performance statistics
    frame_count: u64,
    last_fps_update: std::time::Instant,
}

impl DisplayRenderer {
    /// Create a new DisplayRenderer builder
    pub fn new() -> DisplayRendererBuilder {
        DisplayRendererBuilder::default()
    }
    
    /// Create a DisplayRenderer for the specified window
    async fn new_with_window(
        window: Arc<Window>,
        config: DisplayRendererBuilder,
    ) -> Result<Self, RenderError> {
        let window_size = window.inner_size();
        debug!(
            "Creating DisplayRenderer for window {}x{}",
            window_size.width, window_size.height
        );
        
        // Create the pixels surface
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &*window);
        let pixels = Pixels::new_async(window_size.width, window_size.height, surface_texture)
            .await
            .context("Failed to create pixels surface")?;
        
        // Initialize viewport
        let viewport = Viewport::new(config.viewport_config);
        
        // Initialize cursor renderer
        let cursor_renderer = CursorRenderer::new(config.cursor_mode);
        
        debug!("DisplayRenderer initialized successfully");
        
        Ok(Self {
            pixels,
            window,
            framebuffer_size: (0, 0),
            viewport,
            cursor_renderer,
            scale_mode: config.scale_mode,
            target_fps: config.target_fps,
            frame_count: 0,
            last_fps_update: std::time::Instant::now(),
        })
    }
    
    /// Handle window resize events
    pub fn resize(&mut self, new_size: PhysicalSize<u32>) -> Result<(), RenderError> {
        debug!("Resizing renderer to {}x{}", new_size.width, new_size.height);
        
        if new_size.width == 0 || new_size.height == 0 {
            warn!("Attempted to resize to zero-size window, ignoring");
            return Ok(());
        }
        
        self.pixels
            .resize_surface(new_size.width, new_size.height)
            .context("Failed to resize pixels surface")?;
        
        // Update viewport for new window size
        self.viewport.set_window_size(new_size.width, new_size.height);
        
        Ok(())
    }
    
    /// Set the framebuffer dimensions (called when VNC server sends new size)
    pub fn set_framebuffer_size(&mut self, width: u32, height: u32) {
        if self.framebuffer_size != (width, height) {
            debug!("Framebuffer size changed to {}x{}", width, height);
            self.framebuffer_size = (width, height);
            self.viewport.set_framebuffer_size(width, height);
        }
    }
    
    /// Present a framebuffer to the screen
    pub fn present<P>(&mut self, framebuffer: &P) -> Result<(), RenderError>
    where
        P: PixelBuffer,
    {
        trace!("Presenting framebuffer");
        
        // Ensure framebuffer is RGB888 format
        let pixel_format = framebuffer.pixel_format();
        if !pixel_format.is_rgb888() {
            return Err(RenderError::InvalidFormat(
                "Framebuffer must be RGB888 format".to_string(),
            ));
        }
        
        let (fb_width, fb_height) = framebuffer.dimensions();
        self.set_framebuffer_size(fb_width as u32, fb_height as u32);
        
        // Get texture dimensions and viewport state before borrowing frame
        let texture_width = self.pixels.texture().width();
        let texture_height = self.pixels.texture().height();
        let viewport_state = self.viewport.state().clone();
        
        // Get the pixels frame buffer
        let frame = self.pixels.frame_mut();
        
        // Clear the frame
        frame.fill(0);
        
        // Render the framebuffer with current viewport and scaling
        Self::render_framebuffer_to_frame_static(
            framebuffer,
            frame,
            texture_width,
            texture_height,
            &viewport_state,
            self.framebuffer_size,
            self.scale_mode,
        )?;
        
        // Render cursor on top
        self.cursor_renderer.render_to_frame(
            frame,
            texture_width,
            texture_height,
        );
        
        // Release the mutable borrow before calling render
        let _ = frame;
        
        // Present the frame
        self.pixels
            .render()
            .context("Failed to present pixels frame")?;
        
        // Update performance statistics
        self.update_fps_stats();
        
        Ok(())
    }
    
    /// Render the VNC framebuffer to the pixels frame with scaling and viewport (static version)
    fn render_framebuffer_to_frame_static<P>(
        framebuffer: &P,
        frame: &mut [u8],
        texture_width: u32,
        texture_height: u32,
        viewport_state: &crate::ViewportState,
        framebuffer_size: (u32, u32),
        scale_mode: ScaleMode,
    ) -> Result<(), RenderError>
    where
        P: PixelBuffer,
    {
        let (frame_width, frame_height) = (
            texture_width as i32,
            texture_height as i32,
        );
        
        // Get framebuffer data
        let (fb_width, fb_height) = framebuffer.dimensions();
        let fb_rect = rfb_common::Rect::new(0, 0, fb_width, fb_height);
        let mut stride = 0;
        let fb_data = framebuffer.get_buffer(fb_rect, &mut stride)
            .context("Failed to get framebuffer data")?;
        
        // Calculate bytes per pixel (should be 4 for RGB888 in pixels crate)
        let bytes_per_pixel = framebuffer.pixel_format().bytes_per_pixel() as usize;
        
        trace!(
            "Rendering framebuffer {}x{} (stride={}) to frame {}x{}",
            fb_width,
            fb_height,
            stride,
            frame_width,
            frame_height
        );
        
        // Perform scaling and blitting based on viewport
        match scale_mode {
            ScaleMode::Native => {
                Self::render_native_scale_static(
                    fb_data,
                    stride as usize,
                    bytes_per_pixel,
                    frame,
                    frame_width,
                    frame_height,
                    viewport_state,
                    framebuffer_size,
                )?;
            }
            ScaleMode::Fit => {
                Self::render_fit_scale_static(
                    fb_data,
                    stride as usize,
                    bytes_per_pixel,
                    frame,
                    frame_width,
                    frame_height,
                    framebuffer_size,
                )?;
            }
            ScaleMode::Fill => {
                Self::render_fill_scale_static(
                    fb_data,
                    stride as usize,
                    bytes_per_pixel,
                    frame,
                    frame_width,
                    frame_height,
                    framebuffer_size,
                )?;
            }
        }
        
        Ok(())
    }
    
    /// Render at native (1:1) scale (static version)
    fn render_native_scale_static(
        fb_data: &[u8],
        stride: usize,
        bytes_per_pixel: usize,
        frame: &mut [u8],
        frame_width: i32,
        frame_height: i32,
        viewport_state: &crate::ViewportState,
        framebuffer_size: (u32, u32),
    ) -> Result<(), RenderError> {
        let (fb_width, fb_height) = framebuffer_size;
        
        // Calculate visible region based on viewport offset
        let src_x = viewport_state.offset_x.max(0) as usize;
        let src_y = viewport_state.offset_y.max(0) as usize;
        let dst_x = (-viewport_state.offset_x).max(0) as usize;
        let dst_y = (-viewport_state.offset_y).max(0) as usize;
        
        let copy_width = ((fb_width as i32 - src_x as i32).min(frame_width - dst_x as i32)).max(0) as usize;
        let copy_height = ((fb_height as i32 - src_y as i32).min(frame_height - dst_y as i32)).max(0) as usize;
        
        // Copy pixels line by line
        for y in 0..copy_height {
            let src_row_start = (src_y + y) * stride * bytes_per_pixel + src_x * bytes_per_pixel;
            let dst_row_start = (dst_y + y) * frame_width as usize * 4 + dst_x * 4; // 4 bytes per pixel in frame
            
            if src_row_start + copy_width * bytes_per_pixel <= fb_data.len()
                && dst_row_start + copy_width * 4 <= frame.len()
            {
                // Convert RGB to RGBA for pixels crate
                for x in 0..copy_width {
                    let src_idx = src_row_start + x * bytes_per_pixel;
                    let dst_idx = dst_row_start + x * 4;
                    
                    if bytes_per_pixel >= 3 {
                        frame[dst_idx] = fb_data[src_idx + 2];     // B
                        frame[dst_idx + 1] = fb_data[src_idx + 1]; // G
                        frame[dst_idx + 2] = fb_data[src_idx];     // R
                        frame[dst_idx + 3] = 255;                  // A
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Render with fit scaling (maintain aspect ratio, fit in window) - static version
    fn render_fit_scale_static(
        fb_data: &[u8],
        stride: usize,
        bytes_per_pixel: usize,
        frame: &mut [u8],
        frame_width: i32,
        frame_height: i32,
        framebuffer_size: (u32, u32),
    ) -> Result<(), RenderError> {
        let (fb_width, fb_height) = framebuffer_size;
        
        // Calculate fit scaling parameters
        let scale_params = crate::ScaleParams::fit(
            fb_width,
            fb_height,
            frame_width as u32,
            frame_height as u32,
        );
        
        Self::render_scaled_static(
            fb_data,
            stride,
            bytes_per_pixel,
            frame,
            frame_width,
            frame_height,
            &scale_params,
            framebuffer_size,
        )
    }
    
    /// Render with fill scaling (stretch to fill window) - static version
    fn render_fill_scale_static(
        fb_data: &[u8],
        stride: usize,
        bytes_per_pixel: usize,
        frame: &mut [u8],
        frame_width: i32,
        frame_height: i32,
        framebuffer_size: (u32, u32),
    ) -> Result<(), RenderError> {
        let (fb_width, fb_height) = framebuffer_size;
        
        // Calculate fill scaling parameters
        let scale_params = crate::ScaleParams::fill(
            fb_width,
            fb_height,
            frame_width as u32,
            frame_height as u32,
        );
        
        Self::render_scaled_static(
            fb_data,
            stride,
            bytes_per_pixel,
            frame,
            frame_width,
            frame_height,
            &scale_params,
            framebuffer_size,
        )
    }
    
    /// Generic scaled rendering implementation with bilinear filtering
    fn render_scaled_static(
        fb_data: &[u8],
        stride: usize,
        bytes_per_pixel: usize,
        frame: &mut [u8],
        frame_width: i32,
        frame_height: i32,
        scale_params: &crate::ScaleParams,
        framebuffer_size: (u32, u32),
    ) -> Result<(), RenderError> {
        let (fb_width, fb_height) = framebuffer_size;
        
        // If no scaling is needed, use the fast path
        if !scale_params.requires_scaling() {
            return Self::render_native_scale_static(
                fb_data,
                stride,
                bytes_per_pixel,
                frame,
                frame_width,
                frame_height,
                &crate::ViewportState {
                    window_width: frame_width as u32,
                    window_height: frame_height as u32,
                    framebuffer_width: fb_width,
                    framebuffer_height: fb_height,
                    pan_zoom: crate::PanZoomState::default(),
                    offset_x: scale_params.offset_x,
                    offset_y: scale_params.offset_y,
                    scale_x: 1.0,
                    scale_y: 1.0,
                },
                framebuffer_size,
            );
        }
        
        // Render the scaled region
        let dst_start_x = scale_params.offset_x.max(0) as i32;
        let dst_start_y = scale_params.offset_y.max(0) as i32;
        let dst_end_x = (dst_start_x + scale_params.dest_width as i32).min(frame_width);
        let dst_end_y = (dst_start_y + scale_params.dest_height as i32).min(frame_height);
        
        for dst_y in dst_start_y..dst_end_y {
            for dst_x in dst_start_x..dst_end_x {
                // Map destination pixel to source coordinates
                let src_x_f = (dst_x - dst_start_x) as f64 / scale_params.scale_x;
                let src_y_f = (dst_y - dst_start_y) as f64 / scale_params.scale_y;
                
                // Clamp to valid source coordinates
                let src_x = (src_x_f as u32).min(fb_width.saturating_sub(1));
                let src_y = (src_y_f as u32).min(fb_height.saturating_sub(1));
                
                // Calculate source pixel index
                let src_idx = src_y as usize * stride * bytes_per_pixel + src_x as usize * bytes_per_pixel;
                let dst_idx = dst_y as usize * frame_width as usize * 4 + dst_x as usize * 4;
                
                if src_idx + bytes_per_pixel <= fb_data.len() && dst_idx + 4 <= frame.len() && bytes_per_pixel >= 3 {
                    // Convert RGB to BGRA for pixels crate
                    frame[dst_idx] = fb_data[src_idx + 2];     // B
                    frame[dst_idx + 1] = fb_data[src_idx + 1]; // G
                    frame[dst_idx + 2] = fb_data[src_idx];     // R
                    frame[dst_idx + 3] = 255;                  // A
                }
            }
        }
        
        Ok(())
    }
    
    /// Get current framebuffer size
    pub fn framebuffer_size(&self) -> (u32, u32) {
        self.framebuffer_size
    }
    
    /// Get current scale mode
    pub fn scale_mode(&self) -> ScaleMode {
        self.scale_mode
    }
    
    /// Set scale mode
    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        if self.scale_mode != mode {
            debug!("Scale mode changed to {:?}", mode);
            self.scale_mode = mode;
        }
    }
    
    /// Get viewport reference for input handling
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }
    
    /// Get mutable viewport reference for input handling
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }
    
    /// Get cursor renderer reference
    pub fn cursor_renderer(&self) -> &CursorRenderer {
        &self.cursor_renderer
    }
    
    /// Get mutable cursor renderer reference
    pub fn cursor_renderer_mut(&mut self) -> &mut CursorRenderer {
        &mut self.cursor_renderer
    }
    
    /// Update FPS statistics for performance monitoring
    fn update_fps_stats(&mut self) {
        self.frame_count += 1;
        
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_fps_update);
        
        if elapsed.as_secs() >= 1 {
            let fps = self.frame_count as f64 / elapsed.as_secs_f64();
            debug!("Rendering FPS: {:.1}", fps);
            
            self.frame_count = 0;
            self.last_fps_update = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = DisplayRendererBuilder::default();
        assert_eq!(builder.scale_mode, ScaleMode::Fit);
        assert_eq!(builder.cursor_mode, CursorMode::Remote);
        assert_eq!(builder.target_fps, 60);
    }

    #[test]
    fn test_builder_configuration() {
        let builder = DisplayRenderer::new()
            .scale_mode(ScaleMode::Native)
            .cursor_mode(CursorMode::Local)
            .target_fps(30);
            
        assert_eq!(builder.scale_mode, ScaleMode::Native);
        assert_eq!(builder.cursor_mode, CursorMode::Local);
        assert_eq!(builder.target_fps, 30);
    }
}