/* Copyright (C) 2026 TigerVNC Team. All Rights Reserved.
 *
 * This is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This software is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this software; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA 02111-1307,
 * USA.
 */

#include <rfb/cache/ShiftTolerantScan.h>

#include <rfb/ContentHash.h>
#include <rfb/cache/VolatilityMap.h>

#include <algorithm>
#include <sys/time.h>

namespace {

inline uint64_t nowUs() {
  struct timeval tv;
  gettimeofday(&tv, nullptr);
  return (uint64_t)tv.tv_sec * 1000000ULL + (uint64_t)tv.tv_usec;
}

inline rfb::CacheKey cacheKeyFromHash(const std::vector<uint8_t>& hash) {
  if (hash.size() < 16)
    return rfb::CacheKey();
  return rfb::CacheKey(hash.data());
}

static const int WholeRectCacheMinArea = 10000;

} // anonymous namespace

namespace rfb {
namespace cache {

ShiftTolerantScanner::ShiftTolerantScanner(const ScanConfig& cfg) : cfg_(cfg) {}

static inline void phasesFor(int T, PhaseSet set, std::vector<ScanPhase>* out) {
  out->clear();
  if (T <= 0)
    return;

  if (set == PhaseSet::Minimal) {
    out->push_back({0, 0});
    out->push_back({T / 2, T / 2});
    return;
  }

  const int q = T / 4;
  out->push_back({0, 0});
  out->push_back({q, 0});
  out->push_back({0, q});
  out->push_back({q, q});
}

static inline void snapStart(const core::Rect& scan, int T, int ox, int oy, int* x0, int* y0) {
  int sx = scan.tl.x - ((scan.tl.x - ox) % T);
  int sy = scan.tl.y - ((scan.tl.y - oy) % T);
  if (sx > scan.tl.x)
    sx -= T;
  if (sy > scan.tl.y)
    sy -= T;
  *x0 = sx;
  *y0 = sy;
}

static inline bool cellIsVolatile(const VolatilityMap* vol, int x, int y) {
  if (!vol)
    return false;
  return vol->isVolatileXY(x, y);
}

static inline int coveragePermille(const core::Region& damage, const core::Rect& rect) {
  if (rect.is_empty())
    return 0;

  core::Region in = damage.intersect(rect);
  if (in.is_empty())
    return 0;

  core::Rect bbox = in.get_bounding_rect();
  const int da = bbox.area();
  const int ra = rect.area();
  if (ra <= 0)
    return 0;

  int p = (int)((1000LL * (long long)da) / (long long)ra);
  if (p < 0)
    p = 0;
  if (p > 1000)
    p = 1000;
  return p;
}

std::vector<KeyedRect> ShiftTolerantScanner::scanAndPackImpl(const core::Region& damage, const PixelBuffer* pb,
                                                             VolatilityMap* vol,
                                                             const std::function<bool(const CacheKey&)>& clientKnows,
                                                             ScanStats* outStats) {
  ScanStats localStats;
  std::vector<KeyedRect> out;

  if (!pb || damage.is_empty()) {
    if (outStats)
      *outStats = localStats;
    return out;
  }

  const core::Rect fbRect = pb->getRect();
  const core::Rect bbox = damage.get_bounding_rect();
  core::Rect scan(bbox.tl.x - cfg_.padPixels, bbox.tl.y - cfg_.padPixels, bbox.br.x + cfg_.padPixels,
                  bbox.br.y + cfg_.padPixels);
  scan = scan.intersect(fbRect);
  if (scan.is_empty()) {
    if (outStats)
      *outStats = localStats;
    return out;
  }

  const uint64_t t0 = nowUs();

  std::vector<int> tileSizes = cfg_.tileSizes;
  if (tileSizes.empty())
    tileSizes.push_back(128);

  for (int T : tileSizes) {
    if (T <= 0)
      continue;

    std::vector<ScanPhase> phases;
    phasesFor(T, cfg_.phaseSet, &phases);

    for (const auto& ph : phases) {
      int x0 = 0, y0 = 0;
      snapStart(scan, T, ph.ox, ph.oy, &x0, &y0);

      const int gridW = (scan.br.x - x0 + T - 1) / T;
      const int gridH = (scan.br.y - y0 + T - 1) / T;
      if (gridW <= 0 || gridH <= 0)
        continue;

      std::vector<uint8_t> hitMask((size_t)gridW * (size_t)gridH, 0);
      auto idx = [gridW](int gx, int gy) { return gy * gridW + gx; };

      for (int gy = 0; gy < gridH; ++gy) {
        for (int gx = 0; gx < gridW; ++gx) {
          const uint64_t now = nowUs();
          const int elapsedUs = (int)(now - t0);
          if ((cfg_.budgetUs > 0 && elapsedUs >= cfg_.budgetUs) ||
              (cfg_.maxBlocks > 0 && (int)localStats.blocksHashed >= cfg_.maxBlocks)) {
            goto done;
          }

          core::Rect r(x0 + gx * T, y0 + gy * T, x0 + (gx + 1) * T, y0 + (gy + 1) * T);
          r = r.intersect(fbRect);
          if (r.is_empty())
            continue;

          ++localStats.blocksConsidered;

          if (damage.intersect(r).is_empty())
            continue;

          if (cellIsVolatile(vol, r.tl.x, r.tl.y))
            continue;

          std::vector<uint8_t> hash = ContentHash::computeRect(pb, r);
          CacheKey key = cacheKeyFromHash(hash);
          ++localStats.blocksHashed;

          if (!clientKnows(key))
            continue;

          ++localStats.blocksHit;
          hitMask[idx(gx, gy)] = 1;
        }
      }

      // Greedy packing over the hit mask.
      std::vector<uint8_t> maskCopy = hitMask;
      std::vector<core::Rect> packed;

      for (int gy = 0; gy < gridH; ++gy) {
        for (int gx = 0; gx < gridW; ++gx) {
          if (maskCopy[idx(gx, gy)] == 0)
            continue;

          int w = 0;
          while (gx + w < gridW && maskCopy[idx(gx + w, gy)] != 0)
            ++w;

          int h = 1;
          for (;;) {
            if (gy + h >= gridH)
              break;
            bool ok = true;
            for (int x = 0; x < w; ++x) {
              if (maskCopy[idx(gx + x, gy + h)] == 0) {
                ok = false;
                break;
              }
            }
            if (!ok)
              break;
            ++h;
          }

          for (int yy = 0; yy < h; ++yy)
            for (int xx = 0; xx < w; ++xx)
              maskCopy[idx(gx + xx, gy + yy)] = 0;

          core::Rect pr(x0 + gx * T, y0 + gy * T, x0 + (gx + w) * T, y0 + (gy + h) * T);
          pr = pr.intersect(fbRect);
          if (!pr.is_empty())
            packed.push_back(pr);
        }
      }

      localStats.packedRects += packed.size();

      if (cfg_.preferLargestFirst) {
        std::sort(packed.begin(), packed.end(),
                  [](const core::Rect& a, const core::Rect& b) { return a.area() > b.area(); });
      }

      for (const auto& pr : packed) {
        if (cfg_.minPackedArea > 0 && pr.area() < cfg_.minPackedArea)
          continue;

        if (pr.area() >= WholeRectCacheMinArea) {
          const int cov = coveragePermille(damage, pr);
          if (cov < cfg_.coverageThresholdPermille)
            continue;
        }

        // Exact verification for packed rectangle.
        std::vector<uint8_t> hash = ContentHash::computeRect(pb, pr);
        CacheKey key = cacheKeyFromHash(hash);
        ++localStats.rectsHashed;

        if (!clientKnows(key))
          continue;

        ++localStats.rectHitsVerified;
        out.push_back({pr, key});
      }
    }
  }

done:
  localStats.rectHitsEmitted = out.size();
  localStats.timeUs = nowUs() - t0;
  if (outStats)
    *outStats = localStats;
  return out;
}

} // namespace cache
} // namespace rfb
