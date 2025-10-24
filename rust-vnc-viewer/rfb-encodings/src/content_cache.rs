//! ContentCache - Content-addressable historical cache for VNC rectangles.
//!
//! This module provides a high-performance cache for decoded VNC rectangle data,
//! enabling 97-99% bandwidth reduction by referencing previously-seen content
//! instead of re-encoding it.
//!
//! ## Architecture
//!
//! - **Storage**: HashMap<u64, CachedPixels> for O(1) lookup by cache_id
//! - **Eviction**: LRU (Least Recently Used) algorithm with configurable size limits
//! - **Statistics**: Hit/miss rates, memory usage, and cache efficiency metrics
//!
//! ## Usage
//!
//! ```rust
//! use rfb_encodings::content_cache::{ContentCache, CachedPixels};
//! use rfb_pixelbuffer::PixelFormat;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create cache with 1GB limit
//! let mut cache = ContentCache::new(1024);
//!
//! // Store decoded pixels
//! let pixels = CachedPixels::new(
//!     12345,
//!     vec![0xFF; 64 * 64 * 4],  // RGBA data
//!     PixelFormat::rgb888(),
//!     64, 64, 64
//! );
//! cache.insert(12345, pixels)?;
//!
//! // Lookup later
//! if let Some(cached) = cache.lookup(12345) {
//!     println!("Cache hit! Pixels: {} bytes", cached.pixels.len());
//! }
//!
//! // Check statistics
//! let stats = cache.stats();
//! println!("Hit rate: {:.1}%", stats.hit_rate * 100.0);
//! # Ok(())
//! # }
//! ```

use rfb_pixelbuffer::PixelFormat;
use std::collections::HashMap;
use std::time::Instant;
use anyhow::Result;

/// Cached pixel data with metadata.
///
/// Stores decoded pixel data along with format information and
/// access tracking for LRU eviction.
#[derive(Debug, Clone)]
pub struct CachedPixels {
    /// Unique identifier for this cached content.
    pub cache_id: u64,
    
    /// Decoded pixel data in the specified format.
    pub pixels: Vec<u8>,
    
    /// Pixel format (bits per pixel, color channels, etc).
    pub format: PixelFormat,
    
    /// Rectangle width in pixels.
    pub width: u32,
    
    /// Rectangle height in pixels.
    pub height: u32,
    
    /// Row stride in pixels (may be larger than width for alignment).
    pub stride: usize,
    
    /// Last access time for LRU eviction.
    pub last_used: Instant,
    
    /// Creation time for age-based debugging.
    pub created_at: Instant,
}

impl CachedPixels {
    /// Create new cached pixel data.
    pub fn new(
        cache_id: u64,
        pixels: Vec<u8>,
        format: PixelFormat,
        width: u32,
        height: u32,
        stride: usize,
    ) -> Self {
        let now = Instant::now();
        Self {
            cache_id,
            pixels,
            format,
            width,
            height,
            stride,
            last_used: now,
            created_at: now,
        }
    }
    
    /// Get the memory size of this cached entry in bytes.
    pub fn memory_size(&self) -> usize {
        self.pixels.len() + std::mem::size_of::<Self>()
    }
    
    /// Get the age of this cache entry.
    pub fn age(&self) -> std::time::Duration {
        Instant::now().duration_since(self.created_at)
    }
    
    /// Mark this entry as recently used.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }
}

/// Cache statistics for monitoring and debugging.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cache entries.
    pub entries: usize,
    
    /// Memory usage in megabytes.
    pub size_mb: usize,
    
    /// Maximum memory limit in megabytes.
    pub max_size_mb: usize,
    
    /// Cache hit rate (0.0 to 1.0).
    pub hit_rate: f64,
    
    /// Total number of cache hits.
    pub hit_count: u64,
    
    /// Total number of cache misses.
    pub miss_count: u64,
    
    /// Total number of evictions.
    pub eviction_count: u64,
    
    /// Number of bytes saved by cache hits.
    pub bytes_saved: u64,
    
    /// Average cache entry size in bytes.
    pub avg_entry_size: usize,
}

impl CacheStats {
    /// Get the total number of cache accesses.
    pub fn total_accesses(&self) -> u64 {
        self.hit_count + self.miss_count
    }
    
    /// Get cache utilization as a percentage (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        if self.max_size_mb == 0 {
            0.0
        } else {
            self.size_mb as f64 / self.max_size_mb as f64
        }
    }
}

/// ContentCache - High-performance content-addressable cache.
///
/// Maintains a cache of decoded pixel data indexed by content hash (cache_id).
/// Uses LRU eviction to stay within memory limits.
///
/// ## Thread Safety
///
/// ContentCache is NOT thread-safe by itself. Use `Arc<Mutex<ContentCache>>`
/// for multi-threaded access.
///
/// ## Memory Management
///
/// The cache enforces a maximum memory limit in MB. When the limit would be
/// exceeded by a new insertion, the least recently used entries are evicted
/// until sufficient space is available.
pub struct ContentCache {
    /// Main storage for cached pixels.
    pixels: HashMap<u64, CachedPixels>,
    
    /// Maximum cache size in megabytes.
    max_size_mb: usize,
    
    /// Current memory usage in bytes.
    current_size_bytes: usize,
    
    /// Total number of cache hits.
    hit_count: u64,
    
    /// Total number of cache misses.
    miss_count: u64,
    
    /// Total number of evictions performed.
    eviction_count: u64,
    
    /// Total bytes saved by cache hits (estimated).
    bytes_saved: u64,
    
    /// Cache creation time for metrics.
    created_at: Instant,
}

impl ContentCache {
    /// Create a new ContentCache with the specified size limit.
    ///
    /// # Parameters
    ///
    /// - `max_size_mb`: Maximum cache size in megabytes (0 = unlimited)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rfb_encodings::content_cache::ContentCache;
    ///
    /// // Create 2GB cache
    /// let cache = ContentCache::new(2048);
    ///
    /// // Create unlimited cache (not recommended for production)
    /// let unlimited_cache = ContentCache::new(0);
    /// ```
    pub fn new(max_size_mb: usize) -> Self {
        Self {
            pixels: HashMap::new(),
            max_size_mb,
            current_size_bytes: 0,
            hit_count: 0,
            miss_count: 0,
            eviction_count: 0,
            bytes_saved: 0,
            created_at: Instant::now(),
        }
    }
    
    /// Insert cached pixels into the cache.
    ///
    /// If inserting this entry would exceed the memory limit, least recently
    /// used entries are evicted first.
    ///
    /// # Parameters
    ///
    /// - `cache_id`: Unique identifier for this content
    /// - `pixels`: Cached pixel data to store
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - cache_id is 0 (reserved)
    /// - Entry is larger than total cache size limit
    /// - Eviction fails to free sufficient space
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rfb_encodings::content_cache::{ContentCache, CachedPixels};
    /// use rfb_pixelbuffer::PixelFormat;
    ///
    /// let mut cache = ContentCache::new(100); // 100MB
    /// let pixels = CachedPixels::new(
    ///     12345,
    ///     vec![0; 1024],
    ///     PixelFormat::rgb888(),
    ///     32, 32, 32
    /// );
    ///
    /// cache.insert(12345, pixels)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn insert(&mut self, cache_id: u64, pixels: CachedPixels) -> Result<()> {
        if cache_id == 0 {
            anyhow::bail!("Cache ID 0 is reserved and cannot be used");
        }
        
        let entry_size = pixels.memory_size();
        
        // Check if entry is too large for the entire cache
        if self.max_size_mb > 0 && entry_size > self.max_size_mb * 1024 * 1024 {
            anyhow::bail!(
                "Entry size ({} bytes) exceeds total cache limit ({} MB)",
                entry_size,
                self.max_size_mb
            );
        }
        
        // Evict entries if necessary to make room
        if self.max_size_mb > 0 {
            let max_size_bytes = self.max_size_mb * 1024 * 1024;
            while self.current_size_bytes + entry_size > max_size_bytes {
                if !self.evict_lru()? {
                    anyhow::bail!("Failed to evict entries to make room for new cache entry");
                }
            }
        }
        
        // Remove existing entry if present (update case)
        if let Some(old_entry) = self.pixels.remove(&cache_id) {
            self.current_size_bytes -= old_entry.memory_size();
        }
        
        // Insert new entry
        self.current_size_bytes += entry_size;
        self.pixels.insert(cache_id, pixels);
        
        Ok(())
    }
    
    /// Look up cached pixels by cache_id.
    ///
    /// If found, the entry is marked as recently used for LRU tracking.
    ///
    /// # Parameters
    ///
    /// - `cache_id`: Unique identifier to look up
    ///
    /// # Returns
    ///
    /// - `Some(&CachedPixels)` if found (cache hit)
    /// - `None` if not found (cache miss)
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rfb_encodings::content_cache::ContentCache;
    /// let mut cache = ContentCache::new(100);
    ///
    /// // Cache miss
    /// assert!(cache.lookup(12345).is_none());
    ///
    /// // After insertion, cache hit
    /// // cache.insert(12345, pixels)?;
    /// // assert!(cache.lookup(12345).is_some());
    /// ```
    pub fn lookup(&mut self, cache_id: u64) -> Option<&CachedPixels> {
        if let Some(cached) = self.pixels.get_mut(&cache_id) {
            // Cache hit: update access time and statistics
            cached.touch();
            self.hit_count += 1;
            
            // Estimate bytes saved (approximate - would be actual encoding size)
            self.bytes_saved += cached.pixels.len() as u64;
            
            Some(cached)
        } else {
            // Cache miss: update statistics
            self.miss_count += 1;
            None
        }
    }
    
    /// Check if the cache contains an entry for the given cache_id.
    ///
    /// This is a read-only operation that doesn't update LRU ordering.
    pub fn contains(&self, cache_id: u64) -> bool {
        self.pixels.contains_key(&cache_id)
    }
    
    /// Remove an entry from the cache.
    ///
    /// Returns the removed entry if it existed.
    pub fn remove(&mut self, cache_id: u64) -> Option<CachedPixels> {
        if let Some(entry) = self.pixels.remove(&cache_id) {
            self.current_size_bytes -= entry.memory_size();
            Some(entry)
        } else {
            None
        }
    }
    
    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.pixels.clear();
        self.current_size_bytes = 0;
    }
    
    /// Get current cache statistics.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rfb_encodings::content_cache::ContentCache;
    /// let cache = ContentCache::new(1024);
    /// let stats = cache.stats();
    ///
    /// println!("Cache hit rate: {:.1}%", stats.hit_rate * 100.0);
    /// println!("Memory usage: {} MB / {} MB", stats.size_mb, stats.max_size_mb);
    /// println!("Entries: {}", stats.entries);
    /// ```
    pub fn stats(&self) -> CacheStats {
        let total_accesses = self.hit_count + self.miss_count;
        let hit_rate = if total_accesses > 0 {
            self.hit_count as f64 / total_accesses as f64
        } else {
            0.0
        };
        
        let avg_entry_size = if self.pixels.is_empty() {
            0
        } else {
            self.current_size_bytes / self.pixels.len()
        };
        
        CacheStats {
            entries: self.pixels.len(),
            size_mb: self.current_size_bytes / (1024 * 1024),
            max_size_mb: self.max_size_mb,
            hit_rate,
            hit_count: self.hit_count,
            miss_count: self.miss_count,
            eviction_count: self.eviction_count,
            bytes_saved: self.bytes_saved,
            avg_entry_size,
        }
    }
    
    /// Get cache capacity utilization as a percentage (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        if self.max_size_mb == 0 {
            0.0
        } else {
            let max_bytes = self.max_size_mb * 1024 * 1024;
            self.current_size_bytes as f64 / max_bytes as f64
        }
    }
    
    /// Get the age of this cache instance.
    pub fn age(&self) -> std::time::Duration {
        Instant::now().duration_since(self.created_at)
    }
    
    /// Evict the least recently used entry.
    ///
    /// Returns `true` if an entry was evicted, `false` if cache is empty.
    fn evict_lru(&mut self) -> Result<bool> {
        if self.pixels.is_empty() {
            return Ok(false);
        }
        
        // Find the least recently used entry
        let mut oldest_id = 0;
        let mut oldest_time = Instant::now();
        
        for (cache_id, cached) in &self.pixels {
            if cached.last_used < oldest_time {
                oldest_time = cached.last_used;
                oldest_id = *cache_id;
            }
        }
        
        // Remove the oldest entry
        if let Some(removed) = self.pixels.remove(&oldest_id) {
            self.current_size_bytes -= removed.memory_size();
            self.eviction_count += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    /// Force eviction of old entries to free up space.
    ///
    /// This can be called periodically to proactively manage memory usage.
    ///
    /// # Parameters
    ///
    /// - `target_utilization`: Target utilization (0.0 to 1.0, e.g., 0.8 for 80%)
    ///
    /// # Returns
    ///
    /// Number of entries evicted.
    pub fn compact(&mut self, target_utilization: f64) -> usize {
        if self.max_size_mb == 0 {
            return 0; // No size limit
        }
        
        let target_bytes = (self.max_size_mb as f64 * target_utilization * 1024.0 * 1024.0) as usize;
        let mut evicted_count = 0;
        
        while self.current_size_bytes > target_bytes {
            if self.evict_lru().unwrap_or(false) {
                evicted_count += 1;
            } else {
                break; // Cache is empty
            }
        }
        
        evicted_count
    }
}

impl Default for ContentCache {
    /// Create a ContentCache with default settings (2GB limit).
    fn default() -> Self {
        Self::new(2048) // 2GB default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfb_pixelbuffer::PixelFormat;

    fn make_test_pixels(cache_id: u64, width: u32, height: u32) -> CachedPixels {
        let stride = width as usize;
        let pixel_data = vec![0u8; (width * height * 4) as usize]; // RGBA
        CachedPixels::new(
            cache_id,
            pixel_data,
            PixelFormat::rgb888(),
            width,
            height,
            stride,
        )
    }

    #[test]
    fn test_cache_creation() {
        let cache = ContentCache::new(1024);
        assert_eq!(cache.max_size_mb, 1024);
        assert_eq!(cache.current_size_bytes, 0);
        assert_eq!(cache.pixels.len(), 0);
        
        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 0);
    }

    #[test]
    fn test_insert_and_lookup() -> Result<()> {
        let mut cache = ContentCache::new(100);
        let pixels = make_test_pixels(12345, 64, 64);
        
        // Insert
        cache.insert(12345, pixels.clone())?;
        
        // Verify entry exists
        assert_eq!(cache.pixels.len(), 1);
        assert!(cache.contains(12345));
        
        // Lookup (cache hit)
        let cached = cache.lookup(12345);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().cache_id, 12345);
        assert_eq!(cached.unwrap().width, 64);
        
        // Verify statistics
        let stats = cache.stats();
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 0);
        assert!(stats.hit_rate > 0.99); // Should be 1.0
        
        Ok(())
    }

    #[test]
    fn test_cache_miss() {
        let mut cache = ContentCache::new(100);
        
        // Lookup non-existent entry
        let result = cache.lookup(99999);
        assert!(result.is_none());
        
        // Verify statistics
        let stats = cache.stats();
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.hit_rate, 0.0);
    }

    #[test]
    fn test_insert_zero_id_rejected() {
        let mut cache = ContentCache::new(100);
        let pixels = make_test_pixels(0, 32, 32);
        
        let result = cache.insert(0, pixels);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_entry_too_large_rejected() {
        let mut cache = ContentCache::new(1); // 1MB limit
        // Create entry larger than 1MB
        let large_pixels = CachedPixels::new(
            12345,
            vec![0u8; 2 * 1024 * 1024], // 2MB
            PixelFormat::rgb888(),
            512, 512, 512,
        );
        
        let result = cache.insert(12345, large_pixels);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds total cache limit"));
    }

    #[test]
    fn test_lru_eviction() -> Result<()> {
        // Use very small cache to guarantee eviction
        let mut cache = ContentCache::new(1); // 1MB limit
        
        // Fill cache to very close to capacity with large entries
        let large_entry = CachedPixels::new(
            1,
            vec![0u8; 400_000], // 400KB
            PixelFormat::rgb888(),
            200, 200, 200,
        );
        cache.insert(1, large_entry)?;
        
        let large_entry2 = CachedPixels::new(
            2,
            vec![0u8; 400_000], // 400KB
            PixelFormat::rgb888(),
            200, 200, 200,
        );
        cache.insert(2, large_entry2)?;
        
        println!("After 2 large entries: {} bytes used", cache.current_size_bytes);
        
        // This should force eviction since 400KB + 400KB + 400KB > 1MB
        let large_entry3 = CachedPixels::new(
            3,
            vec![0u8; 400_000], // 400KB - should trigger eviction
            PixelFormat::rgb888(),
            200, 200, 200,
        );
        cache.insert(3, large_entry3)?;
        
        // The third entry should exist
        assert!(cache.contains(3));
        
        // Check that eviction occurred
        let stats = cache.stats();
        println!("After third entry: {} entries, {} bytes used, {} evictions", 
                stats.entries, cache.current_size_bytes, stats.eviction_count);
        
        // Should have fewer than 3 entries due to eviction
        assert!(stats.entries < 3, "Expected entries < 3 due to eviction, got {}", stats.entries);
        assert!(stats.eviction_count > 0, "Expected at least one eviction");
        
        Ok(())
    }

    #[test]
    fn test_cache_replacement() -> Result<()> {
        let mut cache = ContentCache::new(100);
        let pixels1 = make_test_pixels(12345, 64, 64);
        let pixels2 = make_test_pixels(12345, 128, 128); // Same ID, different size
        
        // Insert first entry
        cache.insert(12345, pixels1)?;
        assert_eq!(cache.pixels.len(), 1);
        let old_size = cache.current_size_bytes;
        
        // Replace with different size
        cache.insert(12345, pixels2)?;
        assert_eq!(cache.pixels.len(), 1); // Still one entry
        
        // Size should be updated
        let cached = cache.lookup(12345).unwrap();
        assert_eq!(cached.width, 128);
        
        // Memory accounting should be correct
        let new_size = cache.current_size_bytes;
        assert_ne!(old_size, new_size);
        
        Ok(())
    }

    #[test]
    fn test_remove() -> Result<()> {
        let mut cache = ContentCache::new(100);
        let pixels = make_test_pixels(12345, 64, 64);
        
        cache.insert(12345, pixels)?;
        assert!(cache.contains(12345));
        
        let removed = cache.remove(12345);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().cache_id, 12345);
        assert!(!cache.contains(12345));
        assert_eq!(cache.current_size_bytes, 0);
        
        Ok(())
    }

    #[test]
    fn test_clear() -> Result<()> {
        let mut cache = ContentCache::new(100);
        
        // Insert multiple entries
        for i in 1..=5 {
            let pixels = make_test_pixels(i, 32, 32);
            cache.insert(i, pixels)?;
        }
        
        assert_eq!(cache.pixels.len(), 5);
        assert!(cache.current_size_bytes > 0);
        
        cache.clear();
        
        assert_eq!(cache.pixels.len(), 0);
        assert_eq!(cache.current_size_bytes, 0);
        
        Ok(())
    }

    #[test]
    fn test_utilization_calculation() -> Result<()> {
        let mut cache = ContentCache::new(100); // 100MB
        let pixels = make_test_pixels(1, 1024, 1024); // ~4MB
        
        cache.insert(1, pixels)?;
        
        let utilization = cache.utilization();
        assert!(utilization > 0.0);
        assert!(utilization < 1.0);
        
        // Should be approximately 4MB / 100MB = 0.04
        assert!(utilization < 0.1);
        
        Ok(())
    }

    #[test]
    fn test_compact() -> Result<()> {
        let mut cache = ContentCache::new(10); // 10MB
        
        // Fill cache close to capacity
        for i in 1..=20 {
            let pixels = make_test_pixels(i, 256, 256); // ~256KB each
            cache.insert(i, pixels)?;
        }
        
        let initial_entries = cache.pixels.len();
        let initial_utilization = cache.utilization();
        
        // Compact to 50% utilization
        let evicted = cache.compact(0.5);
        
        assert!(evicted > 0);
        assert!(cache.pixels.len() < initial_entries);
        assert!(cache.utilization() < initial_utilization);
        
        Ok(())
    }

    #[test]
    fn test_cached_pixels_creation() {
        let pixels = make_test_pixels(12345, 64, 64);
        
        assert_eq!(pixels.cache_id, 12345);
        assert_eq!(pixels.width, 64);
        assert_eq!(pixels.height, 64);
        assert_eq!(pixels.pixels.len(), 64 * 64 * 4);
        assert!(pixels.memory_size() > pixels.pixels.len()); // Includes struct overhead
    }

    #[test]
    fn test_cached_pixels_touch() {
        let mut pixels = make_test_pixels(1, 32, 32);
        let original_time = pixels.last_used;
        
        std::thread::sleep(std::time::Duration::from_millis(1));
        pixels.touch();
        
        assert!(pixels.last_used > original_time);
    }

    #[test]
    fn test_unlimited_cache() -> Result<()> {
        let mut cache = ContentCache::new(0); // No limit
        
        // Insert many large entries
        for i in 1..=100 {
            let pixels = make_test_pixels(i, 256, 256);
            cache.insert(i, pixels)?;
        }
        
        assert_eq!(cache.pixels.len(), 100);
        assert_eq!(cache.stats().eviction_count, 0); // No evictions in unlimited cache
        
        Ok(())
    }

    #[test]
    fn test_stats_accuracy() -> Result<()> {
        let mut cache = ContentCache::new(100);
        let pixels = make_test_pixels(1, 64, 64);
        cache.insert(1, pixels)?;
        
        // Generate some hits and misses
        cache.lookup(1); // hit
        cache.lookup(1); // hit
        cache.lookup(2); // miss
        cache.lookup(3); // miss
        
        let stats = cache.stats();
        assert_eq!(stats.hit_count, 2);
        assert_eq!(stats.miss_count, 2);
        assert_eq!(stats.total_accesses(), 4);
        assert_eq!(stats.hit_rate, 0.5);
        assert_eq!(stats.entries, 1);
        
        Ok(())
    }
}