//! Multi-monitor support and window placement functionality.
//!
//! This module provides utilities for detecting available monitors, managing
//! window placement across multiple displays, and handling high DPI scenarios
//! on different monitors.

use crate::DpiConfig;
use std::fmt;
use tracing::debug;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    monitor::{MonitorHandle, VideoMode},
};

/// Information about a display monitor
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// Monitor name (if available)
    pub name: Option<String>,
    /// Physical position of monitor in desktop coordinate system
    pub position: PhysicalPosition<i32>,
    /// Physical size of monitor in pixels
    pub size: PhysicalSize<u32>,
    /// DPI scale factor for this monitor
    pub scale_factor: f64,
    /// Whether this is the primary monitor
    pub is_primary: bool,
    /// DPI configuration for this monitor
    pub dpi_config: DpiConfig,
    /// Available video modes (resolutions)
    pub video_modes: Vec<VideoModeInfo>,
}

impl MonitorInfo {
    /// Create MonitorInfo from a winit MonitorHandle
    pub fn from_monitor_handle(monitor: &MonitorHandle, is_primary: bool) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        let scale_factor = monitor.scale_factor();
        let name = monitor.name();

        let dpi_config = DpiConfig::from_scale_factor(scale_factor);

        // Collect video modes
        let video_modes: Vec<VideoModeInfo> = monitor
            .video_modes()
            .map(VideoModeInfo::from_video_mode)
            .collect();

        debug!(
            "Monitor detected: {} {}x{} @{:.1}x scale (primary: {})",
            name.as_deref().unwrap_or("Unknown"),
            size.width,
            size.height,
            scale_factor,
            is_primary
        );

        Self {
            name,
            position,
            size,
            scale_factor,
            is_primary,
            dpi_config,
            video_modes,
        }
    }

    /// Get the logical size of the monitor
    pub fn logical_size(&self) -> PhysicalSize<u32> {
        let (width, height) = self
            .dpi_config
            .physical_size_to_logical(self.size.width, self.size.height);
        PhysicalSize::new(width, height)
    }

    /// Get the working area (excluding taskbars, docks, etc.)
    pub fn working_area(&self) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
        // TODO: This would require platform-specific code to get actual working area
        // For now, return the full monitor area
        Some((self.position, self.size))
    }

    /// Check if a point is within this monitor
    pub fn contains_point(&self, point: PhysicalPosition<i32>) -> bool {
        point.x >= self.position.x
            && point.x < self.position.x + self.size.width as i32
            && point.y >= self.position.y
            && point.y < self.position.y + self.size.height as i32
    }

    /// Check if a rectangle intersects with this monitor
    pub fn intersects_rect(
        &self,
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    ) -> bool {
        let rect_right = position.x + size.width as i32;
        let rect_bottom = position.y + size.height as i32;
        let monitor_right = self.position.x + self.size.width as i32;
        let monitor_bottom = self.position.y + self.size.height as i32;

        !(position.x >= monitor_right
            || rect_right <= self.position.x
            || position.y >= monitor_bottom
            || rect_bottom <= self.position.y)
    }

    /// Calculate the best position to center a window on this monitor
    pub fn center_window_position(&self, window_size: PhysicalSize<u32>) -> PhysicalPosition<i32> {
        PhysicalPosition::new(
            self.position.x + (self.size.width as i32 - window_size.width as i32) / 2,
            self.position.y + (self.size.height as i32 - window_size.height as i32) / 2,
        )
    }
}

/// Information about a video mode (resolution/refresh rate)
#[derive(Debug, Clone)]
pub struct VideoModeInfo {
    /// Resolution width in pixels
    pub width: u32,
    /// Resolution height in pixels  
    pub height: u32,
    /// Refresh rate in Hz
    pub refresh_rate_millihertz: u32,
    /// Color depth in bits per pixel
    pub bit_depth: u16,
}

impl VideoModeInfo {
    /// Create from a winit VideoMode
    fn from_video_mode(mode: VideoMode) -> Self {
        Self {
            width: mode.size().width,
            height: mode.size().height,
            refresh_rate_millihertz: mode.refresh_rate_millihertz(),
            bit_depth: mode.bit_depth(),
        }
    }

    /// Get refresh rate in Hz
    pub fn refresh_rate_hz(&self) -> f64 {
        self.refresh_rate_millihertz as f64 / 1000.0
    }
}

impl fmt::Display for VideoModeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} @{:.1}Hz ({}bit)",
            self.width,
            self.height,
            self.refresh_rate_hz(),
            self.bit_depth
        )
    }
}

/// Window placement preferences
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowPlacement {
    /// Place on primary monitor
    Primary,
    /// Place on a specific monitor by index
    Monitor(usize),
    /// Place on monitor containing the mouse cursor
    CursorMonitor,
    /// Place on monitor with most available space
    LargestMonitor,
    /// Remember last position
    RememberLast,
}

impl Default for WindowPlacement {
    fn default() -> Self {
        Self::Primary
    }
}

impl fmt::Display for WindowPlacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => write!(f, "Primary Monitor"),
            Self::Monitor(index) => write!(f, "Monitor {}", index + 1),
            Self::CursorMonitor => write!(f, "Cursor Monitor"),
            Self::LargestMonitor => write!(f, "Largest Monitor"),
            Self::RememberLast => write!(f, "Remember Last Position"),
        }
    }
}

/// Manager for multi-monitor operations
pub struct MonitorManager {
    /// List of available monitors
    monitors: Vec<MonitorInfo>,
    /// Index of primary monitor
    primary_index: Option<usize>,
}

impl MonitorManager {
    /// Create a new monitor manager by detecting available monitors
    pub fn new() -> Self {
        Self {
            monitors: Vec::new(),
            primary_index: None,
        }
    }

    /// Update monitor information from the event loop
    pub fn update_monitors<T>(&mut self, event_loop: &winit::event_loop::EventLoopWindowTarget<T>) {
        let available_monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
        let primary_monitor = event_loop.primary_monitor();

        debug!("Detected {} monitors", available_monitors.len());

        self.monitors.clear();
        self.primary_index = None;

        for (index, monitor) in available_monitors.iter().enumerate() {
            let is_primary = primary_monitor
                .as_ref()
                .map(|primary| monitor.name() == primary.name())
                .unwrap_or(index == 0);

            if is_primary {
                self.primary_index = Some(index);
            }

            let monitor_info = MonitorInfo::from_monitor_handle(monitor, is_primary);
            self.monitors.push(monitor_info);
        }

        if self.primary_index.is_none() && !self.monitors.is_empty() {
            self.primary_index = Some(0);
            if let Some(monitor) = self.monitors.get_mut(0) {
                monitor.is_primary = true;
            }
        }
    }

    /// Get all available monitors
    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    /// Get the primary monitor
    pub fn primary_monitor(&self) -> Option<&MonitorInfo> {
        self.primary_index
            .and_then(|index| self.monitors.get(index))
    }

    /// Get a monitor by index
    pub fn monitor(&self, index: usize) -> Option<&MonitorInfo> {
        self.monitors.get(index)
    }

    /// Get the number of monitors
    pub fn monitor_count(&self) -> usize {
        self.monitors.len()
    }

    /// Find monitor containing a point
    pub fn monitor_at_point(&self, point: PhysicalPosition<i32>) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .find(|monitor| monitor.contains_point(point))
    }

    /// Find monitor with the largest area
    pub fn largest_monitor(&self) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .max_by_key(|monitor| monitor.size.width as u64 * monitor.size.height as u64)
    }

    /// Find the best monitor for window placement based on preference
    pub fn best_monitor_for_placement(
        &self,
        placement: WindowPlacement,
        cursor_position: Option<PhysicalPosition<i32>>,
    ) -> Option<&MonitorInfo> {
        match placement {
            WindowPlacement::Primary => self.primary_monitor(),
            WindowPlacement::Monitor(index) => self.monitor(index),
            WindowPlacement::CursorMonitor => {
                if let Some(cursor_pos) = cursor_position {
                    self.monitor_at_point(cursor_pos)
                        .or_else(|| self.primary_monitor())
                } else {
                    self.primary_monitor()
                }
            }
            WindowPlacement::LargestMonitor => self.largest_monitor(),
            WindowPlacement::RememberLast => {
                // TODO: Would need to load last position from preferences
                self.primary_monitor()
            }
        }
    }

    /// Calculate optimal window size for a monitor
    pub fn optimal_window_size(
        &self,
        monitor: &MonitorInfo,
        preferred_size: PhysicalSize<u32>,
        max_ratio: f64,
    ) -> PhysicalSize<u32> {
        let monitor_size = monitor.size;

        // Don't exceed a fraction of the monitor size
        let max_width = (monitor_size.width as f64 * max_ratio) as u32;
        let max_height = (monitor_size.height as f64 * max_ratio) as u32;

        PhysicalSize::new(
            preferred_size.width.min(max_width),
            preferred_size.height.min(max_height),
        )
    }

    /// Get monitor information summary for logging/debugging
    pub fn summary(&self) -> String {
        let mut summary = format!("MonitorManager: {} monitors", self.monitors.len());

        for (index, monitor) in self.monitors.iter().enumerate() {
            let primary_marker = if monitor.is_primary { " [PRIMARY]" } else { "" };
            summary.push_str(&format!(
                "\n  {}: {} {}x{} @{:.1}x{}",
                index + 1,
                monitor.name.as_deref().unwrap_or("Unknown"),
                monitor.size.width,
                monitor.size.height,
                monitor.scale_factor,
                primary_marker
            ));
        }

        summary
    }
}

impl Default for MonitorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MonitorManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MonitorManager")
            .field("monitor_count", &self.monitors.len())
            .field("primary_index", &self.primary_index)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalSize;

    fn create_test_monitor(
        name: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        scale_factor: f64,
        is_primary: bool,
    ) -> MonitorInfo {
        MonitorInfo {
            name: Some(name.to_string()),
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(width, height),
            scale_factor,
            is_primary,
            dpi_config: DpiConfig::from_scale_factor(scale_factor),
            video_modes: vec![],
        }
    }

    #[test]
    fn test_monitor_info_contains_point() {
        let monitor = create_test_monitor("Test", 0, 0, 1920, 1080, 1.0, true);

        assert!(monitor.contains_point(PhysicalPosition::new(100, 100)));
        assert!(monitor.contains_point(PhysicalPosition::new(0, 0)));
        assert!(monitor.contains_point(PhysicalPosition::new(1919, 1079)));

        assert!(!monitor.contains_point(PhysicalPosition::new(-1, 0)));
        assert!(!monitor.contains_point(PhysicalPosition::new(0, -1)));
        assert!(!monitor.contains_point(PhysicalPosition::new(1920, 1080)));
    }

    #[test]
    fn test_monitor_info_intersects_rect() {
        let monitor = create_test_monitor("Test", 0, 0, 1920, 1080, 1.0, true);

        // Rectangle inside monitor
        assert!(
            monitor.intersects_rect(PhysicalPosition::new(100, 100), PhysicalSize::new(200, 200))
        );

        // Rectangle overlapping monitor
        assert!(monitor.intersects_rect(
            PhysicalPosition::new(-100, -100),
            PhysicalSize::new(200, 200)
        ));

        // Rectangle completely outside
        assert!(!monitor.intersects_rect(
            PhysicalPosition::new(2000, 2000),
            PhysicalSize::new(100, 100)
        ));
    }

    #[test]
    fn test_monitor_info_center_window() {
        let monitor = create_test_monitor("Test", 100, 50, 1920, 1080, 1.0, true);
        let window_size = PhysicalSize::new(800, 600);

        let center_pos = monitor.center_window_position(window_size);

        // Should center the window on the monitor
        let expected_x = 100 + (1920 - 800) / 2;
        let expected_y = 50 + (1080 - 600) / 2;

        assert_eq!(center_pos.x, expected_x);
        assert_eq!(center_pos.y, expected_y);
    }

    #[test]
    fn test_monitor_info_logical_size() {
        let monitor = create_test_monitor("Test", 0, 0, 1920, 1080, 2.0, true);
        let logical_size = monitor.logical_size();

        // At 2x scale, logical size should be half of physical
        assert_eq!(logical_size.width, 960);
        assert_eq!(logical_size.height, 540);
    }

    #[test]
    fn test_video_mode_info_display() {
        let mode = VideoModeInfo {
            width: 1920,
            height: 1080,
            refresh_rate_millihertz: 60000,
            bit_depth: 24,
        };

        let display_str = format!("{}", mode);
        assert!(display_str.contains("1920x1080"));
        assert!(display_str.contains("60.0Hz"));
        assert!(display_str.contains("24bit"));
    }

    #[test]
    fn test_video_mode_refresh_rate() {
        let mode = VideoModeInfo {
            width: 1920,
            height: 1080,
            refresh_rate_millihertz: 75000,
            bit_depth: 24,
        };

        assert_eq!(mode.refresh_rate_hz(), 75.0);
    }

    #[test]
    fn test_window_placement_display() {
        assert_eq!(format!("{}", WindowPlacement::Primary), "Primary Monitor");
        assert_eq!(format!("{}", WindowPlacement::Monitor(1)), "Monitor 2");
        assert_eq!(
            format!("{}", WindowPlacement::CursorMonitor),
            "Cursor Monitor"
        );
        assert_eq!(
            format!("{}", WindowPlacement::LargestMonitor),
            "Largest Monitor"
        );
        assert_eq!(
            format!("{}", WindowPlacement::RememberLast),
            "Remember Last Position"
        );
    }

    #[test]
    fn test_monitor_manager_creation() {
        let manager = MonitorManager::new();

        assert_eq!(manager.monitor_count(), 0);
        assert!(manager.primary_monitor().is_none());
        assert!(manager.monitors().is_empty());
    }

    #[test]
    fn test_monitor_manager_with_mock_monitors() {
        let mut manager = MonitorManager::new();

        // Simulate adding monitors
        manager
            .monitors
            .push(create_test_monitor("Monitor1", 0, 0, 1920, 1080, 1.0, true));
        manager.monitors.push(create_test_monitor(
            "Monitor2", 1920, 0, 1920, 1080, 1.0, false,
        ));
        manager.primary_index = Some(0);

        assert_eq!(manager.monitor_count(), 2);
        assert!(manager.primary_monitor().is_some());
        assert_eq!(
            manager.primary_monitor().unwrap().name.as_deref(),
            Some("Monitor1")
        );

        let monitor1 = manager.monitor(0);
        assert!(monitor1.is_some());
        assert!(monitor1.unwrap().is_primary);

        let monitor2 = manager.monitor(1);
        assert!(monitor2.is_some());
        assert!(!monitor2.unwrap().is_primary);
    }

    #[test]
    fn test_monitor_at_point() {
        let mut manager = MonitorManager::new();
        manager
            .monitors
            .push(create_test_monitor("Left", 0, 0, 1920, 1080, 1.0, true));
        manager.monitors.push(create_test_monitor(
            "Right", 1920, 0, 1920, 1080, 1.0, false,
        ));

        let left_point = PhysicalPosition::new(100, 100);
        let right_point = PhysicalPosition::new(2000, 100);
        let outside_point = PhysicalPosition::new(4000, 100);

        assert_eq!(
            manager
                .monitor_at_point(left_point)
                .unwrap()
                .name
                .as_deref(),
            Some("Left")
        );
        assert_eq!(
            manager
                .monitor_at_point(right_point)
                .unwrap()
                .name
                .as_deref(),
            Some("Right")
        );
        assert!(manager.monitor_at_point(outside_point).is_none());
    }

    #[test]
    fn test_largest_monitor() {
        let mut manager = MonitorManager::new();
        manager
            .monitors
            .push(create_test_monitor("Small", 0, 0, 1280, 720, 1.0, true));
        manager.monitors.push(create_test_monitor(
            "Large", 1280, 0, 2560, 1440, 1.0, false,
        ));

        let largest = manager.largest_monitor();
        assert!(largest.is_some());
        assert_eq!(largest.unwrap().name.as_deref(), Some("Large"));
    }

    #[test]
    fn test_optimal_window_size() {
        let manager = MonitorManager::new();
        let monitor = create_test_monitor("Test", 0, 0, 1920, 1080, 1.0, true);

        // Window smaller than max ratio - should remain unchanged
        let small_size = PhysicalSize::new(800, 600);
        let optimal = manager.optimal_window_size(&monitor, small_size, 0.8);
        assert_eq!(optimal, small_size);

        // Window larger than max ratio - should be constrained
        let large_size = PhysicalSize::new(2000, 1200);
        let optimal = manager.optimal_window_size(&monitor, large_size, 0.8);
        assert!(optimal.width <= (1920.0 * 0.8) as u32);
        assert!(optimal.height <= (1080.0 * 0.8) as u32);
    }

    #[test]
    fn test_monitor_manager_summary() {
        let mut manager = MonitorManager::new();
        manager
            .monitors
            .push(create_test_monitor("Monitor1", 0, 0, 1920, 1080, 1.0, true));
        manager.monitors.push(create_test_monitor(
            "Monitor2", 1920, 0, 1920, 1080, 2.0, false,
        ));

        let summary = manager.summary();
        assert!(summary.contains("2 monitors"));
        assert!(summary.contains("Monitor1"));
        assert!(summary.contains("Monitor2"));
        assert!(summary.contains("[PRIMARY]"));
        assert!(summary.contains("1920x1080"));
        assert!(summary.contains("@2.0x"));
    }
}
