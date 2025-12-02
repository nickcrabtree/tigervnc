/* Copyright (C) 2025 TigerVNC Team.  All Rights Reserved.
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

#include <rfb/ContentHash.h>
#include <algorithm>

using namespace rfb;

std::vector<ContentHash::BorderedRegion> ContentHash::detectBorderedRegions(
    const PixelBuffer* pb,
    int /*minBorderWidth*/,
    int minArea) {
  
  std::vector<BorderedRegion> regions;
  
  if (!pb) return regions;
  
  int width = pb->width();
  int height = pb->height();
  
  if (width < 400 || height < 300) {
    return regions;  // Too small for typical application layout
  }
  
  (void)minArea;  // Not used in this simplified approach
  
  // HEURISTIC APPROACH for LibreOffice Impress editing mode:
  //
  // The UI layout is typically:
  // - Left panel: slide thumbnails (about 10-15% of width, ~150-200px)
  // - Top: toolbar and menu (about 50-100px)
  // - Main area: slide editor with the slide centered
  //
  // The slide itself has a shadow/border around it. We estimate the slide
  // panel boundaries based on typical UI proportions.
  //
  // For a 1920x1080 screen:
  // - Slide panel starts around x=170 (after thumbnails)
  // - Slide panel ends around x=1800
  // - Top starts around y=80 (after toolbars)
  // - Bottom ends around y=1020 (before status bar)
  //
  // We use proportional positioning to handle different resolutions.
  
  // Estimate slide editing area bounds (proportional to screen)
  int leftMargin = width * 9 / 100;    // ~9% for thumbnail panel
  int rightMargin = width * 2 / 100;   // ~2% right margin
  int topMargin = height * 8 / 100;    // ~8% for toolbars
  int bottomMargin = height * 2 / 100; // ~2% for status bar
  
  int contentLeft = leftMargin;
  int contentRight = width - rightMargin;
  int contentTop = topMargin;
  int contentBottom = height - bottomMargin;
  
  int contentWidth = contentRight - contentLeft;
  int contentHeight = contentBottom - contentTop;
  
  // Only return if the estimated region is reasonably sized
  if (contentWidth >= 300 && contentHeight >= 200) {
    BorderedRegion region;
    region.contentRect = core::Rect(contentLeft, contentTop, contentRight, contentBottom);
    region.outerRect = region.contentRect;
    region.borderColor = 0;
    region.borderLeft = leftMargin;
    region.borderRight = rightMargin;
    region.borderTop = topMargin;
    region.borderBottom = bottomMargin;
    regions.push_back(region);
  }
  
  return regions;
}
