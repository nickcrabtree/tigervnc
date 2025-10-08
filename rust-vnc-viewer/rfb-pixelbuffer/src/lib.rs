//! RFB pixel buffer types and utilities.
//!
//! This crate provides pixel format descriptions and buffer management for the
//! RFB/VNC protocol implementation.
//!
//! # Modules
//!
//! - [`format`]: Pixel format definitions and conversions
//! - [`buffer`]: Pixel buffer traits for read and write access
//!
//! # Key Types
//!
//! - [`PixelFormat`]: Describes how pixels are encoded (bit depth, endianness, etc.)
//! - [`PixelBuffer`]: Trait for read-only buffer access
//! - [`MutablePixelBuffer`]: Trait for read-write buffer access with rendering operations

pub mod buffer;
pub mod format;

pub use buffer::{MutablePixelBuffer, PixelBuffer};
pub use format::PixelFormat;
