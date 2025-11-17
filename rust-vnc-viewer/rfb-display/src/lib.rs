//! # rfb-display: Efficient VNC Framebuffer Rendering
//!
//! This crate provides efficient framebuffer-to-screen rendering for VNC clients using modern
//! graphics APIs (wgpu/Metal on macOS). It supports multiple scaling modes, viewport management,
//! cursor rendering, and high DPI displays.
//!
//! ## Features
//!
//! - **Multiple scaling modes**: fit window, fill window, 1:1 native
//! - **Viewport management**: pan, zoom, scroll with smooth interactions
//! - **Cursor rendering**: local cursor, remote cursor, and dot cursor modes
//! - **Multi-monitor support**: window placement and high DPI awareness
//! - **Performance**: 60 fps target for 1080p framebuffers
//!
//! ## Example
//!
//! ```rust,no_run
//! use rfb_display::{DisplayRenderer, ViewportConfig, CursorMode, ScaleMode};
//! use rfb_pixelbuffer::ManagedPixelBuffer;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // This example shows how to use the display renderer (requires actual window)
//! // In practice, you'd create an Arc<Window> from winit
//! //
//! // let window = Arc::new(window); // from winit
//! // let mut renderer = DisplayRenderer::new()
//! //     .scale_mode(ScaleMode::Fit)
//! //     .cursor_mode(CursorMode::Remote)
//! //     .build_for_window(window).await?;
//! //
//! // let pixel_format = rfb_pixelbuffer::PixelFormat::rgb888();
//! // let mut framebuffer = ManagedPixelBuffer::new(1920, 1080, pixel_format)?;
//! //
//! // renderer.present(&framebuffer)?;
//! # Ok(())
//! # }
//! ```

mod cursor;
mod monitor;
mod renderer;
mod scaling;
mod viewport;

pub use cursor::{CursorImage, CursorMode, CursorRenderer, CursorState};
pub use monitor::{MonitorInfo, MonitorManager, WindowPlacement};
pub use renderer::{DisplayRenderer, DisplayRendererBuilder, RenderError};
pub use scaling::{DpiConfig, ScaleFilter, ScaleMode, ScaleParams, ScaleUtils};
pub use viewport::{PanZoomState, Viewport, ViewportConfig, ViewportState};

// Re-export commonly needed types from dependencies
pub use winit::{
    event::{Event, WindowEvent},
    event_loop::{EventLoop, EventLoopWindowTarget},
    window::{Window, WindowBuilder},
};

/// Common result type for display operations
pub type DisplayResult<T> = Result<T, RenderError>;

#[cfg(test)]
mod tests {
    #[test]
    fn test_crate_compiles() {
        // Basic smoke test to ensure the crate compiles
        assert_eq!(2 + 2, 4);
    }
}
