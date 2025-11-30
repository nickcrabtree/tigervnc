#include <rfb/cache/TilingIntegration.h>

#include <vector>

#include <core/LogWriter.h>
#include <core/Region.h>
#include <rfb/ServerCore.h>

namespace rfb { namespace cache {

static core::LogWriter vlog("TilingIntegration");

namespace {

// Shared helper: compute a width/height/hash key for a tile using the
// same scheme as ContentCache (16-byte MD5 -> first 8 bytes as uint64).
struct TileKey {
  uint16_t width = 0;
  uint16_t height = 0;
  uint64_t hash = 0;
};

inline TileKey computeTileKey(const core::Rect& tileRect,
                              const PixelBuffer* pb)
{
  TileKey k;
  if (!pb || tileRect.is_empty())
    return k;

  k.width = static_cast<uint16_t>(tileRect.width());
  k.height = static_cast<uint16_t>(tileRect.height());

  std::vector<uint8_t> fullHash = ContentHash::computeRect(pb, tileRect);
  if (!fullHash.empty() && fullHash.size() >= 8)
    memcpy(&k.hash, fullHash.data(), 8);

  return k;
}

} // anonymous namespace

TileCacheState PersistentCacheQuery::classifyTile(const core::Rect& tileRect,
                                                  const PixelBuffer* pb)
{
  if (!conn_ || !pb)
    return TileCacheState::NotCacheable;

  // Skip very small tiles to avoid overhead.
  if (tileRect.area() < Server::persistentCacheMinRectSize)
    return TileCacheState::NotCacheable;

  // Use the same width/height/hash scheme as ContentCache for tiling
  // analysis, but delegate hit knowledge to the persistent ID index.
  TileKey keyInfo = computeTileKey(tileRect, pb);
  if (keyInfo.hash == 0 || keyInfo.width == 0 || keyInfo.height == 0)
    return TileCacheState::NotCacheable;

  // With the 64-bit PersistentCache protocol, we track client knowledge
  // by 64-bit ID rather than full hash vectors.
  if (conn_->knowsPersistentId(keyInfo.hash))
    return TileCacheState::Hit;

  // Unknown to this client but a valid tile: treat as Init candidate.
  return TileCacheState::InitCandidate;
}

void analyzeRegionTilingLogOnly(const core::Region& region,
                                int tileSize,
                                int minTiles,
                                const PixelBuffer* pb,
                                CacheQueryInterface& cacheQuery)
{
  if (!pb || region.is_empty() || tileSize <= 0)
    return;

  core::Rect bounds = region.get_bounding_rect();

  std::vector<TileInfo> tiles;
  int tilesX = 0, tilesY = 0;
  buildTilingGrid(bounds, tileSize, pb, cacheQuery, tiles, tilesX, tilesY);

  if (tiles.empty() || tilesX <= 0 || tilesY <= 0)
    return;

  int hitCount = 0;
  int initCandCount = 0;
  for (const auto& t : tiles) {
    if (t.state == TileCacheState::Hit)
      ++hitCount;
    else if (t.state == TileCacheState::InitCandidate)
      ++initCandCount;
  }

  vlog.info("Tiling analysis: bounds=[%d,%d-%d,%d] tiles=%dx%d hits=%d init=%d",
            bounds.tl.x, bounds.tl.y, bounds.br.x, bounds.br.y,
            tilesX, tilesY, hitCount, initCandCount);

  MaxRect maxRect;
  if (findLargestHitRectangle(tiles, tilesX, tilesY, minTiles, maxRect)) {
    vlog.info("Tiling analysis: largest HIT rect [%d,%d-%d,%d] tiles=%ux%u",
              maxRect.rect.tl.x, maxRect.rect.tl.y,
              maxRect.rect.br.x, maxRect.rect.br.y,
              maxRect.tilesWide, maxRect.tilesHigh);
  } else {
    vlog.info("Tiling analysis: no HIT rectangle >= %d tiles", minTiles);
  }
}

}} // namespace rfb::cache
