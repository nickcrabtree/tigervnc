#ifdef __APPLE__
#include <gtest/gtest.h>
#include "../../vncviewer/Surface.h"

// Black-box orientation test: draw a two-color source surface and blit to a
// destination Surface. Verify the top row keeps the source's top color.
TEST(SurfaceOSXOrientation, BlitPreservesTopRow) {
  const int w = 4, h = 4;
  Surface src(w, h);
  Surface dst(w, h);

  // Fill src: top half red (255,0,0), bottom half blue (0,0,255)
  for (int y = 0; y < h; ++y) {
    for (int x = 0; x < w; ++x) {
      unsigned char* p = src.data + (y * w + x) * 4;
      if (y < h / 2) { p[2] = 255; p[1] = 0; p[0] = 0; } // RGB in BGRA order
      else           { p[2] = 0;   p[1] = 0; p[0] = 255; }
      p[3] = 255;
    }
  }

  // Blit entire src into dst at (0,0)
  src.draw(&dst, 0, 0, 0, 0, w, h);

  // Read dst top-left pixel (y=0) and bottom-left pixel (y=h-1)
  auto pixel = [&](int y) {
    unsigned char* p = dst.data + (y * w) * 4;
    return std::tuple<int,int,int>(p[2], p[1], p[0]); // R,G,B
  };

  auto top = pixel(0);
  auto bottom = pixel(h - 1);

  // Expect top row red, bottom row blue
  EXPECT_EQ(top, std::make_tuple(255, 0, 0));
  EXPECT_EQ(bottom, std::make_tuple(0, 0, 255));
}
#endif
