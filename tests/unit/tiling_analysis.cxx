/* TilingAnalysis unit tests for ContentCache/PersistentCache tiling core.
 *
 * These tests exercise buildTilingGrid() and findLargestHitRectangle()
 * using a synthetic CacheQueryInterface implementation that does not
 * depend on real ContentCache/PersistentCache state.
 */

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <rfb/cache/TilingAnalysis.h>

using namespace rfb::cache;

namespace {

// Dummy PixelBuffer implementation used only to satisfy the
// CacheQueryInterface signature. The tests below never read from
// the PixelBuffer, so getBuffer() is never called.
class DummyPixelBuffer : public rfb::PixelBuffer {
public:
  DummyPixelBuffer()
    : rfb::PixelBuffer(rfb::PixelFormat(), /*width*/ 256, /*height*/ 256) {}

  const uint8_t* getBuffer(const core::Rect& /*r*/, int* stride) const override {
    if (stride)
      *stride = 0;
    return nullptr;
  }
};

// CacheQuery that classifies tiles based on their position within the
// grid rather than actual pixel content. This allows deterministic
// masks without touching pixel data.
class MaskCacheQuery : public CacheQueryInterface {
public:
  MaskCacheQuery(int tilesX, int tilesY)
    : tilesX_(tilesX), tilesY_(tilesY)
  {
    mask_.assign(static_cast<size_t>(tilesX_ * tilesY_), TileCacheState::NotCacheable);
  }

  // Set state for tile at (tx, ty)
  void setTileState(int tx, int ty, TileCacheState state)
  {
    mask_[index(tx, ty)] = state;
  }

  TileCacheState classifyTile(const core::Rect& /*tileRect*/,
                              const rfb::PixelBuffer* /*pb*/) override
  {
    // Map tileRect back to (tx, ty) by assuming tiles are laid out
    // left-to-right, top-to-bottom in row-major order, and that
    // buildTilingGrid walks tiles in that exact order.
    //
    // We don't compute tx/ty from coordinates here; instead we
    // advance an internal cursor each time classifyTile() is called.
    if (curIndex_ >= mask_.size())
      return TileCacheState::NotCacheable;
    return mask_[curIndex_++];
  }

  void reset() { curIndex_ = 0; }

private:
  size_t index(int tx, int ty) const {
    return static_cast<size_t>(ty * tilesX_ + tx);
  }

  int tilesX_;
  int tilesY_;
  std::vector<TileCacheState> mask_;
  size_t curIndex_ = 0;
};

} // anonymous namespace

// ============================================================================
// buildTilingGrid
// ============================================================================

TEST(TilingAnalysis, BuildGridDimensions)
{
  core::Rect bounds(0, 0, 100, 50); // 100x50
  int tileSize = 16;

  DummyPixelBuffer pb;
  MaskCacheQuery query(/*tilesX*/ (100 + tileSize - 1) / tileSize,
                       /*tilesY*/ (50 + tileSize - 1) / tileSize);

  std::vector<TileInfo> tiles;
  int tilesX = 0, tilesY = 0;

  buildTilingGrid(bounds, tileSize, &pb, query, tiles, tilesX, tilesY);

  EXPECT_EQ(tilesX, (100 + tileSize - 1) / tileSize);
  EXPECT_EQ(tilesY, (50 + tileSize - 1) / tileSize);
  ASSERT_EQ(static_cast<size_t>(tilesX * tilesY), tiles.size());

  // All tile rects should be non-empty and inside bounds.
  for (const auto& t : tiles) {
    EXPECT_FALSE(t.rect.is_empty());
    EXPECT_TRUE(t.rect.enclosed_by(bounds));
  }
}

// ============================================================================
// findLargestHitRectangle
// ============================================================================

TEST(TilingAnalysis, LargestHitRectFullGrid)
{
  core::Rect bounds(0, 0, 64, 64); // 4x4 tiles of 16x16
  int tileSize = 16;

  DummyPixelBuffer pb;
  const int tilesXExpected = 4;
  const int tilesYExpected = 4;
  MaskCacheQuery query(tilesXExpected, tilesYExpected);

  // Mark every tile as Hit
  for (int ty = 0; ty < tilesYExpected; ++ty) {
    for (int tx = 0; tx < tilesXExpected; ++tx) {
      query.setTileState(tx, ty, TileCacheState::Hit);
    }
  }
  query.reset();

  std::vector<TileInfo> tiles;
  int tilesX = 0, tilesY = 0;
  buildTilingGrid(bounds, tileSize, &pb, query, tiles, tilesX, tilesY);

  ASSERT_EQ(tilesXExpected, tilesX);
  ASSERT_EQ(tilesYExpected, tilesY);

  MaxRect mr;
  bool found = findLargestHitRectangle(tiles, tilesX, tilesY,
                                       /*minTiles*/ 1, mr);
  ASSERT_TRUE(found);
  EXPECT_EQ(mr.rect, bounds);
  EXPECT_EQ(mr.tilesWide, static_cast<unsigned>(tilesXExpected));
  EXPECT_EQ(mr.tilesHigh, static_cast<unsigned>(tilesYExpected));
}

TEST(TilingAnalysis, LargestHitRectThreshold)
{
  core::Rect bounds(0, 0, 64, 64); // 4x4 grid
  int tileSize = 16;

  DummyPixelBuffer pb;
  MaskCacheQuery query(4, 4);

  // Single 1x1 Hit at (1,1)
  for (int ty = 0; ty < 4; ++ty) {
    for (int tx = 0; tx < 4; ++tx) {
      TileCacheState state = (tx == 1 && ty == 1) ?
          TileCacheState::Hit : TileCacheState::NotCacheable;
      query.setTileState(tx, ty, state);
    }
  }
  query.reset();

  std::vector<TileInfo> tiles;
  int tilesX = 0, tilesY = 0;
  buildTilingGrid(bounds, tileSize, &pb, query, tiles, tilesX, tilesY);

  MaxRect mr;
  // With minTiles=1 we should find the single-tile rect
  bool found = findLargestHitRectangle(tiles, tilesX, tilesY,
                                       /*minTiles*/ 1, mr);
  ASSERT_TRUE(found);
  EXPECT_EQ(mr.tilesWide, 1u);
  EXPECT_EQ(mr.tilesHigh, 1u);

  // With minTiles=2, there is no rectangle of area >= 2
  MaxRect mr2;
  bool found2 = findLargestHitRectangle(tiles, tilesX, tilesY,
                                        /*minTiles*/ 2, mr2);
  EXPECT_FALSE(found2);
}

TEST(TilingAnalysis, LargestHitRectDisjointRegions)
{
  core::Rect bounds(0, 0, 64, 64); // 4x4
  int tileSize = 16;

  DummyPixelBuffer pb;
  MaskCacheQuery query(4, 4);

  // Two disjoint 2x2 Hit blocks:
  // - Block A: tiles (0,0)-(1,1)
  // - Block B: tiles (2,2)-(3,3)
  for (int ty = 0; ty < 4; ++ty) {
    for (int tx = 0; tx < 4; ++tx) {
      bool inA = (tx < 2 && ty < 2);
      bool inB = (tx >= 2 && ty >= 2);
      TileCacheState state = (inA || inB) ? TileCacheState::Hit
                                          : TileCacheState::NotCacheable;
      query.setTileState(tx, ty, state);
    }
  }
  query.reset();

  std::vector<TileInfo> tiles;
  int tilesX = 0, tilesY = 0;
  buildTilingGrid(bounds, tileSize, &pb, query, tiles, tilesX, tilesY);

  MaxRect mr;
  bool found = findLargestHitRectangle(tiles, tilesX, tilesY,
                                       /*minTiles*/ 1, mr);
  ASSERT_TRUE(found);

  // Each block is 2x2 tiles; either is acceptable as "largest".
  EXPECT_EQ(mr.tilesWide * mr.tilesHigh, 4u);
  EXPECT_TRUE(mr.rect.enclosed_by(bounds));
}
