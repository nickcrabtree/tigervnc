/* Unit tests for DecodeManager cache gating (unified cache engine / PersistentCache).
 */

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <core/Configuration.h>
#include <rfb/DecodeManager.h>

using namespace rfb;

// Test-scoped parameters that Configuration::getParam() will see.
static core::BoolParameter gPersistentCacheParam("PersistentCache", "Enable PersistentCache in client", true);

namespace {

TEST(DecodeManagerGating, PersistentCacheDisabledStillConstructsEngine) {
  // Arrange: Disable PersistentCache. The unified cache engine should still
  // be constructed (memory-only), with protocol/disk usage gated internally.
  gPersistentCacheParam.setParam(false);

  // Act: construct DecodeManager with null connection (safe for this test)
  DecodeManager dm(nullptr);

  // Assert: unified cache engine pointer is available regardless of the
  // PersistentCache toggle.
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

TEST(DecodeManagerGating, PersistentCacheEnabledConstructsEngine) {
  gPersistentCacheParam.setParam(true);
  DecodeManager dm(nullptr);
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

} // namespace
int main(int argc, char** argv) {
  ::testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}
