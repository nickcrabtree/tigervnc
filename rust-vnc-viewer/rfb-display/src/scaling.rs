//! Scaling strategies and DPI handling for VNC framebuffer rendering.
//!
//! This module provides different scaling modes (fit, fill, native) and filtering
//! options (nearest, linear) for rendering VNC framebuffers at different sizes.
//! It also handles high DPI displays and provides utilities for scale calculations.

use std::fmt;
use tracing::{debug, warn};

/// Scaling modes for framebuffer rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// Native 1:1 pixel mapping (no scaling)
    Native,
    /// Scale to fit within window while maintaining aspect ratio
    Fit,
    /// Scale to fill entire window (may crop or stretch)
    Fill,
}

impl Default for ScaleMode {
    fn default() -> Self {
        Self::Fit
    }
}

impl fmt::Display for ScaleMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => write!(f, "Native"),
            Self::Fit => write!(f, "Fit"),
            Self::Fill => write!(f, "Fill"),
        }
    }
}

/// Filtering options for scaling operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFilter {
    /// Nearest neighbor filtering (sharp, pixelated)
    Nearest,
    /// Linear filtering (smooth, blurred)
    Linear,
}

impl Default for ScaleFilter {
    fn default() -> Self {
        Self::Linear
    }
}

impl fmt::Display for ScaleFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nearest => write!(f, "Nearest"),
            Self::Linear => write!(f, "Linear"),
        }
    }
}

/// DPI configuration and scaling factors
#[derive(Debug, Clone)]
pub struct DpiConfig {
    /// Base DPI (typically 96 on Windows, 72 on macOS)
    pub base_dpi: f64,
    /// Current display DPI
    pub display_dpi: f64,
    /// DPI scale factor (display_dpi / base_dpi)
    pub scale_factor: f64,
    /// Whether this is a high DPI display
    pub is_high_dpi: bool,
}

impl DpiConfig {
    /// Create a new DPI configuration
    pub fn new(display_dpi: f64) -> Self {
        let base_dpi = 96.0; // Standard Windows DPI
        let scale_factor = display_dpi / base_dpi;
        let is_high_dpi = scale_factor > 1.25;

        debug!(
            "DPI config: display={:.1}, base={:.1}, scale={:.2}, high_dpi={}",
            display_dpi, base_dpi, scale_factor, is_high_dpi
        );

        Self {
            base_dpi,
            display_dpi,
            scale_factor,
            is_high_dpi,
        }
    }

    /// Create DPI config from a scale factor
    pub fn from_scale_factor(scale_factor: f64) -> Self {
        let base_dpi = 96.0;
        let display_dpi = base_dpi * scale_factor;
        let is_high_dpi = scale_factor > 1.25;

        Self {
            base_dpi,
            display_dpi,
            scale_factor,
            is_high_dpi,
        }
    }

    /// Scale a value from logical to physical pixels
    pub fn logical_to_physical(&self, logical: f64) -> f64 {
        logical * self.scale_factor
    }

    /// Scale a value from physical to logical pixels
    pub fn physical_to_logical(&self, physical: f64) -> f64 {
        physical / self.scale_factor
    }

    /// Convert logical size to physical size
    pub fn logical_size_to_physical(&self, width: u32, height: u32) -> (u32, u32) {
        (
            (width as f64 * self.scale_factor).round() as u32,
            (height as f64 * self.scale_factor).round() as u32,
        )
    }

    /// Convert physical size to logical size
    pub fn physical_size_to_logical(&self, width: u32, height: u32) -> (u32, u32) {
        (
            (width as f64 / self.scale_factor).round() as u32,
            (height as f64 / self.scale_factor).round() as u32,
        )
    }
}

impl Default for DpiConfig {
    fn default() -> Self {
        Self::new(96.0)
    }
}

/// Scale calculation results
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleParams {
    /// Scale factor for X axis
    pub scale_x: f64,
    /// Scale factor for Y axis
    pub scale_y: f64,
    /// Destination width in pixels
    pub dest_width: u32,
    /// Destination height in pixels
    pub dest_height: u32,
    /// Offset X for centering (if applicable)
    pub offset_x: i32,
    /// Offset Y for centering (if applicable)
    pub offset_y: i32,
}

impl ScaleParams {
    /// Create scale parameters for native (1:1) scaling
    pub fn native(fb_width: u32, fb_height: u32) -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            dest_width: fb_width,
            dest_height: fb_height,
            offset_x: 0,
            offset_y: 0,
        }
    }

    /// Create scale parameters for fit scaling (maintain aspect ratio)
    pub fn fit(fb_width: u32, fb_height: u32, window_width: u32, window_height: u32) -> Self {
        if fb_width == 0 || fb_height == 0 || window_width == 0 || window_height == 0 {
            warn!("Invalid dimensions for fit scaling");
            return Self::native(fb_width, fb_height);
        }

        let fb_aspect = fb_width as f64 / fb_height as f64;
        let window_aspect = window_width as f64 / window_height as f64;

        let (scale, dest_width, dest_height) = if fb_aspect > window_aspect {
            // Framebuffer is wider - fit to width
            let scale = window_width as f64 / fb_width as f64;
            let dest_height = (fb_height as f64 * scale).round() as u32;
            (scale, window_width, dest_height)
        } else {
            // Framebuffer is taller - fit to height
            let scale = window_height as f64 / fb_height as f64;
            let dest_width = (fb_width as f64 * scale).round() as u32;
            (scale, dest_width, window_height)
        };

        // Center the scaled framebuffer
        let offset_x = ((window_width as i32 - dest_width as i32) / 2).max(0);
        let offset_y = ((window_height as i32 - dest_height as i32) / 2).max(0);

        Self {
            scale_x: scale,
            scale_y: scale,
            dest_width,
            dest_height,
            offset_x,
            offset_y,
        }
    }

    /// Create scale parameters for fill scaling (stretch to fill)
    pub fn fill(fb_width: u32, fb_height: u32, window_width: u32, window_height: u32) -> Self {
        if fb_width == 0 || fb_height == 0 || window_width == 0 || window_height == 0 {
            warn!("Invalid dimensions for fill scaling");
            return Self::native(fb_width, fb_height);
        }

        let scale_x = window_width as f64 / fb_width as f64;
        let scale_y = window_height as f64 / fb_height as f64;

        Self {
            scale_x,
            scale_y,
            dest_width: window_width,
            dest_height: window_height,
            offset_x: 0,
            offset_y: 0,
        }
    }

    /// Check if scaling is required (scale factors != 1.0)
    pub fn requires_scaling(&self) -> bool {
        (self.scale_x - 1.0).abs() > f64::EPSILON || (self.scale_y - 1.0).abs() > f64::EPSILON
    }

    /// Check if uniform scaling (same scale for X and Y)
    pub fn is_uniform(&self) -> bool {
        (self.scale_x - self.scale_y).abs() < f64::EPSILON
    }

    /// Get the effective scale factor (minimum of X and Y scales)
    pub fn effective_scale(&self) -> f64 {
        self.scale_x.min(self.scale_y)
    }
}

/// Utility functions for scaling calculations
pub struct ScaleUtils;

impl ScaleUtils {
    /// Calculate scale parameters for the given mode
    pub fn calculate_scale_params(
        mode: ScaleMode,
        fb_width: u32,
        fb_height: u32,
        window_width: u32,
        window_height: u32,
    ) -> ScaleParams {
        match mode {
            ScaleMode::Native => ScaleParams::native(fb_width, fb_height),
            ScaleMode::Fit => ScaleParams::fit(fb_width, fb_height, window_width, window_height),
            ScaleMode::Fill => ScaleParams::fill(fb_width, fb_height, window_width, window_height),
        }
    }

    /// Calculate the best scale mode based on size difference
    pub fn suggest_scale_mode(
        fb_width: u32,
        fb_height: u32,
        window_width: u32,
        window_height: u32,
    ) -> ScaleMode {
        if fb_width == 0 || fb_height == 0 || window_width == 0 || window_height == 0 {
            return ScaleMode::Fit;
        }

        let fb_area = fb_width as f64 * fb_height as f64;
        let window_area = window_width as f64 * window_height as f64;
        let area_ratio = fb_area / window_area;

        // If framebuffer is much smaller or larger than window, suggest scaling
        if !(0.25..=4.0).contains(&area_ratio) {
            ScaleMode::Fit
        } else {
            ScaleMode::Native
        }
    }

    /// Calculate zoom level needed to fit framebuffer in window
    pub fn calculate_fit_zoom(
        fb_width: u32,
        fb_height: u32,
        window_width: u32,
        window_height: u32,
    ) -> f64 {
        let fit_params = ScaleParams::fit(fb_width, fb_height, window_width, window_height);
        fit_params.effective_scale()
    }

    /// Calculate zoom level needed to fill window with framebuffer
    pub fn calculate_fill_zoom(
        fb_width: u32,
        fb_height: u32,
        window_width: u32,
        window_height: u32,
    ) -> f64 {
        let fill_params = ScaleParams::fill(fb_width, fb_height, window_width, window_height);
        fill_params.scale_x.max(fill_params.scale_y)
    }

    /// Clamp scale factor to reasonable bounds
    pub fn clamp_scale(scale: f64, min_scale: f64, max_scale: f64) -> f64 {
        scale.clamp(min_scale, max_scale)
    }

    /// Round scale factor to common increments for UI display
    pub fn round_scale_for_display(scale: f64) -> f64 {
        // Round to nearest 0.1 for scales < 2.0, nearest 0.25 for larger scales
        if scale < 2.0 {
            (scale * 10.0).round() / 10.0
        } else {
            (scale * 4.0).round() / 4.0
        }
    }

    /// Convert scale factor to percentage string
    pub fn scale_to_percent_string(scale: f64) -> String {
        format!("{:.0}%", scale * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_mode_display() {
        assert_eq!(format!("{}", ScaleMode::Native), "Native");
        assert_eq!(format!("{}", ScaleMode::Fit), "Fit");
        assert_eq!(format!("{}", ScaleMode::Fill), "Fill");
    }

    #[test]
    fn test_scale_filter_display() {
        assert_eq!(format!("{}", ScaleFilter::Nearest), "Nearest");
        assert_eq!(format!("{}", ScaleFilter::Linear), "Linear");
    }

    #[test]
    fn test_dpi_config_creation() {
        let config = DpiConfig::new(144.0);

        assert_eq!(config.base_dpi, 96.0);
        assert_eq!(config.display_dpi, 144.0);
        assert_eq!(config.scale_factor, 1.5);
        assert!(config.is_high_dpi);
    }

    #[test]
    fn test_dpi_config_from_scale() {
        let config = DpiConfig::from_scale_factor(2.0);

        assert_eq!(config.scale_factor, 2.0);
        assert_eq!(config.display_dpi, 192.0);
        assert!(config.is_high_dpi);
    }

    #[test]
    fn test_dpi_conversions() {
        let config = DpiConfig::from_scale_factor(2.0);

        assert_eq!(config.logical_to_physical(100.0), 200.0);
        assert_eq!(config.physical_to_logical(200.0), 100.0);

        let (phys_w, phys_h) = config.logical_size_to_physical(800, 600);
        assert_eq!((phys_w, phys_h), (1600, 1200));

        let (log_w, log_h) = config.physical_size_to_logical(1600, 1200);
        assert_eq!((log_w, log_h), (800, 600));
    }

    #[test]
    fn test_scale_params_native() {
        let params = ScaleParams::native(1920, 1080);

        assert_eq!(params.scale_x, 1.0);
        assert_eq!(params.scale_y, 1.0);
        assert_eq!(params.dest_width, 1920);
        assert_eq!(params.dest_height, 1080);
        assert_eq!(params.offset_x, 0);
        assert_eq!(params.offset_y, 0);
        assert!(!params.requires_scaling());
        assert!(params.is_uniform());
    }

    #[test]
    fn test_scale_params_fit() {
        // Framebuffer wider than window (letterbox)
        let params = ScaleParams::fit(1920, 1080, 800, 600);

        // Should scale to fit width, with vertical centering
        let expected_scale = 800.0 / 1920.0;
        let expected_height = (1080_f64 * expected_scale).round() as u32;
        let expected_offset_y = ((600 - expected_height as i32) / 2).max(0);

        assert!((params.scale_x - expected_scale).abs() < f64::EPSILON);
        assert!((params.scale_y - expected_scale).abs() < f64::EPSILON);
        assert_eq!(params.dest_width, 800);
        assert_eq!(params.dest_height, expected_height);
        assert_eq!(params.offset_x, 0);
        assert_eq!(params.offset_y, expected_offset_y);
        assert!(params.is_uniform());
    }

    #[test]
    fn test_scale_params_fit_tall() {
        // Framebuffer taller than window (pillarbox)
        let params = ScaleParams::fit(800, 1200, 1920, 600);

        // Should scale to fit height, with horizontal centering
        let expected_scale = 600.0 / 1200.0;
        let expected_width = (800_f64 * expected_scale).round() as u32;
        let expected_offset_x = ((1920 - expected_width as i32) / 2).max(0);

        assert!((params.scale_x - expected_scale).abs() < f64::EPSILON);
        assert!((params.scale_y - expected_scale).abs() < f64::EPSILON);
        assert_eq!(params.dest_width, expected_width);
        assert_eq!(params.dest_height, 600);
        assert_eq!(params.offset_x, expected_offset_x);
        assert_eq!(params.offset_y, 0);
        assert!(params.is_uniform());
    }

    #[test]
    fn test_scale_params_fill() {
        let params = ScaleParams::fill(1920, 1080, 800, 600);

        let expected_scale_x = 800.0 / 1920.0;
        let expected_scale_y = 600.0 / 1080.0;

        assert!((params.scale_x - expected_scale_x).abs() < f64::EPSILON);
        assert!((params.scale_y - expected_scale_y).abs() < f64::EPSILON);
        assert_eq!(params.dest_width, 800);
        assert_eq!(params.dest_height, 600);
        assert_eq!(params.offset_x, 0);
        assert_eq!(params.offset_y, 0);
        assert!(params.requires_scaling());
        assert!(!params.is_uniform()); // Different X and Y scales
    }

    #[test]
    fn test_scale_utils_calculate() {
        let params = ScaleUtils::calculate_scale_params(ScaleMode::Fit, 1920, 1080, 800, 600);
        let fit_params = ScaleParams::fit(1920, 1080, 800, 600);

        assert_eq!(params, fit_params);
    }

    #[test]
    fn test_scale_utils_suggest_mode() {
        // Similar sizes - suggest native
        let mode = ScaleUtils::suggest_scale_mode(800, 600, 1024, 768);
        assert_eq!(mode, ScaleMode::Native);

        // Very different sizes - suggest fit
        let mode = ScaleUtils::suggest_scale_mode(320, 240, 1920, 1080);
        assert_eq!(mode, ScaleMode::Fit);

        let mode = ScaleUtils::suggest_scale_mode(3840, 2160, 800, 600);
        assert_eq!(mode, ScaleMode::Fit);
    }

    #[test]
    fn test_scale_utils_zoom_calculations() {
        let fit_zoom = ScaleUtils::calculate_fit_zoom(1920, 1080, 800, 600);
        let fill_zoom = ScaleUtils::calculate_fill_zoom(1920, 1080, 800, 600);

        // Fit zoom should be smaller than fill zoom for this case
        assert!(fit_zoom < fill_zoom);
        assert!(fit_zoom > 0.0);
        assert!(fill_zoom > 0.0);
    }

    #[test]
    fn test_scale_utils_clamping() {
        assert_eq!(ScaleUtils::clamp_scale(0.05, 0.1, 8.0), 0.1);
        assert_eq!(ScaleUtils::clamp_scale(10.0, 0.1, 8.0), 8.0);
        assert_eq!(ScaleUtils::clamp_scale(2.0, 0.1, 8.0), 2.0);
    }

    #[test]
    fn test_scale_utils_rounding() {
        assert_eq!(ScaleUtils::round_scale_for_display(1.23), 1.2);
        assert_eq!(ScaleUtils::round_scale_for_display(1.27), 1.3);
        assert_eq!(ScaleUtils::round_scale_for_display(3.1), 3.0);
        assert_eq!(ScaleUtils::round_scale_for_display(3.3), 3.25);
    }

    #[test]
    fn test_scale_utils_percent_string() {
        assert_eq!(ScaleUtils::scale_to_percent_string(1.0), "100%");
        assert_eq!(ScaleUtils::scale_to_percent_string(1.5), "150%");
        assert_eq!(ScaleUtils::scale_to_percent_string(0.5), "50%");
    }

    #[test]
    fn test_scale_params_edge_cases() {
        // Zero dimensions should not panic
        let params = ScaleParams::fit(0, 0, 800, 600);
        assert!(!params.requires_scaling());

        let params = ScaleParams::fill(1920, 1080, 0, 0);
        assert!(!params.requires_scaling());
    }

    #[test]
    fn test_dpi_config_standard_dpi() {
        let config = DpiConfig::new(96.0);

        assert_eq!(config.scale_factor, 1.0);
        assert!(!config.is_high_dpi);
    }

    #[test]
    fn test_effective_scale() {
        let params = ScaleParams {
            scale_x: 1.5,
            scale_y: 2.0,
            dest_width: 800,
            dest_height: 600,
            offset_x: 0,
            offset_y: 0,
        };

        assert_eq!(params.effective_scale(), 1.5); // Minimum of X and Y
    }
}
