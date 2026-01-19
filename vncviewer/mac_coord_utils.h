#pragma once

// Helper to map destination Y coordinate to Quartz window coordinates.
// FLTK on macOS typically sets a CTM with scaleY < 0 (origin at top-left).
// Flip when scaleY is positive (origin at bottom-left); leave unchanged when negative (already top-left).
inline int mac_map_dst_y(double scaleY, int windowH, int dst_y, int dst_h) {
  if (scaleY > 0) {
    return windowH - (dst_y + dst_h);
  }
  return dst_y;
}
