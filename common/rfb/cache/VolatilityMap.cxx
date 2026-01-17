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

#include <rfb/cache/VolatilityMap.h>

static inline int ceil_div(int a, int b) {
  return (a + b - 1) / b;
}

rfb::cache::VolatilityMap::VolatilityMap(int fbWidth, int fbHeight, int gridSize, int windowMs)
    : gridSize_(gridSize), windowMs_(windowMs) {
  tilesX_ = ceil_div(fbWidth, gridSize_);
  tilesY_ = ceil_div(fbHeight, gridSize_);
  ewma_.assign(tilesX_ * tilesY_, 0);
  lastMs_.assign(tilesX_ * tilesY_, 0);
}

void rfb::cache::VolatilityMap::resize(int fbWidth, int fbHeight) {
  tilesX_ = ceil_div(fbWidth, gridSize_);
  tilesY_ = ceil_div(fbHeight, gridSize_);
  ewma_.assign(tilesX_ * tilesY_, 0);
  lastMs_.assign(tilesX_ * tilesY_, 0);
}

void rfb::cache::VolatilityMap::noteDamage(const core::Rect& /*bbox*/, uint64_t /*nowMs*/) {
  // Stub implementation (Phase 0 scaffolding).
}

bool rfb::cache::VolatilityMap::isVolatileXY(int /*x*/, int /*y*/) const {
  // Stub implementation (Phase 0 scaffolding).
  return false;
}

bool rfb::cache::VolatilityMap::rectTouchesVolatile(const core::Rect& /*r*/) const {
  // Stub implementation (Phase 0 scaffolding).
  return false;
}
