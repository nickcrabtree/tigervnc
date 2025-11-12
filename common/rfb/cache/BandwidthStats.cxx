#include <rfb/cache/BandwidthStats.h>
#include <core/string.h>

namespace rfb { namespace cache {

static inline size_t estimateCompressed(size_t uncompressedBytes) {
  // Conservative estimate; tune if better signals are available
  return uncompressedBytes / 10; // ~10:1
}

std::string CacheProtocolStats::formatSummary(const char* label) const {
  const uint64_t saved = bandwidthSaved();
  const double pct = reductionPercentage();
  return core::format("%s: %s bandwidth saving (%.1f%% reduction)",
                      label,
                      core::iecPrefix(saved, "B").c_str(),
                      pct);
}

void trackContentCacheRef(CacheProtocolStats& stats,
                          const core::Rect& r,
                          const rfb::PixelFormat& pf)
{
  (void)pf; // Not needed for ContentCache reference accounting
  // 20 bytes per ref (12 header + 8 cacheId)
  const size_t refBytes = 20;
  size_t alt = 16 + estimateCompressed(r.area() * (pf.bpp / 8));
  stats.cachedRectBytes += refBytes;
  stats.alternativeBytes += alt;
  stats.cachedRectCount++;
}

void trackContentCacheInit(CacheProtocolStats& stats,
                           size_t compressedBytes)
{
  const size_t overhead = 24; // 12 header + 8 cacheId + 4 encoding
  stats.cachedRectInitBytes += overhead + compressedBytes;
  stats.alternativeBytes += 16 + compressedBytes; // 12 header + 4 encoding + compressed
  stats.cachedRectInitCount++;
}

void trackPersistentCacheRef(CacheProtocolStats& stats,
                             const core::Rect& r,
                             const rfb::PixelFormat& pf,
                             size_t hashLen)
{
  const size_t overhead = 12 + 1 + hashLen; // rect header + hashLen + hash
  size_t alt = 16 + estimateCompressed(r.area() * (pf.bpp / 8));
  stats.cachedRectBytes += overhead;
  stats.alternativeBytes += alt;
  stats.cachedRectCount++;
}

void trackPersistentCacheInit(CacheProtocolStats& stats,
                              size_t hashLen,
                              size_t compressedBytes)
{
  const size_t overhead = 12 + 1 + hashLen + 4; // rect header + hashLen + hash + encoding
  stats.cachedRectInitBytes += overhead + compressedBytes;
  stats.alternativeBytes += 16 + compressedBytes; // baseline
  stats.cachedRectInitCount++;
}

}} // namespace rfb::cache
