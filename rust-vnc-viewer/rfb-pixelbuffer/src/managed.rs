//! Managed pixel buffer implementation.
//!
//! This module provides [`ManagedPixelBuffer`], a concrete implementation of the
//! [`PixelBuffer`] and [`MutablePixelBuffer`] traits that owns its pixel data in a Vec.
//!
//! # Example
//!
//! ```
//! use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat, MutablePixelBuffer, PixelBuffer};
//! use rfb_common::Rect;
//!
//! // Create a 100x100 buffer with RGB888 format
//! let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
//!
//! // Fill a rectangle with red
//! let format = buffer.pixel_format().clone();
//! let red = format.from_rgb888([255, 0, 0, 255]);
//! let rect = Rect::new(10, 10, 50, 50);
//! buffer.fill_rect(rect, &red).unwrap();
//!
//! // Access the dimensions
//! let (width, height) = buffer.dimensions();
//! assert_eq!((width, height), (100, 100));
//! ```

use crate::{MutablePixelBuffer, PixelBuffer, PixelFormat};
use anyhow::{anyhow, Result};
use rfb_common::{Point, Rect};

/// A pixel buffer that manages its own memory.
///
/// This is the primary concrete implementation of the pixel buffer traits.
/// It stores pixel data in a contiguous `Vec<u8>` and provides efficient
/// implementations of all rendering operations.
///
/// # Memory Layout
///
/// The buffer is stored in row-major order with a stride equal to the width.
/// For a buffer of width W, height H, and bytes-per-pixel B:
///
/// ```text
/// Total size = W * H * B bytes
/// Pixel at (x, y) starts at offset: (y * W + x) * B
/// ```
///
/// # Stride Convention
///
/// The stride is always measured in **pixels** (not bytes) and equals the width.
/// This matches the critical convention documented in TigerVNC's WARP.md.
///
/// # Example
///
/// ```
/// use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat, PixelBuffer};
///
/// let format = PixelFormat::rgb888();
/// let buffer = ManagedPixelBuffer::new(1920, 1080, format);
///
/// assert_eq!(buffer.dimensions(), (1920, 1080));
/// assert_eq!(buffer.stride(), 1920); // Stride in pixels
/// ```
#[derive(Debug, Clone)]
pub struct ManagedPixelBuffer {
    /// Buffer width in pixels
    width: u32,

    /// Buffer height in pixels
    height: u32,

    /// Pixel format describing how pixels are encoded
    format: PixelFormat,

    /// Raw pixel data (row-major, no padding)
    data: Vec<u8>,

    /// Stride in **pixels** (always equals width for this implementation)
    stride: usize,
}

impl ManagedPixelBuffer {
    /// Creates a new pixel buffer with the specified dimensions and format.
    ///
    /// The buffer is initialized with all zeros (black for most formats).
    ///
    /// # Parameters
    ///
    /// - `width`: Buffer width in pixels
    /// - `height`: Buffer height in pixels
    /// - `format`: Pixel format describing the encoding
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat, PixelBuffer};
    ///
    /// let buffer = ManagedPixelBuffer::new(800, 600, PixelFormat::rgb888());
    /// assert_eq!(buffer.dimensions(), (800, 600));
    /// ```
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let stride = width as usize;
        let bytes_per_pixel = format.bytes_per_pixel() as usize;
        let data = vec![0u8; stride * height as usize * bytes_per_pixel];

        Self {
            width,
            height,
            format,
            data,
            stride,
        }
    }

    /// Resizes the buffer to new dimensions.
    ///
    /// This reallocates the internal buffer. Existing pixel data is not preserved.
    /// The new buffer is initialized with all zeros.
    ///
    /// # Parameters
    ///
    /// - `width`: New width in pixels
    /// - `height`: New height in pixels
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat, PixelBuffer};
    ///
    /// let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
    /// buffer.resize(200, 150);
    /// assert_eq!(buffer.dimensions(), (200, 150));
    /// ```
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.stride = width as usize;
        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        self.data
            .resize(self.stride * height as usize * bytes_per_pixel, 0);
    }

    /// Returns the stride in pixels.
    ///
    /// For `ManagedPixelBuffer`, the stride always equals the width.
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat};
    ///
    /// let buffer = ManagedPixelBuffer::new(800, 600, PixelFormat::rgb888());
    /// assert_eq!(buffer.stride(), 800);
    /// ```
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Returns a reference to the raw pixel data.
    ///
    /// The data is in row-major order with no padding between rows.
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::{ManagedPixelBuffer, PixelFormat};
    ///
    /// let buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
    /// let data = buffer.data();
    /// assert_eq!(data.len(), 100 * 100 * 4); // 100x100 pixels * 4 bytes/pixel
    /// ```
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns the buffer width in pixels.
    pub fn width(&self) -> usize {
        self.width as usize
    }

    /// Returns the buffer height in pixels.
    pub fn height(&self) -> usize {
        self.height as usize
    }

    /// Returns a reference to the pixel format.
    pub fn format(&self) -> &PixelFormat {
        &self.format
    }

    /// Validates that a rectangle is within buffer bounds.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the rectangle is valid
    /// - `Err` if the rectangle is out of bounds
    fn validate_rect(&self, rect: Rect) -> Result<()> {
        if rect.x as u32 + rect.width > self.width || rect.y as u32 + rect.height > self.height {
            return Err(anyhow!(
                "Rectangle out of bounds: {:?} (buffer size: {}x{})",
                rect,
                self.width,
                self.height
            ));
        }
        Ok(())
    }
}

impl PixelBuffer for ManagedPixelBuffer {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn pixel_format(&self) -> &PixelFormat {
        &self.format
    }

    fn get_buffer(&self, rect: Rect, stride: &mut usize) -> Option<&[u8]> {
        if self.validate_rect(rect).is_err() {
            return None;
        }

        *stride = self.stride;
        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        let start = (rect.y as usize * self.stride + rect.x as usize) * bytes_per_pixel;
        let len = rect.height as usize * self.stride * bytes_per_pixel;

        Some(&self.data[start..start + len])
    }
}

impl MutablePixelBuffer for ManagedPixelBuffer {
    fn get_buffer_rw(&mut self, rect: Rect, stride: &mut usize) -> Option<&mut [u8]> {
        if self.validate_rect(rect).is_err() {
            return None;
        }

        *stride = self.stride;
        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        let start = (rect.y as usize * self.stride + rect.x as usize) * bytes_per_pixel;
        let len = rect.height as usize * self.stride * bytes_per_pixel;

        Some(&mut self.data[start..start + len])
    }

    fn commit_buffer(&mut self, _rect: Rect) {
        // No-op for ManagedPixelBuffer as we directly modify the underlying data
    }

    fn fill_rect(&mut self, rect: Rect, pixel: &[u8]) -> Result<()> {
        self.validate_rect(rect)?;

        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        if pixel.len() != bytes_per_pixel {
            return Err(anyhow!(
                "Invalid pixel size: got {} bytes, expected {}",
                pixel.len(),
                bytes_per_pixel
            ));
        }

        for y in 0..rect.height as usize {
            let row_offset =
                ((rect.y as usize + y) * self.stride + rect.x as usize) * bytes_per_pixel;

            for x in 0..rect.width as usize {
                let offset = row_offset + x * bytes_per_pixel;
                self.data[offset..offset + bytes_per_pixel].copy_from_slice(pixel);
            }
        }

        Ok(())
    }

    fn copy_rect(&mut self, dest: Rect, src_offset: Point) -> Result<()> {
        self.validate_rect(dest)?;

        // Calculate source rectangle
        // src_offset is the offset FROM destination TO source
        // So source = dest + src_offset
        let src_x = dest.x + src_offset.x;
        let src_y = dest.y + src_offset.y;
        let src_rect = Rect::new(src_x, src_y, dest.width, dest.height);
        self.validate_rect(src_rect)?;

        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        let rect_width_bytes = dest.width as usize * bytes_per_pixel;

        // Handle overlapping regions by choosing copy direction
        // If source is above/left of dest (negative offset), copy from bottom/right to top/left
        // to avoid overwriting source data before it's copied
        if src_offset.y < 0 || (src_offset.y == 0 && src_offset.x < 0) {
            // Copy from bottom to top (reverse)
            for y in (0..dest.height as usize).rev() {
                let src_offset_calc = ((src_rect.y as usize + y) * self.stride
                    + src_rect.x as usize)
                    * bytes_per_pixel;
                let dst_offset_calc =
                    ((dest.y as usize + y) * self.stride + dest.x as usize) * bytes_per_pixel;

                self.data.copy_within(
                    src_offset_calc..src_offset_calc + rect_width_bytes,
                    dst_offset_calc,
                );
            }
        } else {
            // Copy from top to bottom (forward)
            for y in 0..dest.height as usize {
                let src_offset_calc = ((src_rect.y as usize + y) * self.stride
                    + src_rect.x as usize)
                    * bytes_per_pixel;
                let dst_offset_calc =
                    ((dest.y as usize + y) * self.stride + dest.x as usize) * bytes_per_pixel;

                self.data.copy_within(
                    src_offset_calc..src_offset_calc + rect_width_bytes,
                    dst_offset_calc,
                );
            }
        }

        Ok(())
    }

    fn image_rect(&mut self, dest: Rect, pixels: &[u8], stride: usize) -> Result<()> {
        self.validate_rect(dest)?;

        let bytes_per_pixel = self.format.bytes_per_pixel() as usize;
        let rect_width_bytes = dest.width as usize * bytes_per_pixel;

        // If stride is 0, source is tightly packed
        let actual_src_stride = if stride == 0 {
            dest.width as usize
        } else {
            stride
        };
        let actual_src_stride_bytes = actual_src_stride * bytes_per_pixel;

        // Validate source data size
        let required_src_bytes =
            actual_src_stride_bytes * (dest.height as usize - 1) + rect_width_bytes;
        if pixels.len() < required_src_bytes {
            return Err(anyhow!(
                "Insufficient source data: got {} bytes, need at least {}",
                pixels.len(),
                required_src_bytes
            ));
        }

        for y in 0..dest.height as usize {
            let dst_offset =
                ((dest.y as usize + y) * self.stride + dest.x as usize) * bytes_per_pixel;
            let src_offset = y * actual_src_stride_bytes;

            self.data[dst_offset..dst_offset + rect_width_bytes]
                .copy_from_slice(&pixels[src_offset..src_offset + rect_width_bytes]);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_buffer() {
        let buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        assert_eq!(buffer.dimensions(), (100, 100));
        assert_eq!(buffer.stride(), 100);
        assert_eq!(buffer.data().len(), 100 * 100 * 4);
    }

    #[test]
    fn test_resize() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        buffer.resize(200, 150);
        assert_eq!(buffer.dimensions(), (200, 150));
        assert_eq!(buffer.stride(), 200);
        assert_eq!(buffer.data().len(), 200 * 150 * 4);
    }

    #[test]
    fn test_fill_rect() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let format = buffer.pixel_format().clone();
        let red = format.from_rgb888([255, 0, 0, 255]);

        let rect = Rect::new(10, 10, 20, 20);
        buffer.fill_rect(rect, &red).unwrap();

        // Verify a pixel in the filled region
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(Rect::new(15, 15, 1, 1), &mut stride) {
            let pixel = &pixels[0..4];
            assert_eq!(pixel, &red[..]);
        } else {
            panic!("Failed to get buffer");
        }
    }

    #[test]
    fn test_copy_rect_non_overlapping() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let format = buffer.pixel_format().clone();

        // Fill source region with red
        let red = format.from_rgb888([255, 0, 0, 255]);
        buffer.fill_rect(Rect::new(10, 10, 20, 20), &red).unwrap();

        // Copy to non-overlapping destination
        // Source at (10, 10), destination at (50, 50)
        // src_offset = source - dest = (10-50, 10-50) = (-40, -40)
        let dest = Rect::new(50, 50, 20, 20);
        let src_offset = Point::new(-40, -40);
        buffer.copy_rect(dest, src_offset).unwrap();

        // Verify destination has red pixels
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(Rect::new(55, 55, 1, 1), &mut stride) {
            let pixel = &pixels[0..4];
            assert_eq!(pixel, &red[..]);
        } else {
            panic!("Failed to get buffer");
        }
    }

    #[test]
    fn test_copy_rect_overlapping_down() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let format = buffer.pixel_format().clone();

        // Fill source region
        let blue = format.from_rgb888([0, 0, 255, 255]);
        buffer.fill_rect(Rect::new(20, 20, 30, 30), &blue).unwrap();

        // Copy down (overlapping)
        // Source at (20, 20), destination at (20, 30)
        // src_offset = source - dest = (20-20, 20-30) = (0, -10)
        let dest = Rect::new(20, 30, 30, 30);
        let src_offset = Point::new(0, -10);
        buffer.copy_rect(dest, src_offset).unwrap();

        // Verify the copy worked
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(Rect::new(25, 35, 1, 1), &mut stride) {
            let pixel = &pixels[0..4];
            assert_eq!(pixel, &blue[..]);
        } else {
            panic!("Failed to get buffer");
        }
    }

    #[test]
    fn test_image_rect_tightly_packed() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let format = buffer.pixel_format().clone();

        // Create 10x10 green image (tightly packed)
        let green = format.from_rgb888([0, 255, 0, 255]);
        let mut image_data = Vec::new();
        for _ in 0..100 {
            image_data.extend_from_slice(&green);
        }

        // Copy to buffer
        let dest = Rect::new(30, 30, 10, 10);
        buffer.image_rect(dest, &image_data, 0).unwrap(); // stride=0 means tightly packed

        // Verify
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(Rect::new(35, 35, 1, 1), &mut stride) {
            let pixel = &pixels[0..4];
            assert_eq!(pixel, &green[..]);
        } else {
            panic!("Failed to get buffer");
        }
    }

    #[test]
    fn test_image_rect_with_stride() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let format = buffer.pixel_format().clone();

        // Create 10x10 image with stride of 20 pixels
        let yellow = format.from_rgb888([255, 255, 0, 255]);
        let mut image_data = Vec::new();
        for _ in 0..10 {
            // 10 rows
            for _ in 0..10 {
                // 10 pixels of data
                image_data.extend_from_slice(&yellow);
            }
            for _ in 0..10 {
                // 10 pixels of padding
                image_data.extend_from_slice(&[0, 0, 0, 0]);
            }
        }

        // Copy to buffer with stride=20
        let dest = Rect::new(40, 40, 10, 10);
        buffer.image_rect(dest, &image_data, 20).unwrap();

        // Verify
        let mut stride = 0;
        if let Some(pixels) = buffer.get_buffer(Rect::new(45, 45, 1, 1), &mut stride) {
            let pixel = &pixels[0..4];
            assert_eq!(pixel, &yellow[..]);
        } else {
            panic!("Failed to get buffer");
        }
    }

    #[test]
    fn test_validate_rect_out_of_bounds() {
        let buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());

        // Too wide
        let rect = Rect::new(90, 50, 20, 10);
        assert!(buffer.validate_rect(rect).is_err());

        // Too tall
        let rect = Rect::new(50, 90, 10, 20);
        assert!(buffer.validate_rect(rect).is_err());

        // Valid rectangle
        let rect = Rect::new(50, 50, 40, 40);
        assert!(buffer.validate_rect(rect).is_ok());
    }

    #[test]
    fn test_get_buffer() {
        let buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let rect = Rect::new(10, 10, 50, 50);
        let mut stride = 0;

        let slice = buffer.get_buffer(rect, &mut stride);
        assert!(slice.is_some());
        assert_eq!(stride, 100); // Stride equals width
    }

    #[test]
    fn test_get_buffer_rw() {
        let mut buffer = ManagedPixelBuffer::new(100, 100, PixelFormat::rgb888());
        let rect = Rect::new(10, 10, 50, 50);
        let mut stride = 0;

        let slice = buffer.get_buffer_rw(rect, &mut stride);
        assert!(slice.is_some());
        assert_eq!(stride, 100);

        // Commit should not panic
        buffer.commit_buffer(rect);
    }
}
