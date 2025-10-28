//! PersistentCache - Disk-backable, content-hash addressed cache for rectangles.

use rfb_pixelbuffer::PixelFormat;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PersistentCachedPixels {
    pub id: [u8; 16],
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    /// Stride in pixels (CRITICAL: pixels, not bytes)
    pub stride_pixels: usize,
    pub last_used: Instant,
}

impl PersistentCachedPixels {
    pub fn bytes(&self) -> usize { self.pixels.len() }
}

#[derive(Debug, Default)]
pub struct PersistentClientCache {
    map: HashMap<[u8; 16], PersistentCachedPixels>,
    max_size_mb: usize,
    current_bytes: usize,
}

impl PersistentClientCache {
    pub fn new(max_size_mb: usize) -> Self {
        Self { map: HashMap::new(), max_size_mb, current_bytes: 0 }
    }

    pub fn lookup(&mut self, id: &[u8;16]) -> Option<&PersistentCachedPixels> {
        self.map.get(id)
    }

    pub fn insert(&mut self, entry: PersistentCachedPixels) {
        let size = entry.bytes();
        self.current_bytes += size;
        // TODO: Evict by ARC policy; for now simple insert
        self.map.insert(entry.id, entry);
    }
}