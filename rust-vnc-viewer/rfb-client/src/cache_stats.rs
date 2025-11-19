//! Cache protocol bandwidth statistics (client-side).
//!
//! Mirrors the semantics of the C++ BandwidthStats helper used by the
//! classic TigerVNC viewer so that end-of-run logs for ContentCache and
//! PersistentCache are directly comparable.

use rfb_protocol::messages::types::PixelFormat as ServerPixelFormat;
use rfb_protocol::messages::types::Rectangle;

/// Aggregate bandwidth statistics for a single cache protocol.
#[derive(Debug, Default, Clone, Copy)]
pub struct CacheProtocolStats {
    /// Bytes actually sent on the wire for reference messages
    /// (CachedRect / PersistentCachedRect).
    pub cached_rect_bytes: u64,
    pub cached_rect_count: u32,

    /// Bytes actually sent on the wire for init messages
    /// (CachedRectInit / PersistentCachedRectInit).
    pub cached_rect_init_bytes: u64,
    pub cached_rect_init_count: u32,

    /// Estimated bytes that would have been sent without the cache.
    pub alternative_bytes: u64,
}

impl CacheProtocolStats {
    /// Estimated bytes saved compared to the alternative baseline.
    pub fn bandwidth_saved(&self) -> u64 {
        let used = self.cached_rect_bytes + self.cached_rect_init_bytes;
        if self.alternative_bytes > used {
            self.alternative_bytes - used
        } else {
            0
        }
    }

    /// Estimated reduction percentage vs the alternative baseline.
    pub fn reduction_percentage(&self) -> f64 {
        let used = self.cached_rect_bytes + self.cached_rect_init_bytes;
        if self.alternative_bytes == 0 || used >= self.alternative_bytes {
            0.0
        } else {
            100.0 * (self.alternative_bytes - used) as f64 / self.alternative_bytes as f64
        }
    }

    /// Format a human-readable summary identical in spirit to the C++ viewer.
    pub fn format_summary(&self, label: &str) -> String {
        let saved = self.bandwidth_saved();
        let pct = self.reduction_percentage();
        format!(
            "{}: {} bandwidth saving ({:.1}% reduction)",
            label,
            human_bytes(saved),
            pct,
        )
    }
}

/// Conservative estimate of compressed size given uncompressed bytes.
fn estimate_compressed(uncompressed: u64) -> u64 {
    // Match the C++ helper: assume ~10:1 compression.
    uncompressed / 10
}

/// Track a ContentCache reference (CachedRect) operation.
///
/// Wire size: 20 bytes total (12-byte rect header + 8-byte cacheId).
pub fn track_content_cache_ref(stats: &mut CacheProtocolStats, rect: &Rectangle, pf: &ServerPixelFormat) {
    let bpp_bytes = (pf.bits_per_pixel / 8) as u64;
    let pixels = rect.width as u64 * rect.height as u64;
    let uncompressed = pixels * bpp_bytes;
    let ref_bytes = 20u64;
    let alt = 16u64 + estimate_compressed(uncompressed);

    stats.cached_rect_bytes = stats.cached_rect_bytes.saturating_add(ref_bytes);
    stats.alternative_bytes = stats.alternative_bytes.saturating_add(alt);
    stats.cached_rect_count = stats.cached_rect_count.saturating_add(1);
}

/// Track a ContentCache init (CachedRectInit) operation.
///
/// We treat `compressed_bytes` as the size of the encoded payload (excluding
/// the standard 12-byte rect header).
pub fn track_content_cache_init(stats: &mut CacheProtocolStats, compressed_bytes: u64) {
    // Overhead: 12 header + 8 cacheId + 4 encoding.
    let overhead = 24u64;
    stats.cached_rect_init_bytes = stats
        .cached_rect_init_bytes
        .saturating_add(overhead + compressed_bytes);
    // Baseline: 12 header + 4 encoding + compressed payload.
    stats.alternative_bytes = stats
        .alternative_bytes
        .saturating_add(16u64 + compressed_bytes);
    stats.cached_rect_init_count = stats.cached_rect_init_count.saturating_add(1);
}

/// Track a PersistentCache reference (PersistentCachedRect) operation.
///
/// Wire size: 12-byte header + 1-byte hashLen + hashLen bytes.
pub fn track_persistent_cache_ref(
    stats: &mut CacheProtocolStats,
    rect: &Rectangle,
    pf: &ServerPixelFormat,
    hash_len: u64,
) {
    let bpp_bytes = (pf.bits_per_pixel / 8) as u64;
    let pixels = rect.width as u64 * rect.height as u64;
    let uncompressed = pixels * bpp_bytes;
    let overhead = 12u64 + 1 + hash_len;
    let alt = 16u64 + estimate_compressed(uncompressed);

    stats.cached_rect_bytes = stats.cached_rect_bytes.saturating_add(overhead);
    stats.alternative_bytes = stats.alternative_bytes.saturating_add(alt);
    stats.cached_rect_count = stats.cached_rect_count.saturating_add(1);
}

/// Track a PersistentCache init (PersistentCachedRectInit) operation.
///
/// `compressed_bytes` should be the size of the encoded payload excluding
/// the 16-byte hash and 4-byte inner encoding fields.
pub fn track_persistent_cache_init(
    stats: &mut CacheProtocolStats,
    hash_len: u64,
    compressed_bytes: u64,
) {
    // Overhead: 12 header + 1-byte hashLen + hashLen bytes + 4-byte encoding.
    let overhead = 12u64 + 1 + hash_len + 4;
    stats.cached_rect_init_bytes = stats
        .cached_rect_init_bytes
        .saturating_add(overhead + compressed_bytes);
    // Baseline: 12 header + 4 encoding + compressed payload.
    stats.alternative_bytes = stats
        .alternative_bytes
        .saturating_add(16u64 + compressed_bytes);
    stats.cached_rect_init_count = stats.cached_rect_init_count.saturating_add(1);
}

/// Simple IEC-style byte formatter (bytes, KiB, MiB, GiB) mirroring core::iecPrefix.
fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GIB {
        format!("{:.3} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.3} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.3} KiB", b / KIB)
    } else {
        format!("{} B", bytes)
    }
}
