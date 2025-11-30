#pragma once

#include <rfb/cache/TilingAnalysis.h>
#include <rfb/ContentHash.h>
#include <rfb/SConnection.h>

namespace rfb { namespace cache {

// Concrete CacheQueryInterface implementation for PersistentCache. The
// unified cache engine exposes a single 64-bit ID space, so tiling
// analysis only needs to consult the PersistentCache session state on
// the server side.
class PersistentCacheQuery : public CacheQueryInterface {
public:
  PersistentCacheQuery(SConnection* conn)
    : conn_(conn) {}

  TileCacheState classifyTile(const core::Rect& tileRect,
                              const PixelBuffer* pb) override;

private:
  SConnection* conn_;
};

// Log-only helper function to analyze a large dirty region and report
// what cacheable rectangles could be found, without actually changing
// encoding behavior. This is intended for tuning thresholds and observing
// the tiling pass before switching to production use.
//
// - region: dirty region to analyze
// - tileSize: tile granularity (e.g. 128)
// - minTiles: minimum area in tiles to consider (e.g. 4 for 2x2)
// - pb: framebuffer pixel buffer
// - cacheQuery: cache classifier (ContentCache or PersistentCache)
//
// Logs via "TilingIntegration" LogWriter with details about how many
// tiles are hits, how many maximal rectangles were found, etc.

void analyzeRegionTilingLogOnly(const core::Region& region,
                                int tileSize,
                                int minTiles,
                                const PixelBuffer* pb,
                                CacheQueryInterface& cacheQuery);

}} // namespace rfb::cache
