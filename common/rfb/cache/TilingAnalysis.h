#pragma once

#include <vector>

#include <core/Rect.h>
#include <rfb/PixelBuffer.h>

namespace rfb { namespace cache {

// Simple classification for a tile with respect to a given cache protocol.
// For the tiling mask we only need to know whether the tile is a "hit"
// (can be served entirely from cache) or not, but InitCandidate is useful
// for future seeding policies.

enum class TileCacheState {
  NotCacheable = 0,
  Hit,
  InitCandidate
};

struct TileInfo {
  core::Rect rect;        // Pixel-space rectangle covered by the tile
  TileCacheState state;   // Cache classification for this tile
};

struct MaxRect {
  core::Rect rect;        // Pixel-space rectangle of the maximal region
  unsigned tilesWide;     // Width in tiles
  unsigned tilesHigh;     // Height in tiles
};

// Abstract interface that can be implemented by ContentCache and
// PersistentCache callers to classify tiles without exposing cache
// implementation details to the tiling layer.

class CacheQueryInterface {
public:
  virtual ~CacheQueryInterface() {}

  // Classify a tile rectangle for the current client connection.
  // Implementations may use ContentHash::computeRect, server-side
  // ContentCache / PersistentCache lookups, and per-connection
  // "knows" state to determine Hit vs InitCandidate vs NotCacheable.
  virtual TileCacheState classifyTile(const core::Rect& tileRect,
                                      const PixelBuffer* pb) = 0;
};

// Build a 2D grid of tiles over the given bounding rectangle. The grid
// is laid out row-major in the "tiles" vector: index = y * tilesX + x.
//
// - bounds: bounding rectangle to cover with tiles
// - tileSize: tile edge length in pixels (both width and height)
// - pb: framebuffer pixel buffer
// - cacheQuery: protocol-specific cache classifier
// - tiles: output tile descriptors
// - tilesX / tilesY: dimensions of the tile grid
//
// The function does not allocate per-tile hashes; callers can extend
// TileInfo if they need to retain extra data.

void buildTilingGrid(const core::Rect& bounds,
                     int tileSize,
                     const PixelBuffer* pb,
                     CacheQueryInterface& cacheQuery,
                     std::vector<TileInfo>& tiles,
                     int& tilesX,
                     int& tilesY);

// Find the single largest axis-aligned rectangle of Hit tiles in the
// given grid using a standard O(W*H) histogram-based maximal-rectangle
// algorithm. If no rectangle of at least minTiles area exists, outMax
// is left unchanged and the function returns false.
//
// - tiles: row-major tile array from buildTilingGrid
// - tilesX / tilesY: grid dimensions
// - minTiles: minimum tile area (e.g. 4 for a 2x2 region)
// - outMax: on success, receives the maximal Hit rectangle

bool findLargestHitRectangle(const std::vector<TileInfo>& tiles,
                             int tilesX,
                             int tilesY,
                             int minTiles,
                             MaxRect& outMax);

}} // namespace rfb::cache
