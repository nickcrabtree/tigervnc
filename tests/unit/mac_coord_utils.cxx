#include <gtest/gtest.h>

#include "../../vncviewer/mac_coord_utils.h"

TEST(MacCoordUtils, NoFlipWhenScaleYNegative) {
  // FLTK typically sets scaleY < 0 to produce a top-left origin; no extra flip
  int windowH = 1000;
  int dst_y = 0;
  int dst_h = 100;
  EXPECT_EQ(mac_map_dst_y(-2.0, windowH, dst_y, dst_h), 0);
}

TEST(MacCoordUtils, FlipsWhenScaleYPositive) {
  int windowH = 1000;
  int dst_y = 50;
  int dst_h = 100;
  // Positive scaleY means origin at bottom-left; need to flip to top-left
  EXPECT_EQ(mac_map_dst_y(2.0, windowH, dst_y, dst_h), 850);
}
