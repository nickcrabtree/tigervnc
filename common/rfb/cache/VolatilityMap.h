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

#ifndef COMMON_RFB_CACHE_VOLATILITYMAP_H_
#define COMMON_RFB_CACHE_VOLATILITYMAP_H_

#include <core/Rect.h>
#include <stdint.h>
#include <vector>

namespace rfb {
namespace cache {

class VolatilityMap {
public:
  VolatilityMap(int fbWidth, int fbHeight, int gridSize, int windowMs);

  void noteDamage(const core::Rect& bbox, uint64_t nowMs);
  bool isVolatileXY(int x, int y) const;
  bool rectTouchesVolatile(const core::Rect& r) const;

  void resize(int fbWidth, int fbHeight);

private:
  // cppcheck-suppress unusedStructMember
  int gridSize_;
  // cppcheck-suppress unusedStructMember
  int windowMs_;
  int tilesX_;
  // cppcheck-suppress unusedStructMember
  int tilesY_;

  // cppcheck-suppress unusedStructMember
  std::vector<uint32_t> ewma_;
  // cppcheck-suppress unusedStructMember
  std::vector<uint64_t> lastMs_;

  int tileIndex(int tx, int ty) const {
    return ty * tilesX_ + tx;
  }
};

} // namespace cache
} // namespace rfb

#endif // COMMON_RFB_CACHE_VOLATILITYMAP_H_
