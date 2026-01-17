/* Copyright (C) 2026 TigerVNC Team.  All Rights Reserved.
 *
 * This is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This software is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this software; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307,
 * USA.
 */

#ifndef COMMON_RFB_CACHE_SHIFTTOLERANTSCAN_H_
#define COMMON_RFB_CACHE_SHIFTTOLERANTSCAN_H_

#include <core/Rect.h>
#include <core/Region.h>
#include <functional>
#include <rfb/CacheKey.h>
#include <stdint.h>
#include <vector>
namespace rfb {

class PixelBuffer;

namespace cache {

struct ScanPhase {
  // cppcheck-suppress unusedStructMember
  int ox;
  // cppcheck-suppress unusedStructMember
  int oy;
};

enum class PhaseSet {
  Minimal,
  Quarter,
};

struct ScanConfig {
  // cppcheck-suppress unusedStructMember
  std::vector<int> tileSizes;
  PhaseSet phaseSet;
  int padPixels;
  int budgetUs;
  int maxBlocks;
  int minPackedArea;
  int coverageThresholdPermille;
  bool preferLargestFirst;
  bool logStats;

  ScanConfig()
      : phaseSet(PhaseSet::Quarter), padPixels(512), budgetUs(2000), maxBlocks(5000), minPackedArea(2048),
        coverageThresholdPermille(500), preferLargestFirst(true), logStats(false) {}
};

struct ScanStats {
  uint64_t blocksConsidered;
  uint64_t blocksHashed;
  uint64_t rectsHashed;
  uint64_t blocksHit;
  uint64_t packedRects;
  uint64_t rectHitsVerified;
  uint64_t rectHitsEmitted;
  uint64_t timeUs;

  ScanStats()
      : blocksConsidered(0), blocksHashed(0), rectsHashed(0), blocksHit(0), packedRects(0), rectHitsVerified(0),
        rectHitsEmitted(0), timeUs(0) {}
};

struct KeyedRect {
  core::Rect rect;
  CacheKey key;
};

class VolatilityMap;

class ShiftTolerantScanner {
public:
  explicit ShiftTolerantScanner(const ScanConfig& cfg);

  template <typename ClientKnowsFn>
  std::vector<KeyedRect> scanAndPack(const core::Region& damage, const PixelBuffer* pb, VolatilityMap* vol,
                                     ClientKnowsFn clientKnows, ScanStats* outStats) {
    std::function<bool(const CacheKey&)> fn = clientKnows;
    return this->scanAndPackImpl(damage, pb, vol, fn, outStats);
  }

private:
  std::vector<KeyedRect> scanAndPackImpl(const core::Region& damage, const PixelBuffer* pb, VolatilityMap* vol,
                                         const std::function<bool(const CacheKey&)>& clientKnows, ScanStats* outStats);
  ScanConfig cfg_;
};

} // namespace cache
} // namespace rfb

#endif // COMMON_RFB_CACHE_SHIFTTOLERANTSCAN_H_
