#pragma once

#include <string>
#include <cstdint>
#include <rfb/PixelFormat.h>
#include <core/Rect.h>

namespace rfb { namespace cache {

struct CacheProtocolStats {
  // With cache
  uint64_t cachedRectBytes = 0;      // Reference messages
  uint32_t cachedRectCount = 0;
  uint64_t cachedRectInitBytes = 0;  // Init messages
  uint32_t cachedRectInitCount = 0;

  // Without cache (estimated baseline)
  uint64_t alternativeBytes = 0;

  // Computed helpers
  uint64_t bandwidthSaved() const {
    if (alternativeBytes > (cachedRectBytes + cachedRectInitBytes))
      return alternativeBytes - (cachedRectBytes + cachedRectInitBytes);
    return 0;
  }
  double reductionPercentage() const {
    uint64_t used = cachedRectBytes + cachedRectInitBytes;
    if (alternativeBytes == 0 || used >= alternativeBytes) return 0.0;
    return 100.0 * (double)(alternativeBytes - used) / (double)alternativeBytes;
  }
  std::string formatSummary(const char* label = "Cache") const;
};

// ContentCache: IDs are uint64_t. Reference 20 bytes total per rect.
void trackContentCacheRef(CacheProtocolStats& stats,
                          const core::Rect& r,
                          const rfb::PixelFormat& pf);

void trackContentCacheInit(CacheProtocolStats& stats,
                           size_t compressedBytes);

// PersistentCache: now uses the same 64-bit ID on the wire as
// ContentCache. Reference overhead is therefore identical to
// CachedRect: 20 bytes per rect.
void trackPersistentCacheRef(CacheProtocolStats& stats,
                             const core::Rect& r,
                             const rfb::PixelFormat& pf);

// PersistentCachedRectInit overhead: 24 bytes (rect header + ID + encoding)
void trackPersistentCacheInit(CacheProtocolStats& stats,
                              size_t compressedBytes);

}} // namespace rfb::cache
