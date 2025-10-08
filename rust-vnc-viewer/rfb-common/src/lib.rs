//! Common types and utilities for RFB protocol implementation.
//!
//! This crate provides shared types used across the VNC viewer implementation:
//! - [`Point`] - 2D point with i32 coordinates
//! - [`Rect`] - Rectangle with position and dimensions

/// A 2D point with integer coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    /// Create a new point.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A rectangle defined by top-left position and dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Create a new rectangle.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Get the right edge (x + width).
    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// Get the bottom edge (y + height).
    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// Check if a point is contained within this rectangle.
    pub const fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Get the area of the rectangle.
    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point() {
        let p = Point::new(10, 20);
        assert_eq!(p.x, 10);
        assert_eq!(p.y, 20);
    }

    #[test]
    fn test_rect() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.x, 10);
        assert_eq!(r.y, 20);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 50);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
        assert_eq!(r.area(), 5000);
    }

    #[test]
    fn test_contains_point() {
        let r = Rect::new(10, 20, 100, 50);
        assert!(r.contains_point(10, 20)); // top-left corner
        assert!(r.contains_point(109, 69)); // bottom-right minus 1
        assert!(!r.contains_point(9, 20)); // left of rect
        assert!(!r.contains_point(10, 19)); // above rect
        assert!(!r.contains_point(110, 69)); // right edge (exclusive)
        assert!(!r.contains_point(109, 70)); // bottom edge (exclusive)
    }
}
