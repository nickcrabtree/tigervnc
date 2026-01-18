/* Unit tests for shift-tolerant cache scan (ShiftTolerantScan).
 *
 * These tests validate:
 *  - packing correctness (tile hits packed into a verified rectangle)
 *  - phase recovery under translation (content reappears at shifted coords)
 *  - deterministic budget enforcement via maxBlocks
 */

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <core/Region.h>
#include <rfb/CacheKey.h>
#include <rfb/ContentHash.h>
#include <rfb/PixelBuffer.h>
#include <rfb/cache/ShiftTolerantScan.h>

#include <cstring>
#include <set>
#include <vector>

using rfb::CacheKey;
using rfb::ContentHash;
using rfb::ManagedPixelBuffer;
using rfb::PixelFormat;
using rfb::cache::KeyedRect;
using rfb::cache::PhaseSet;
using rfb::cache::ScanConfig;
using rfb::cache::ScanStats;
using rfb::cache::ShiftTolerantScanner;

namespace {

static PixelFormat testPF() {
  return PixelFormat(32, 24, false, true, 255, 255, 255, 16, 8, 0);
}

static CacheKey cacheKeyFromHash(const std::vector<uint8_t>& hash) {
  // CacheKey expects at least 16 bytes; ContentHash always returns 16.
  if (hash.size() < 16)
    return CacheKey();
  return CacheKey(hash.data());
}

static std::string keyBytes(const CacheKey& k) {
  return std::string(reinterpret_cast<const char*>(k.bytes.data()), k.bytes.size());
}

static void fillPattern(ManagedPixelBuffer& pb, int seed) {
  PixelFormat pf = pb.getPF();
  core::Rect r = pb.getRect();

  const int w = r.width();
  const int h = r.height();
  std::vector<uint32_t> pixels;
  pixels.resize(static_cast<size_t>(w) * static_cast<size_t>(h));

  for (int y = 0; y < h; ++y) {
    for (int x = 0; x < w; ++x) {
      // Deterministic but non-trivial pattern.
      uint8_t rr = static_cast<uint8_t>((x + seed) & 0xff);
      uint8_t gg = static_cast<uint8_t>((y + seed * 3) & 0xff);
      uint8_t bb = static_cast<uint8_t>((x ^ y ^ seed) & 0xff);
      pixels[static_cast<size_t>(y) * w + x] =
          (static_cast<uint32_t>(rr) << 16) | (static_cast<uint32_t>(gg) << 8) | static_cast<uint32_t>(bb);
    }
  }

  pb.imageRect(pf, r, pixels.data(), w);
}

static ManagedPixelBuffer makeShifted(const ManagedPixelBuffer& src, int dx, int dy) {
  PixelFormat pf = src.getPF();
  ManagedPixelBuffer dst(pf, src.width(), src.height());

  // Fill background with a constant.
  std::vector<uint32_t> bg;
  bg.resize(static_cast<size_t>(dst.width()) * static_cast<size_t>(dst.height()), 0u);
  dst.imageRect(pf, dst.getRect(), bg.data(), dst.width());

  // Copy pixels from src -> dst with offset (dx,dy).
  const int w = src.width();
  const int h = src.height();
  std::vector<uint32_t> pixels;
  pixels.resize(static_cast<size_t>(w) * static_cast<size_t>(h));

  // Read src in its native PF (same as pf).
  src.getImage(pf, pixels.data(), src.getRect(), w);

  // Write shifted into dst.
  for (int y = 0; y < h; ++y) {
    for (int x = 0; x < w; ++x) {
      const int nx = x + dx;
      const int ny = y + dy;
      if (nx < 0 || ny < 0 || nx >= w || ny >= h)
        continue;
      const uint32_t v = pixels[static_cast<size_t>(y) * w + x];
      bg[static_cast<size_t>(ny) * w + nx] = v;
    }
  }

  dst.imageRect(pf, dst.getRect(), bg.data(), w);
  return dst;
}

} // anonymous namespace

TEST(ShiftTolerantScan, PackAndVerifyBasic) {
  PixelFormat pf = testPF();
  ManagedPixelBuffer pb(pf, 64, 64);
  fillPattern(pb, 7);

  // Configure a single tile size for deterministic packing.
  ScanConfig cfg;
  cfg.tileSizes = {16};
  cfg.phaseSet = PhaseSet::Quarter;
  cfg.padPixels = 0;
  cfg.budgetUs = 0;
  cfg.maxBlocks = 0;
  cfg.minPackedArea = 0;
  cfg.coverageThresholdPermille = 0;
  cfg.preferLargestFirst = true;
  cfg.logStats = false;

  // We expect a 2x2 tile packed rectangle at (16,16)-(48,48).
  const core::Rect packedRect(16, 16, 48, 48);

  // Prepare a client-known set that includes the packed rect key.
  std::set<std::string> known;
  known.insert(keyBytes(cacheKeyFromHash(ContentHash::computeRect(&pb, packedRect))));

  // Also allow the four constituent tiles to be hits.
  const int T = 16;
  for (int ty = 16; ty < 48; ty += T) {
    for (int tx = 16; tx < 48; tx += T) {
      core::Rect tile(tx, ty, tx + T, ty + T);
      known.insert(keyBytes(cacheKeyFromHash(ContentHash::computeRect(&pb, tile))));
    }
  }

  auto clientKnows = [&](const CacheKey& key) -> bool { return known.find(keyBytes(key)) != known.end(); };

  ShiftTolerantScanner scanner(cfg);
  ScanStats stats;

  core::Region damage(packedRect);
  std::vector<KeyedRect> hits = scanner.scanAndPack(damage, &pb, nullptr, clientKnows, &stats);

  ASSERT_EQ(hits.size(), 1u);
  EXPECT_EQ(hits[0].rect, packedRect);
  EXPECT_GE(stats.blocksHashed, 1u);
  EXPECT_EQ(stats.rectHitsEmitted, 1u);
}

TEST(ShiftTolerantScan, PhaseRecoversShiftedContent) {
  PixelFormat pf = testPF();
  ManagedPixelBuffer base(pf, 64, 64);
  fillPattern(base, 42);

  // Shift by half-tile so Minimal phases should recover (T/2,T/2).
  const int dx = 8;
  const int dy = 8;
  ManagedPixelBuffer shifted = makeShifted(base, dx, dy);

  ScanConfig cfg;
  cfg.tileSizes = {16};
  cfg.phaseSet = PhaseSet::Minimal;
  cfg.padPixels = 0;
  cfg.budgetUs = 0;
  cfg.maxBlocks = 0;
  cfg.minPackedArea = 0;
  cfg.coverageThresholdPermille = 0;
  cfg.preferLargestFirst = true;
  cfg.logStats = false;

  // A 2x2 tile rectangle in base at (0,0)-(32,32) reappears at (8,8)-(40,40).
  const core::Rect baseRect(0, 0, 32, 32);
  const core::Rect shiftedRect(8, 8, 40, 40);

  std::set<std::string> known;
  known.insert(keyBytes(cacheKeyFromHash(ContentHash::computeRect(&base, baseRect))));

  const int T = 16;
  for (int ty = 0; ty < 32; ty += T) {
    for (int tx = 0; tx < 32; tx += T) {
      core::Rect tile(tx, ty, tx + T, ty + T);
      known.insert(keyBytes(cacheKeyFromHash(ContentHash::computeRect(&base, tile))));
    }
  }

  auto clientKnows = [&](const CacheKey& key) -> bool { return known.find(keyBytes(key)) != known.end(); };

  ShiftTolerantScanner scanner(cfg);
  ScanStats stats;

  core::Region damage(shiftedRect);
  std::vector<KeyedRect> hits = scanner.scanAndPack(damage, &shifted, nullptr, clientKnows, &stats);

  ASSERT_FALSE(hits.empty());

  bool found = false;
  for (const auto& kr : hits) {
    if (kr.rect == shiftedRect)
      found = true;
  }
  EXPECT_TRUE(found);
  EXPECT_EQ(stats.rectHitsEmitted, hits.size());
}

TEST(ShiftTolerantScan, RespectsMaxBlocksBudget) {
  PixelFormat pf = testPF();
  ManagedPixelBuffer pb(pf, 64, 64);
  fillPattern(pb, 3);

  ScanConfig cfg;
  cfg.tileSizes = {16};
  cfg.phaseSet = PhaseSet::Minimal;
  cfg.padPixels = 0;
  cfg.budgetUs = 0;
  cfg.maxBlocks = 1; // deterministic budget cap
  cfg.minPackedArea = 0;
  cfg.coverageThresholdPermille = 0;
  cfg.preferLargestFirst = true;
  cfg.logStats = false;

  auto clientKnows = [&](const CacheKey& /*key*/) -> bool {
    // Accept all keys.
    return true;
  };

  ShiftTolerantScanner scanner(cfg);
  ScanStats stats;

  core::Region damage(core::Rect(0, 0, 64, 64));
  (void)scanner.scanAndPack(damage, &pb, nullptr, clientKnows, &stats);

  EXPECT_LE(stats.blocksHashed, 1u);
}
