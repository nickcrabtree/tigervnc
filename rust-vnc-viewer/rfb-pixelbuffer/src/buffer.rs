//! Pixel buffer traits for RFB/VNC protocol.
//!
//! This module defines traits for accessing and manipulating pixel buffers in the
//! RFB protocol. There are two main traits:
//!
//! - [`PixelBuffer`]: Read-only access to pixel data
//! - [`MutablePixelBuffer`]: Read-write access with rendering operations
//!
//! # Critical: Stride is in Pixels, Not Bytes!
//!
//! **IMPORTANT**: All stride values in this API are measured in **pixels**, not bytes.
//! This is a critical convention from TigerVNC's WARP.md that prevents bugs.
//!
//! To calculate byte offsets:
//! ```text
//! byte_offset = (y * stride + x) * bytes_per_pixel
//! byte_length = height * stride * bytes_per_pixel
//! ```
//!
//! **Wrong** (caused critical bugs in original C++ code):
//! ```text
//! byte_length = height * stride  // Missing bytes_per_pixel!
//! ```
//!
//! # Buffer Access Patterns
//!
//! ## Read-Only Access
//!
//! Use [`PixelBuffer::get_buffer()`] for read-only access:
//!
//! ```no_run
//! # use rfb_pixelbuffer::{PixelBuffer, PixelFormat};
//! # use rfb_common::Rect;
//! # struct MyBuffer;
//! # impl PixelBuffer for MyBuffer {
//! #     fn dimensions(&self) -> (u32, u32) { (100, 100) }
//! #     fn pixel_format(&self) -> &PixelFormat { todo!() }
//! #     fn get_buffer(&self, rect: Rect, stride: &mut usize) -> Option<&[u8]> { todo!() }
//! # }
//! # let buffer = MyBuffer;
//! let rect = Rect::new(10, 10, 50, 50);
//! let mut stride = 0;
//!
//! if let Some(pixels) = buffer.get_buffer(rect, &mut stride) {
//!     let bpp = buffer.pixel_format().bytes_per_pixel() as usize;
//!     
//!     // Access pixel at (x, y) within the rectangle
//!     for y in 0..rect.height {
//!         let row_offset = (y as usize * stride) * bpp;
//!         for x in 0..rect.width {
//!             let pixel_offset = row_offset + (x as usize * bpp);
//!             let pixel = &pixels[pixel_offset..pixel_offset + bpp];
//!             // Process pixel...
//!         }
//!     }
//! }
//! ```
//!
//! ## Read-Write Access
//!
//! Use [`MutablePixelBuffer::get_buffer_rw()`] and [`MutablePixelBuffer::commit_buffer()`]:
//!
//! ```no_run
//! # use rfb_pixelbuffer::{MutablePixelBuffer, PixelBuffer, PixelFormat};
//! # use rfb_common::Rect;
//! # struct MyBuffer;
//! # impl PixelBuffer for MyBuffer {
//! #     fn dimensions(&self) -> (u32, u32) { (100, 100) }
//! #     fn pixel_format(&self) -> &PixelFormat { todo!() }
//! #     fn get_buffer(&self, rect: Rect, stride: &mut usize) -> Option<&[u8]> { todo!() }
//! # }
//! # impl MutablePixelBuffer for MyBuffer {
//! #     fn get_buffer_rw(&mut self, rect: Rect, stride: &mut usize) -> Option<&mut [u8]> { todo!() }
//! #     fn commit_buffer(&mut self, rect: Rect) {}
//! #     fn fill_rect(&mut self, rect: Rect, pixel: &[u8]) -> anyhow::Result<()> { todo!() }
//! #     fn copy_rect(&mut self, dest: Rect, src_offset: rfb_common::Point) -> anyhow::Result<()> { todo!() }
//! #     fn image_rect(&mut self, dest: Rect, pixels: &[u8], stride: usize) -> anyhow::Result<()> { todo!() }
//! # }
//! # let mut buffer = MyBuffer;
//! let rect = Rect::new(10, 10, 50, 50);
//! let mut stride = 0;
//!
//! if let Some(pixels) = buffer.get_buffer_rw(rect, &mut stride) {
//!     // Modify pixel data...
//!     // (Remember: stride is in pixels!)
//! }
//! buffer.commit_buffer(rect);  // Must call when done!
//! ```
//!
//! # Rendering Operations
//!
//! The [`MutablePixelBuffer`] trait provides high-level rendering methods:
//!
//! - [`fill_rect()`](MutablePixelBuffer::fill_rect) - Fill with solid color
//! - [`copy_rect()`](MutablePixelBuffer::copy_rect) - Copy within buffer (handles overlaps)
//! - [`image_rect()`](MutablePixelBuffer::image_rect) - Copy from external data

use crate::PixelFormat;
use anyhow::Result;
use rfb_common::{Point, Rect};

/// Read-only pixel buffer access.
///
/// This trait provides read-only access to pixel data in a framebuffer or similar
/// structure. Implementations must guarantee that pixel data remains valid and
/// unchanged during read access.
///
/// # Stride Convention
///
/// All stride values are in **pixels**, not bytes. See module documentation for details.
///
/// # Example
///
/// ```no_run
/// use rfb_pixelbuffer::{PixelBuffer, PixelFormat};
/// use rfb_common::Rect;
///
/// fn process_buffer<B: PixelBuffer>(buffer: &B) {
///     let (width, height) = buffer.dimensions();
///     let format = buffer.pixel_format();
///     
///     println!("Buffer: {}x{}, format: {:?}", width, height, format);
///     
///     // Access entire buffer
///     let rect = Rect::new(0, 0, width, height);
///     let mut stride = 0;
///     if let Some(pixels) = buffer.get_buffer(rect, &mut stride) {
///         println!("Got {} bytes with stride {} pixels", pixels.len(), stride);
///     }
/// }
/// ```
pub trait PixelBuffer {
    /// Returns the dimensions of the pixel buffer as (width, height).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::PixelBuffer;
    /// # fn example<B: PixelBuffer>(buffer: &B) {
    /// let (width, height) = buffer.dimensions();
    /// println!("Buffer is {}x{} pixels", width, height);
    /// # }
    /// ```
    fn dimensions(&self) -> (u32, u32);

    /// Returns a reference to the pixel format used by this buffer.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::PixelBuffer;
    /// # fn example<B: PixelBuffer>(buffer: &B) {
    /// let format = buffer.pixel_format();
    /// println!("Bits per pixel: {}", format.bits_per_pixel);
    /// # }
    /// ```
    fn pixel_format(&self) -> &PixelFormat;

    /// Gets read-only access to a rectangular region of pixel data.
    ///
    /// # Parameters
    ///
    /// - `rect`: The rectangular region to access
    /// - `stride`: Output parameter receiving the stride in **pixels** (not bytes!)
    ///
    /// # Returns
    ///
    /// - `Some(&[u8])`: Slice containing the pixel data if successful
    /// - `None`: If the rectangle is invalid or out of bounds
    ///
    /// # Stride Convention
    ///
    /// The `stride` parameter is set to the number of **pixels** per row, not bytes.
    /// To calculate byte offsets:
    ///
    /// ```text
    /// bytes_per_pixel = pixel_format().bytes_per_pixel()
    /// byte_offset = (y * stride + x) * bytes_per_pixel
    /// ```
    ///
    /// # Important
    ///
    /// The returned slice may contain more data than just the requested rectangle.
    /// Use the stride value to correctly navigate through rows.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::PixelBuffer;
    /// # use rfb_common::Rect;
    /// # fn example<B: PixelBuffer>(buffer: &B) {
    /// let rect = Rect::new(10, 10, 50, 50);
    /// let mut stride = 0;
    ///
    /// if let Some(pixels) = buffer.get_buffer(rect, &mut stride) {
    ///     let bpp = buffer.pixel_format().bytes_per_pixel() as usize;
    ///     println!("Got pixels with stride {} pixels ({} bytes per row)",
    ///              stride, stride * bpp);
    /// }
    /// # }
    /// ```
    fn get_buffer(&self, rect: Rect, stride: &mut usize) -> Option<&[u8]>;
}

/// Mutable pixel buffer with rendering operations.
///
/// This trait extends [`PixelBuffer`] with write access and common rendering
/// operations like filling, copying, and blitting image data.
///
/// # Usage Pattern
///
/// For direct pixel manipulation:
/// 1. Call [`get_buffer_rw()`](Self::get_buffer_rw) to get mutable access
/// 2. Modify the pixel data
/// 3. Call [`commit_buffer()`](Self::commit_buffer) to finalize changes
///
/// For rendering operations, use the high-level methods:
/// - [`fill_rect()`](Self::fill_rect)
/// - [`copy_rect()`](Self::copy_rect)
/// - [`image_rect()`](Self::image_rect)
///
/// # Example
///
/// ```no_run
/// use rfb_pixelbuffer::{MutablePixelBuffer, PixelFormat};
/// use rfb_common::Rect;
///
/// fn clear_screen<B: MutablePixelBuffer>(buffer: &mut B) -> anyhow::Result<()> {
///     let (width, height) = buffer.dimensions();
///     let rect = Rect::new(0, 0, width, height);
///     
///     // Fill with black (all zeros)
///     let black = vec![0u8; buffer.pixel_format().bytes_per_pixel() as usize];
///     buffer.fill_rect(rect, &black)?;
///     
///     Ok(())
/// }
/// ```
pub trait MutablePixelBuffer: PixelBuffer {
    /// Gets read-write access to a rectangular region of pixel data.
    ///
    /// # Parameters
    ///
    /// - `rect`: The rectangular region to access
    /// - `stride`: Output parameter receiving the stride in **pixels** (not bytes!)
    ///
    /// # Returns
    ///
    /// - `Some(&mut [u8])`: Mutable slice containing the pixel data if successful
    /// - `None`: If the rectangle is invalid or out of bounds
    ///
    /// # Important
    ///
    /// After modifying the pixel data, you **must** call [`commit_buffer()`](Self::commit_buffer)
    /// with the same rectangle to finalize the changes.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::{MutablePixelBuffer, PixelBuffer};
    /// # use rfb_common::Rect;
    /// # fn example<B: MutablePixelBuffer>(buffer: &mut B) {
    /// let rect = Rect::new(0, 0, 100, 100);
    /// let mut stride = 0;
    /// let bpp = buffer.pixel_format().bytes_per_pixel() as usize;
    ///
    /// if let Some(pixels) = buffer.get_buffer_rw(rect, &mut stride) {
    ///     // Modify pixels...
    ///     for i in (0..pixels.len()).step_by(bpp) {
    ///         pixels[i] = 255; // Set red channel to max
    ///     }
    /// }
    /// buffer.commit_buffer(rect);  // Must call!
    /// # }
    /// ```
    fn get_buffer_rw(&mut self, rect: Rect, stride: &mut usize) -> Option<&mut [u8]>;

    /// Commits changes made via [`get_buffer_rw()`](Self::get_buffer_rw).
    ///
    /// # Parameters
    ///
    /// - `rect`: The same rectangle that was passed to `get_buffer_rw()`
    ///
    /// # Important
    ///
    /// This method must be called after every `get_buffer_rw()` call to ensure
    /// changes are properly stored. Failing to call this may result in:
    /// - Lost changes
    /// - Memory leaks (temporary buffers not freed)
    /// - Undefined behavior
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::{MutablePixelBuffer, PixelBuffer};
    /// # use rfb_common::Rect;
    /// # fn example<B: MutablePixelBuffer>(buffer: &mut B) {
    /// let rect = Rect::new(10, 10, 50, 50);
    /// let mut stride = 0;
    ///
    /// if let Some(pixels) = buffer.get_buffer_rw(rect, &mut stride) {
    ///     // Modify pixels...
    /// }
    /// buffer.commit_buffer(rect);  // Required!
    /// # }
    /// ```
    fn commit_buffer(&mut self, rect: Rect);

    /// Fills a rectangle with a solid color.
    ///
    /// # Parameters
    ///
    /// - `rect`: The rectangle to fill
    /// - `pixel`: The pixel value to fill with (must match pixel format size)
    ///
    /// # Returns
    ///
    /// - `Ok(())`: If the fill succeeded
    /// - `Err`: If the rectangle is invalid or pixel size is wrong
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::{MutablePixelBuffer, PixelFormat};
    /// # use rfb_common::Rect;
    /// # fn example<B: MutablePixelBuffer>(buffer: &mut B) -> anyhow::Result<()> {
    /// let format = buffer.pixel_format().clone();
    /// let red = format.from_rgb888([255, 0, 0, 255]);
    ///
    /// let rect = Rect::new(10, 10, 100, 100);
    /// buffer.fill_rect(rect, &red)?;
    /// # Ok(())
    /// # }
    /// ```
    fn fill_rect(&mut self, rect: Rect, pixel: &[u8]) -> Result<()>;

    /// Copies a rectangle within the buffer.
    ///
    /// This handles overlapping source and destination regions correctly.
    ///
    /// # Parameters
    ///
    /// - `dest`: Destination rectangle
    /// - `src_offset`: Offset from `dest` to the source position
    ///
    /// # Returns
    ///
    /// - `Ok(())`: If the copy succeeded
    /// - `Err`: If either rectangle is invalid
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::MutablePixelBuffer;
    /// # use rfb_common::{Rect, Point};
    /// # fn example<B: MutablePixelBuffer>(buffer: &mut B) -> anyhow::Result<()> {
    /// // Copy from (0, 0) to (100, 100)
    /// let dest = Rect::new(100, 100, 50, 50);
    /// let src_offset = Point::new(-100, -100);  // Source is 100 pixels left and up
    ///
    /// buffer.copy_rect(dest, src_offset)?;
    /// # Ok(())
    /// # }
    /// ```
    fn copy_rect(&mut self, dest: Rect, src_offset: Point) -> Result<()>;

    /// Copies image data into a rectangle.
    ///
    /// # Parameters
    ///
    /// - `dest`: Destination rectangle
    /// - `pixels`: Source pixel data (must match pixel format)
    /// - `stride`: Source stride in **pixels** (0 = tightly packed)
    ///
    /// # Returns
    ///
    /// - `Ok(())`: If the copy succeeded
    /// - `Err`: If the rectangle is invalid or data size is wrong
    ///
    /// # Stride Parameter
    ///
    /// If `stride` is 0, the source data is assumed to be tightly packed
    /// (width of rectangle in pixels). Otherwise, use the specified stride.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rfb_pixelbuffer::{MutablePixelBuffer, PixelBuffer};
    /// # use rfb_common::Rect;
    /// # fn example<B: MutablePixelBuffer>(buffer: &mut B) -> anyhow::Result<()> {
    /// let rect = Rect::new(10, 10, 100, 100);
    /// let bpp = buffer.pixel_format().bytes_per_pixel() as usize;
    ///
    /// // Tightly packed image data
    /// let image_data = vec![0u8; 100 * 100 * bpp];
    /// buffer.image_rect(rect, &image_data, 0)?;  // stride=0 means tightly packed
    /// # Ok(())
    /// # }
    /// ```
    fn image_rect(&mut self, dest: Rect, pixels: &[u8], stride: usize) -> Result<()>;
}
