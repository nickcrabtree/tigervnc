#ifdef __APPLE__
#include <gtest/gtest.h>
#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <FL/fl_draw.H>

#include "../../vncviewer/Surface.h"

// Test fixture with helper to populate source surface; has friend access via Surface.h
class SurfaceOSXWindowOrientation : public ::testing::Test {
protected:
  static Surface makeBandsSurface(int w, int h) {
    Surface s(w, h);
    for (int y = 0; y < h; ++y) {
      for (int x = 0; x < w; ++x) {
        unsigned char* p = reinterpret_cast<unsigned char*>(s.data) + (y * w + x) * 4;
        bool top = y < h / 2;
        p[2] = top ? 255 : 0;  // R
        p[1] = 0;              // G
        p[0] = top ? 0 : 255;  // B
        p[3] = 255;            // A
      }
    }
    return s;
  }
};

// Black-box: verify that Surface::draw renders upright when using the FLTK/CG
// window drawing path (the same path used on screen).
class OrientationWindow : public Fl_Window {
public:
  OrientationWindow(int w, int h, Surface* src, bool* drew_flag)
    : Fl_Window(w, h, "orientation-test"), src_(src), drew_(drew_flag) {
    end();
  }

  void draw() override {
    Fl_Window::draw();
    // Blit entire source to window at (0,0)
    src_->draw(0, 0, 0, 0, w(), h());
    if (drew_) *drew_ = true;
  }

private:
  Surface* src_;
  bool* drew_;
};

TEST_F(SurfaceOSXWindowOrientation, RendersUprightInWindow) {
  const int W = 20, H = 20;
  Surface src = makeBandsSurface(W, H);
  bool drew = false;
  OrientationWindow win(W, H, &src, &drew);
  win.show();
  // Let FLTK process and perform its own draw with a couple of cycles
  Fl::wait(0.2);
  Fl::redraw();
  Fl::wait(0.2);
  Fl::flush();
  ASSERT_TRUE(drew) << "draw() was not invoked";
  Fl::flush();

  // Capture window pixels (RGB)
  std::vector<unsigned char> buf(W * H * 3, 0);
  unsigned char* img = fl_read_image(buf.data(), 0, 0, W, H);
  ASSERT_NE(img, nullptr);

  auto pixel = [&](int x, int y) {
    size_t idx = (y * W + x) * 3;
    return std::tuple<int,int,int>(buf[idx], buf[idx+1], buf[idx+2]); // R,G,B
  };

  auto top = pixel(0, 0);
  auto bottom = pixel(0, H - 1);

  // Expect top row red, bottom row blue (upright)
  EXPECT_EQ(top, std::make_tuple(255, 0, 0));
  EXPECT_EQ(bottom, std::make_tuple(0, 0, 255));
}
#endif
