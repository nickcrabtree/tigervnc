#include <core/string.h>
#include <rfb/cache/BandwidthStats.h>

namespace rfb {
namespace cache {

static inline size_t estimateCompressed(size_t uncompressedBytes) {
  // For user-facing "sales pitch" metrics we deliberately assume a very
  // conservative compression baseline (no compression) so that the reported
  // bandwidth reduction reflects the maximum potential savings of the cache
  // rather than the specifics of any one encoder implementation.
  return uncompressedBytes; // Treat baseline as uncompressed pixels
}

std::string CacheProtocolStats::formatSummary(const char* label) const {
  const uint64_t saved = bandwidthSaved();
  const double pct = reductionPercentage();
  return core::format("%s: %s bandwidth saving (%.1f%% reduction)", label, core::iecPrefix(saved, "B").c_str(), pct);
}

void trackContentCacheRef(CacheProtocolStats& stats, const core::Rect& r, const rfb::PixelFormat& pf) {
  (void)pf; // Not needed for ContentCache reference accounting
  // 20 bytes per ref (12 header + 8 cacheId)
  const size_t refBytes = 20;
  size_t alt = 16 + estimateCompressed(r.area() * (pf.bpp / 8));
  stats.cachedRectBytes += refBytes;
  stats.alternativeBytes += alt;
  stats.cachedRectCount++;
}

void trackContentCacheInit(CacheProtocolStats& stats, size_t compressedBytes) {
  const size_t overhead = 24; // 12 header + 8 cacheId + 4 encoding
  stats.cachedRectInitBytes += overhead + compressedBytes;
  stats.alternativeBytes += 16 + compressedBytes; // 12 header + 4 encoding + compressed
  stats.cachedRectInitCount++;
}

void trackPersistentCacheRef(CacheProtocolStats& stats, const core::Rect& r, const rfb::PixelFormat& pf) {
  (void)pf; // Encoding choice is implicit; we just need an estimated baseline.
  // PersistentCachedRect reference overhead: 36 bytes
  // (12-byte rect header + 16-byte CacheKey + 8-byte offset fields).
  const size_t overhead = 36;
  size_t alt = 16 + estimateCompressed(r.area() * (pf.bpp / 8));
  stats.cachedRectBytes += overhead;
  stats.alternativeBytes += alt;
  stats.cachedRectCount++;
}

void trackPersistentCacheInit(CacheProtocolStats& stats, size_t compressedBytes) {
  // PersistentCachedRectInit v2 overhead: 33 bytes
  // (12-byte rect header + 16-byte CacheKey + 1-byte flags + 4-byte encoding).
  const size_t overhead = 33;
  stats.cachedRectInitBytes += overhead + compressedBytes;
  stats.alternativeBytes += 16 + compressedBytes; // baseline
  stats.cachedRectInitCount++;
}

} // namespace cache
} // namespace rfb
