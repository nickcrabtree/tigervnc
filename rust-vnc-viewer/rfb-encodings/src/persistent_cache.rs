//! PersistentCache - Disk-backable, content-hash addressed cache for rectangles.

use crate::arc_cache::ArcCache;
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
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Debug)]
pub struct PersistentClientCache {
    map: HashMap<[u8; 16], PersistentCachedPixels>,
    max_size_mb: usize,
    current_bytes: usize,
    /// ARC eviction core tracking resident and ghost entries by cache ID.
    arc: ArcCache<[u8; 16]>,
}

impl PersistentClientCache {
    pub fn new(max_size_mb: usize) -> Self {
        let max_bytes = max_size_mb.saturating_mul(1024 * 1024);
        Self {
            map: HashMap::new(),
            max_size_mb,
            current_bytes: 0,
            arc: ArcCache::new(max_bytes),
        }
    }

    pub fn lookup(&mut self, id: &[u8; 16]) -> Option<&PersistentCachedPixels> {
        if let Some(entry) = self.map.get(id) {
            // Notify ARC of a resident hit so it can adapt between T1/T2.
            self.arc.on_hit(id);
            Some(entry)
        } else {
            None
        }
    }

    /// Insert or replace an entry in the client cache.
    ///
    /// This integrates with the shared ARC core for eviction. The ARC operates
    /// purely on cache IDs and byte sizes; this layer owns the actual payloads.
    pub fn insert(&mut self, entry: PersistentCachedPixels) {
        let id = entry.id;
        let size = entry.bytes();

        // Remove any existing resident entry for this id from both the map and ARC.
        if let Some(old) = self.map.remove(&id) {
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes());
            let _ = self.arc.remove_resident(&id);
        }

        // Let ARC decide which entries to evict to make room for this one.
        let evicted_ids = self.arc.insert_resident(id, size);
        for evicted_id in evicted_ids {
            if let Some(old) = self.map.remove(&evicted_id) {
                self.current_bytes = self.current_bytes.saturating_sub(old.bytes());
            }
        }

        self.current_bytes = self.current_bytes.saturating_add(size);
        self.map.insert(id, entry);
    }

    /// Current cache usage in bytes.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Configured capacity in megabytes.
    pub fn max_size_mb(&self) -> usize {
        self.max_size_mb
    }

    /// Retrieve and clear the list of cache IDs that were evicted by the ARC
    /// core since the last call.
    pub fn take_evicted_ids(&mut self) -> Vec<[u8; 16]> {
        self.arc.take_pending_evictions()
    }
}

impl Default for PersistentClientCache {
    fn default() -> Self {
        Self::new(0)
    }
}
