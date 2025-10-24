//! RFB pixel format descriptions and conversions.
//!
//! This module defines the [`PixelFormat`] type which describes how pixels are encoded
//! in the RFB protocol. It handles various color depths, endianness, and channel layouts.
//!
//! # True Color Model
//!
//! The RFB protocol supports two color models:
//! - **True color** (direct color): Each pixel directly encodes RGB values using bit fields
//! - **Color map**: Pixels are indices into a separate color lookup table
//!
//! This implementation focuses on **true color** formats. Color map support is not implemented.
//!
//! # Pixel Format Components
//!
//! - **bits_per_pixel**: Storage size in bits (typically 8, 16, or 32)
//! - **depth**: Actual color depth (sum of significant bits in R, G, B channels)
//! - **big_endian**: Byte order for multi-byte pixels
//! - **red/green/blue_max**: Maximum value for each color channel (e.g., 255 for 8-bit)
//! - **red/green/blue_shift**: Bit position of the least significant bit of each channel
//!
//! # Channel Extraction and Scaling
//!
//! To extract a color component from a pixel value:
//! 1. Shift right by the channel's shift value
//! 2. Mask with the channel's max value
//! 3. Scale to 8-bit: `(component * 255) / channel_max`
//!
//! # Critical Note: Stride is in Pixels, Not Bytes!
//!
//! **IMPORTANT**: Per TigerVNC's `WARP.md`, the stride in pixel buffers is measured in **pixels**,
//! not bytes. When calculating byte offsets, always multiply stride by `bytes_per_pixel()`.
//!
//! This was the source of a critical bug that caused hash collisions and visual corruption.
//! Always use: `byte_length = height * stride * bytes_per_pixel()`
//!
//! # Example
//!
//! ```
//! use rfb_pixelbuffer::PixelFormat;
//!
//! // Create standard RGB888 format (32bpp, little-endian)
//! let pf = PixelFormat::rgb888();
//! assert_eq!(pf.bytes_per_pixel(), 4);
//! assert_eq!(pf.depth, 24);
//!
//! // Convert a pixel to RGBA8888
//! let pixel = [0xCC, 0xBB, 0xAA, 0x00]; // Little-endian: 0x00AABBCC
//! let rgba = pf.to_rgb888(&pixel);
//! assert_eq!(rgba, [0xAA, 0xBB, 0xCC, 0xFF]);
//!
//! // Convert RGBA8888 back to pixel format
//! let raw = pf.from_rgb888([0xAA, 0xBB, 0xCC, 0xFF]);
//! assert_eq!(raw, vec![0xCC, 0xBB, 0xAA, 0x00]);
//! ```

/// Describes an RFB pixel format and provides conversions to/from RGB888.
///
/// This structure contains all the information needed to encode and decode pixels
/// in the RFB protocol. It handles various bit depths, endianness, and color channel
/// layouts.
///
/// # Standard Formats
///
/// Use [`PixelFormat::rgb888()`] for the most common format: 32-bit RGBA with
/// 8 bits per channel, little-endian byte order.
#[derive(Debug, Clone, PartialEq, Copy)]
pub struct PixelFormat {
    /// Bits used per pixel (bpp), e.g., 32 for RGB888 in 32-bit storage.
    pub bits_per_pixel: u8,

    /// Actual color depth (sum of significant bits), e.g., 24 for RGB888.
    pub depth: u8,

    /// Byte order for multi-byte pixels (`true` = big endian, `false` = little endian).
    pub big_endian: bool,

    /// True color (direct color) vs. color map (`false`).
    ///
    /// Only true color formats are currently supported.
    pub true_color: bool,

    /// Maximum valid red component value in this format (e.g., 255 for 8-bit red).
    pub red_max: u16,

    /// Maximum valid green component value in this format.
    pub green_max: u16,

    /// Maximum valid blue component value in this format.
    pub blue_max: u16,

    /// Bit shift for the least significant bit of the red component.
    pub red_shift: u8,

    /// Bit shift for the least significant bit of the green component.
    pub green_shift: u8,

    /// Bit shift for the least significant bit of the blue component.
    pub blue_shift: u8,
}

impl PixelFormat {
    /// Returns bytes-per-pixel (storage width), rounded up to the nearest byte.
    ///
    /// # Note
    ///
    /// This returns the **byte** size of a single pixel. In contrast, stride values
    /// elsewhere in the pixel buffer API are measured in **pixels**, not bytes.
    /// Always multiply stride by `bytes_per_pixel()` when calculating byte offsets.
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    /// assert_eq!(pf.bytes_per_pixel(), 4); // 32 bits = 4 bytes
    /// ```
    pub fn bytes_per_pixel(&self) -> u8 {
        self.bits_per_pixel.div_ceil(8)
    }

    /// Returns a standard little-endian 32bpp RGB888 pixel format.
    ///
    /// This is the most common format:
    /// - 32 bits per pixel (4 bytes)
    /// - 24-bit color depth (8 bits per channel)
    /// - Little-endian byte order
    /// - Red at bit 16, Green at bit 8, Blue at bit 0
    ///
    /// In memory, a pixel with R=0xAA, G=0xBB, B=0xCC is stored as:
    /// `[0xCC, 0xBB, 0xAA, 0x00]` (blue, green, red, padding)
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    /// assert_eq!(pf.bits_per_pixel, 32);
    /// assert_eq!(pf.depth, 24);
    /// assert_eq!(pf.big_endian, false);
    /// assert_eq!(pf.red_shift, 16);
    /// assert_eq!(pf.green_shift, 8);
    /// assert_eq!(pf.blue_shift, 0);
    /// ```
    pub fn rgb888() -> Self {
        Self {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: false,
            true_color: true,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        }
    }

    /// Converts a pixel from this format to RGBA8888 `[R, G, B, A]` where `A=255`.
    ///
    /// # Panics
    ///
    /// Panics if `pixel.len()` does not equal `self.bytes_per_pixel()`, or if any
    /// color channel max value is zero (invalid format).
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    ///
    /// // Pixel value 0x00112233 stored little-endian => [0x33, 0x22, 0x11, 0x00]
    /// let pixel = [0x33, 0x22, 0x11, 0x00];
    /// let rgba = pf.to_rgb888(&pixel);
    /// assert_eq!(rgba, [0x11, 0x22, 0x33, 0xFF]);
    /// ```
    ///
    /// # Algorithm
    ///
    /// 1. Assemble bytes into a u32 value according to endianness
    /// 2. Extract each color component by shifting and masking
    /// 3. Scale each component from its format range to 0-255
    /// 4. Return as `[R, G, B, 255]`
    pub fn to_rgb888(&self, pixel: &[u8]) -> [u8; 4] {
        let bpp = self.bytes_per_pixel() as usize;
        assert_eq!(
            pixel.len(),
            bpp,
            "pixel length {} does not match bytes_per_pixel {}",
            pixel.len(),
            bpp
        );

        // Assemble pixel value from bytes according to endianness
        let mut value = 0u32;
        if self.big_endian {
            // Big endian: MSB first
            for &byte in pixel.iter().take(bpp) {
                value = (value << 8) | (byte as u32);
            }
        } else {
            // Little endian: LSB first
            for (i, &byte) in pixel.iter().take(bpp).enumerate() {
                value |= (byte as u32) << (i * 8);
            }
        }

        // Extract color components by shifting and masking
        let r = ((value >> self.red_shift) & (self.red_max as u32)) as u16;
        let g = ((value >> self.green_shift) & (self.green_max as u32)) as u16;
        let b = ((value >> self.blue_shift) & (self.blue_max as u32)) as u16;

        // Scale to 8-bit (0-255)
        assert!(self.red_max > 0, "red_max must be > 0");
        assert!(self.green_max > 0, "green_max must be > 0");
        assert!(self.blue_max > 0, "blue_max must be > 0");

        let r8 = ((r * 255) / self.red_max) as u8;
        let g8 = ((g * 255) / self.green_max) as u8;
        let b8 = ((b * 255) / self.blue_max) as u8;

        [r8, g8, b8, 255]
    }

    /// Converts an RGBA8888 pixel `[R, G, B, A]` to this format.
    ///
    /// The alpha channel is ignored (only RGB channels are encoded).
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    /// let rgba = [0xAA, 0xBB, 0xCC, 0xFF];
    /// let raw = pf.from_rgb888(rgba);
    ///
    /// // For RGB888 little-endian 32bpp with shifts R:16, G:8, B:0:
    /// // Pixel value = 0x00AABBCC => bytes [0xCC, 0xBB, 0xAA, 0x00]
    /// assert_eq!(raw, vec![0xCC, 0xBB, 0xAA, 0x00]);
    /// ```
    ///
    /// # Round-trip Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    /// let original = [0x12, 0x34, 0x56, 0xFF];
    ///
    /// let encoded = pf.from_rgb888(original);
    /// let decoded = pf.to_rgb888(&encoded);
    ///
    /// assert_eq!(decoded, original);
    /// ```
    ///
    /// # Algorithm
    ///
    /// 1. Scale each RGB component from 0-255 to its format range
    /// 2. Shift each component to its bit position
    /// 3. Combine into a single pixel value
    /// 4. Write bytes according to endianness
    pub fn from_rgb888(&self, rgb: [u8; 4]) -> Vec<u8> {
        // Scale from 8-bit to format range
        let r = (rgb[0] as u32 * self.red_max as u32) / 255;
        let g = (rgb[1] as u32 * self.green_max as u32) / 255;
        let b = (rgb[2] as u32 * self.blue_max as u32) / 255;

        // Compose pixel value by shifting to bit positions
        let mut value = (r << self.red_shift) | (g << self.green_shift) | (b << self.blue_shift);

        // Write bytes according to endianness
        let bpp = self.bytes_per_pixel() as usize;
        let mut result = vec![0u8; bpp];

        if self.big_endian {
            // Big endian: MSB first
            for i in 0..bpp {
                result[bpp - 1 - i] = (value & 0xFF) as u8;
                value >>= 8;
            }
        } else {
            // Little endian: LSB first
            for item in result.iter_mut().take(bpp) {
                *item = (value & 0xFF) as u8;
                value >>= 8;
            }
        }

        result
    }

    /// Check if this pixel format is RGB888 (32bpp, 24-bit depth, little-endian).
    ///
    /// # Example
    ///
    /// ```
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let pf = PixelFormat::rgb888();
    /// assert!(pf.is_rgb888());
    /// ```
    pub fn is_rgb888(&self) -> bool {
        self.bits_per_pixel == 32 &&
        self.depth == 24 &&
        !self.big_endian &&
        self.true_color &&
        self.red_max == 255 &&
        self.green_max == 255 &&
        self.blue_max == 255 &&
        self.red_shift == 16 &&
        self.green_shift == 8 &&
        self.blue_shift == 0
    }
}

/// Convert from protocol PixelFormat to pixelbuffer PixelFormat.
impl From<rfb_protocol::messages::types::PixelFormat> for PixelFormat {
    fn from(pf: rfb_protocol::messages::types::PixelFormat) -> Self {
        Self {
            bits_per_pixel: pf.bits_per_pixel,
            depth: pf.depth,
            big_endian: pf.big_endian != 0,
            true_color: pf.true_color != 0,
            red_max: pf.red_max,
            green_max: pf.green_max,
            blue_max: pf.blue_max,
            red_shift: pf.red_shift,
            green_shift: pf.green_shift,
            blue_shift: pf.blue_shift,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_per_pixel() {
        let pf = PixelFormat::rgb888();
        assert_eq!(pf.bytes_per_pixel(), 4);

        // Test rounding up
        let pf_12bit = PixelFormat {
            bits_per_pixel: 12,
            depth: 12,
            big_endian: false,
            true_color: true,
            red_max: 15,
            green_max: 15,
            blue_max: 15,
            red_shift: 8,
            green_shift: 4,
            blue_shift: 0,
        };
        assert_eq!(pf_12bit.bytes_per_pixel(), 2); // 12 bits rounds up to 2 bytes
    }

    #[test]
    fn test_rgb888_format() {
        let pf = PixelFormat::rgb888();
        assert_eq!(pf.bits_per_pixel, 32);
        assert_eq!(pf.depth, 24);
        assert!(!pf.big_endian);
        assert!(pf.true_color);
        assert_eq!(pf.red_max, 255);
        assert_eq!(pf.green_max, 255);
        assert_eq!(pf.blue_max, 255);
        assert_eq!(pf.red_shift, 16);
        assert_eq!(pf.green_shift, 8);
        assert_eq!(pf.blue_shift, 0);
    }

    #[test]
    fn test_to_rgb888_little_endian() {
        let pf = PixelFormat::rgb888();

        // 0x00112233 little-endian = [0x33, 0x22, 0x11, 0x00]
        let pixel = [0x33, 0x22, 0x11, 0x00];
        let rgba = pf.to_rgb888(&pixel);
        assert_eq!(rgba, [0x11, 0x22, 0x33, 0xFF]);
    }

    #[test]
    fn test_from_rgb888_little_endian() {
        let pf = PixelFormat::rgb888();

        let rgba = [0xAA, 0xBB, 0xCC, 0xFF];
        let raw = pf.from_rgb888(rgba);
        // 0x00AABBCC little-endian = [0xCC, 0xBB, 0xAA, 0x00]
        assert_eq!(raw, vec![0xCC, 0xBB, 0xAA, 0x00]);
    }

    #[test]
    fn test_round_trip_rgb888() {
        let pf = PixelFormat::rgb888();

        let original = [0x12, 0x34, 0x56, 0xFF];
        let encoded = pf.from_rgb888(original);
        let decoded = pf.to_rgb888(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_big_endian_conversion() {
        let pf = PixelFormat {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: true, // Big endian
            true_color: true,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        };

        // 0x00112233 big-endian = [0x00, 0x11, 0x22, 0x33]
        let pixel = [0x00, 0x11, 0x22, 0x33];
        let rgba = pf.to_rgb888(&pixel);
        assert_eq!(rgba, [0x11, 0x22, 0x33, 0xFF]);

        // Round trip
        let encoded = pf.from_rgb888([0xAA, 0xBB, 0xCC, 0xFF]);
        assert_eq!(encoded, vec![0x00, 0xAA, 0xBB, 0xCC]);
        let decoded = pf.to_rgb888(&encoded);
        assert_eq!(decoded, [0xAA, 0xBB, 0xCC, 0xFF]);
    }

    #[test]
    fn test_rgb565_format() {
        // 16-bit RGB565: 5 bits red, 6 bits green, 5 bits blue
        let pf = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: false,
            true_color: true,
            red_max: 31,   // 5 bits
            green_max: 63, // 6 bits
            blue_max: 31,  // 5 bits
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
        };

        assert_eq!(pf.bytes_per_pixel(), 2);

        // Test conversion: max values should scale to 255
        let rgba = [255, 255, 255, 255];
        let encoded = pf.from_rgb888(rgba);
        assert_eq!(encoded.len(), 2);

        let decoded = pf.to_rgb888(&encoded);
        assert_eq!(decoded, [255, 255, 255, 255]);
    }

    #[test]
    #[should_panic(expected = "pixel length")]
    fn test_to_rgb888_wrong_size_panics() {
        let pf = PixelFormat::rgb888();
        let wrong_size = [0x11, 0x22]; // Only 2 bytes, need 4
        pf.to_rgb888(&wrong_size);
    }

    #[test]
    #[should_panic(expected = "red_max must be > 0")]
    fn test_to_rgb888_zero_max_panics() {
        let pf = PixelFormat {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: false,
            true_color: true,
            red_max: 0, // Invalid!
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        };
        let pixel = [0x00, 0x00, 0x00, 0x00];
        pf.to_rgb888(&pixel);
    }
}
