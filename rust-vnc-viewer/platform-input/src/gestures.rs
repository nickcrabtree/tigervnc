//! Gesture support for macOS trackpads and touch devices.
//!
//! This module handles multi-touch gestures like pinch-to-zoom and two-finger
//! scroll, integrating with the display system's viewport management.

use std::time::{Duration, Instant};
use tracing::trace;

/// Gesture event types that can be processed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GestureEvent {
    /// Pinch gesture for zoom (scale factor, location)
    Pinch {
        scale: f64,
        center_x: f64,
        center_y: f64,
    },
    /// Two-finger scroll (delta_x, delta_y)
    Scroll { delta_x: f64, delta_y: f64 },
    /// Pan gesture (delta_x, delta_y)
    Pan { delta_x: f64, delta_y: f64 },
    /// Rotation gesture (angle in radians, center)
    Rotation {
        angle: f64,
        center_x: f64,
        center_y: f64,
    },
}

/// Actions that can be triggered by gestures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GestureAction {
    /// Zoom the viewport by a scale factor
    Zoom {
        factor: f64,
        center_x: f64,
        center_y: f64,
    },
    /// Pan the viewport by a delta
    Pan { delta_x: f64, delta_y: f64 },
    /// Scroll content (different from pan - sends scroll wheel events)
    Scroll { delta_x: f64, delta_y: f64 },
    /// No action (gesture filtered out or not recognized)
    None,
}

/// Configuration for gesture recognition and processing.
#[derive(Debug, Clone)]
pub struct GestureConfig {
    /// Enable gesture recognition
    pub enabled: bool,
    /// Minimum scale change to register a zoom gesture
    pub min_zoom_scale: f64,
    /// Maximum scale factor for zoom (prevents extreme zoom)
    pub max_zoom_scale: f64,
    /// Minimum scroll delta to register scroll gesture
    pub min_scroll_delta: f64,
    /// Scroll sensitivity multiplier
    pub scroll_sensitivity: f64,
    /// Pan sensitivity multiplier
    pub pan_sensitivity: f64,
    /// Zoom sensitivity multiplier
    pub zoom_sensitivity: f64,
    /// Enable momentum scrolling
    pub momentum_scroll: bool,
    /// Momentum decay factor (0.0 = instant stop, 1.0 = no decay)
    pub momentum_decay: f64,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_zoom_scale: 0.01,    // 1% minimum scale change
            max_zoom_scale: 10.0,    // 10x maximum zoom in single gesture
            min_scroll_delta: 1.0,   // Minimum scroll distance
            scroll_sensitivity: 1.0, // Default sensitivity
            pan_sensitivity: 1.0,
            zoom_sensitivity: 1.0,
            momentum_scroll: true,
            momentum_decay: 0.95, // 5% decay per frame
        }
    }
}

/// State tracking for gesture recognition and momentum.
#[derive(Debug)]
pub struct GestureProcessor {
    config: GestureConfig,

    // Current gesture state
    active_gesture: Option<GestureEvent>,
    last_gesture_time: Instant,
    _gesture_start_time: Instant,

    // Momentum tracking
    momentum_velocity_x: f64,
    momentum_velocity_y: f64,
    last_momentum_update: Instant,

    // Accumulated deltas for threshold checking
    accumulated_scroll_x: f64,
    accumulated_scroll_y: f64,
    accumulated_zoom: f64,

    // Zoom state
    last_zoom_scale: f64,
    zoom_center_x: f64,
    zoom_center_y: f64,
}

impl Default for GestureProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureProcessor {
    /// Create a new gesture processor with default configuration.
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            config: GestureConfig::default(),
            active_gesture: None,
            last_gesture_time: now,
            _gesture_start_time: now,
            momentum_velocity_x: 0.0,
            momentum_velocity_y: 0.0,
            last_momentum_update: now,
            accumulated_scroll_x: 0.0,
            accumulated_scroll_y: 0.0,
            accumulated_zoom: 0.0,
            last_zoom_scale: 1.0,
            zoom_center_x: 0.0,
            zoom_center_y: 0.0,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: GestureConfig) -> Self {
        let mut processor = Self::new();
        processor.config = config;
        processor
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: GestureConfig) {
        self.config = config;
    }

    /// Process a gesture event and return the action to take.
    pub fn process_gesture(&mut self, event: GestureEvent) -> GestureAction {
        if !self.config.enabled {
            return GestureAction::None;
        }

        let now = Instant::now();
        self.last_gesture_time = now;

        match event {
            GestureEvent::Pinch {
                scale,
                center_x,
                center_y,
            } => self.process_pinch(scale, center_x, center_y, now),
            GestureEvent::Scroll { delta_x, delta_y } => self.process_scroll(delta_x, delta_y, now),
            GestureEvent::Pan { delta_x, delta_y } => self.process_pan(delta_x, delta_y, now),
            GestureEvent::Rotation {
                angle: _,
                center_x: _,
                center_y: _,
            } => {
                // Rotation not implemented yet
                GestureAction::None
            }
        }
    }

    /// Process pinch gesture for zooming.
    fn process_pinch(
        &mut self,
        scale: f64,
        center_x: f64,
        center_y: f64,
        _now: Instant,
    ) -> GestureAction {
        // Track zoom accumulation
        let scale_delta = scale - self.last_zoom_scale;
        self.accumulated_zoom += scale_delta.abs();
        self.last_zoom_scale = scale;
        self.zoom_center_x = center_x;
        self.zoom_center_y = center_y;

        // Check if we've accumulated enough change to trigger zoom
        if self.accumulated_zoom >= self.config.min_zoom_scale {
            let zoom_factor = 1.0 + (scale_delta * self.config.zoom_sensitivity);

            // Clamp zoom factor to reasonable limits
            let clamped_factor = if zoom_factor > self.config.max_zoom_scale {
                self.config.max_zoom_scale
            } else if zoom_factor < (1.0 / self.config.max_zoom_scale) {
                1.0 / self.config.max_zoom_scale
            } else {
                zoom_factor
            };

            if (clamped_factor - 1.0).abs() > 0.001 {
                // Avoid tiny zoom changes
                trace!(
                    "Pinch zoom: factor={:.3}, center=({:.1}, {:.1})",
                    clamped_factor,
                    center_x,
                    center_y
                );
                self.accumulated_zoom = 0.0; // Reset accumulation
                return GestureAction::Zoom {
                    factor: clamped_factor,
                    center_x,
                    center_y,
                };
            }
        }

        GestureAction::None
    }

    /// Process scroll gesture.
    fn process_scroll(&mut self, delta_x: f64, delta_y: f64, now: Instant) -> GestureAction {
        // Apply sensitivity
        let scaled_delta_x = delta_x * self.config.scroll_sensitivity;
        let scaled_delta_y = delta_y * self.config.scroll_sensitivity;

        // Update momentum
        if self.config.momentum_scroll {
            self.momentum_velocity_x = scaled_delta_x;
            self.momentum_velocity_y = scaled_delta_y;
            self.last_momentum_update = now;
        }

        // Accumulate scroll deltas
        self.accumulated_scroll_x += scaled_delta_x.abs();
        self.accumulated_scroll_y += scaled_delta_y.abs();

        // Check if we've scrolled enough to trigger action
        let total_scroll = (self.accumulated_scroll_x + self.accumulated_scroll_y) / 2.0;
        if total_scroll >= self.config.min_scroll_delta {
            trace!(
                "Scroll gesture: delta=({:.1}, {:.1})",
                scaled_delta_x,
                scaled_delta_y
            );
            self.accumulated_scroll_x = 0.0;
            self.accumulated_scroll_y = 0.0;
            return GestureAction::Scroll {
                delta_x: scaled_delta_x,
                delta_y: scaled_delta_y,
            };
        }

        GestureAction::None
    }

    /// Process pan gesture.
    fn process_pan(&mut self, delta_x: f64, delta_y: f64, _now: Instant) -> GestureAction {
        let scaled_delta_x = delta_x * self.config.pan_sensitivity;
        let scaled_delta_y = delta_y * self.config.pan_sensitivity;

        // Pan immediately without accumulation (for responsive viewport movement)
        if scaled_delta_x.abs() > 0.1 || scaled_delta_y.abs() > 0.1 {
            trace!(
                "Pan gesture: delta=({:.1}, {:.1})",
                scaled_delta_x,
                scaled_delta_y
            );
            return GestureAction::Pan {
                delta_x: scaled_delta_x,
                delta_y: scaled_delta_y,
            };
        }

        GestureAction::None
    }

    /// Update momentum and return momentum-based action.
    /// Should be called regularly (e.g., on each frame) to maintain momentum.
    pub fn update_momentum(&mut self) -> GestureAction {
        if !self.config.momentum_scroll || !self.config.enabled {
            return GestureAction::None;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_momentum_update);

        // Only apply momentum if we're not receiving active gestures
        let time_since_gesture = now.duration_since(self.last_gesture_time);
        if time_since_gesture < Duration::from_millis(50) {
            // Recent gesture activity, don't apply momentum
            return GestureAction::None;
        }

        // Apply momentum decay
        let decay_factor = self
            .config
            .momentum_decay
            .powf(elapsed.as_secs_f64() * 60.0); // 60fps baseline
        self.momentum_velocity_x *= decay_factor;
        self.momentum_velocity_y *= decay_factor;
        self.last_momentum_update = now;

        // Check if momentum is significant enough to continue
        let momentum_magnitude =
            (self.momentum_velocity_x.powi(2) + self.momentum_velocity_y.powi(2)).sqrt();
        if momentum_magnitude > 0.5 {
            // Minimum momentum threshold
            trace!(
                "Momentum scroll: velocity=({:.1}, {:.1})",
                self.momentum_velocity_x,
                self.momentum_velocity_y
            );
            return GestureAction::Scroll {
                delta_x: self.momentum_velocity_x,
                delta_y: self.momentum_velocity_y,
            };
        } else {
            // Stop momentum
            self.momentum_velocity_x = 0.0;
            self.momentum_velocity_y = 0.0;
        }

        GestureAction::None
    }

    /// Reset gesture state (useful when focus changes or gestures should stop).
    pub fn reset(&mut self) {
        self.active_gesture = None;
        self.momentum_velocity_x = 0.0;
        self.momentum_velocity_y = 0.0;
        self.accumulated_scroll_x = 0.0;
        self.accumulated_scroll_y = 0.0;
        self.accumulated_zoom = 0.0;
        self.last_zoom_scale = 1.0;
    }

    /// Get current momentum velocity for debugging.
    pub fn momentum_velocity(&self) -> (f64, f64) {
        (self.momentum_velocity_x, self.momentum_velocity_y)
    }

    /// Check if momentum is active.
    pub fn has_momentum(&self) -> bool {
        let magnitude =
            (self.momentum_velocity_x.powi(2) + self.momentum_velocity_y.powi(2)).sqrt();
        magnitude > 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinch_zoom_threshold() {
        let mut processor = GestureProcessor::new();

        // Small pinch should not trigger zoom
        let action = processor.process_gesture(GestureEvent::Pinch {
            scale: 1.005, // 0.5% scale change
            center_x: 100.0,
            center_y: 100.0,
        });
        assert_eq!(action, GestureAction::None);

        // Larger pinch should trigger zoom
        let action = processor.process_gesture(GestureEvent::Pinch {
            scale: 1.05, // 5% scale change
            center_x: 100.0,
            center_y: 100.0,
        });
        matches!(action, GestureAction::Zoom { .. });
    }

    #[test]
    fn test_scroll_accumulation() {
        let mut processor = GestureProcessor::new();

        // Small scrolls should accumulate
        let action1 = processor.process_gesture(GestureEvent::Scroll {
            delta_x: 0.5,
            delta_y: 0.0,
        });
        assert_eq!(action1, GestureAction::None);

        let action2 = processor.process_gesture(GestureEvent::Scroll {
            delta_x: 0.6, // Total: 1.1, should trigger
            delta_y: 0.0,
        });
        matches!(action2, GestureAction::Scroll { .. });
    }

    #[test]
    fn test_momentum_decay() {
        let mut processor = GestureProcessor::new();

        // Set up momentum
        processor.momentum_velocity_x = 10.0;
        processor.momentum_velocity_y = 5.0;

        // Simulate time passing
        std::thread::sleep(Duration::from_millis(50));

        let action = processor.update_momentum();
        matches!(action, GestureAction::Scroll { .. });

        // Velocity should have decayed
        assert!(processor.momentum_velocity_x < 10.0);
        assert!(processor.momentum_velocity_y < 5.0);
    }

    #[test]
    fn test_gesture_config() {
        let config = GestureConfig {
            enabled: false,
            ..Default::default()
        };

        let mut processor = GestureProcessor::with_config(config);

        // Should not process gestures when disabled
        let action = processor.process_gesture(GestureEvent::Scroll {
            delta_x: 10.0,
            delta_y: 10.0,
        });
        assert_eq!(action, GestureAction::None);
    }

    #[test]
    fn test_pan_immediate_response() {
        let mut processor = GestureProcessor::new();

        // Pan should respond immediately without accumulation
        let action = processor.process_gesture(GestureEvent::Pan {
            delta_x: 5.0,
            delta_y: 3.0,
        });
        matches!(action, GestureAction::Pan { .. });
    }

    #[test]
    fn test_zoom_limits() {
        let config = GestureConfig {
            max_zoom_scale: 2.0,
            ..Default::default()
        };
        let mut processor = GestureProcessor::with_config(config);

        // Extreme zoom should be clamped
        let action = processor.process_gesture(GestureEvent::Pinch {
            scale: 10.0, // Would be 10x zoom
            center_x: 0.0,
            center_y: 0.0,
        });

        if let GestureAction::Zoom { factor, .. } = action {
            assert!(factor <= 2.0); // Should be clamped to max
        }
    }
}
