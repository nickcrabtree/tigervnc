#include <rfb/cache/TilingAnalysis.h>

#include <algorithm>

#include <core/LogWriter.h>

namespace rfb { namespace cache {

static core::LogWriter vlog("TilingAnalysis");

void buildTilingGrid(const core::Rect& bounds,
                     int tileSize,
                     const PixelBuffer* pb,
                     CacheQueryInterface& cacheQuery,
                     std::vector<TileInfo>& tiles,
                     int& tilesX,
                     int& tilesY)
{
  tiles.clear();
  tilesX = tilesY = 0;

  if (pb == nullptr || bounds.is_empty() || tileSize <= 0)
    return;

  const int width = bounds.width();
  const int height = bounds.height();

  tilesX = (width + tileSize - 1) / tileSize;
  tilesY = (height + tileSize - 1) / tileSize;

  tiles.resize(static_cast<size_t>(tilesX * tilesY));

  for (int ty = 0; ty < tilesY; ++ty) {
    for (int tx = 0; tx < tilesX; ++tx) {
      const int x1 = bounds.tl.x + tx * tileSize;
      const int y1 = bounds.tl.y + ty * tileSize;
      int x2 = x1 + tileSize;
      int y2 = y1 + tileSize;

      if (x2 > bounds.br.x)
        x2 = bounds.br.x;
      if (y2 > bounds.br.y)
        y2 = bounds.br.y;

      core::Rect tileRect(x1, y1, x2, y2);
      TileInfo info;
      info.rect = tileRect;
      info.state = cacheQuery.classifyTile(tileRect, pb);

      tiles[static_cast<size_t>(ty * tilesX + tx)] = info;
    }
  }
}

bool findLargestHitRectangle(const std::vector<TileInfo>& tiles,
                             int tilesX,
                             int tilesY,
                             int minTiles,
                             MaxRect& outMax)
{
  if (tiles.empty() || tilesX <= 0 || tilesY <= 0)
    return false;

  if (minTiles <= 0)
    minTiles = 1;

  std::vector<int> heights(static_cast<size_t>(tilesX), 0);

  int bestArea = 0;
  int bestX0 = 0, bestY0 = 0, bestX1 = 0, bestY1 = 0;

  for (int y = 0; y < tilesY; ++y) {
    // Update histogram heights for this row
    for (int x = 0; x < tilesX; ++x) {
      const TileInfo& t = tiles[static_cast<size_t>(y * tilesX + x)];
      if (t.state == TileCacheState::Hit)
        heights[static_cast<size_t>(x)] += 1;
      else
        heights[static_cast<size_t>(x)] = 0;
    }

    // Standard largest-rectangle-in-histogram using a monotonic stack
    std::vector<int> stack;
    for (int x = 0; x <= tilesX; ++x) {
      int curHeight = (x == tilesX) ? 0 : heights[static_cast<size_t>(x)];
      while (!stack.empty() && curHeight < heights[static_cast<size_t>(stack.back())]) {
        int h = heights[static_cast<size_t>(stack.back())];
        stack.pop_back();
        int left = stack.empty() ? 0 : (stack.back() + 1);
        int right = x - 1;
        int width = right - left + 1;
        int area = h * width;
        if (area > bestArea && area >= minTiles) {
          bestArea = area;
          bestX0 = left;
          bestX1 = right;
          bestY1 = y;
          bestY0 = y - h + 1;
        }
      }
      stack.push_back(x);
    }
  }

  if (bestArea < minTiles)
    return false;

  // Convert best tile rectangle to pixel-space using the stored TileInfo.
  const TileInfo& topLeft = tiles[static_cast<size_t>(bestY0 * tilesX + bestX0)];
  const TileInfo& bottomRight = tiles[static_cast<size_t>(bestY1 * tilesX + bestX1)];

  MaxRect mr;
  mr.rect.tl = topLeft.rect.tl;
  mr.rect.br = bottomRight.rect.br;
  mr.tilesWide = static_cast<unsigned>(bestX1 - bestX0 + 1);
  mr.tilesHigh = static_cast<unsigned>(bestY1 - bestY0 + 1);

  outMax = mr;
  return true;
}

}} // namespace rfb::cache
