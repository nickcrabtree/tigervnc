/* Unit tests for DecodeManager cache gating (ContentCache & PersistentCache).
 */

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <core/Configuration.h>
#include <rfb/DecodeManager.h>

using namespace rfb;

// Test-scoped parameters that Configuration::getParam() will see.
static core::BoolParameter gContentCacheParam(
  "ContentCache", "Enable ContentCache in client", true);
static core::BoolParameter gPersistentCacheParam(
  "PersistentCache", "Enable PersistentCache in client", true);

namespace {

TEST(DecodeManagerGating, PersistentCacheDisabledLeavesPointerNull)
{
  // Arrange: disable PersistentCache, enable ContentCache
  gPersistentCacheParam.setParam(false);
  gContentCacheParam.setParam(true);

  // Act: construct DecodeManager with null connection (safe for this test)
  DecodeManager dm(nullptr);

  // Assert: PersistentCache must not be constructed; ContentCache is present
  EXPECT_EQ(dm.getPersistentCacheForTest(), nullptr);
  EXPECT_NE(dm.getContentCacheForTest(), nullptr);
}

TEST(DecodeManagerGating, ContentCacheDisabledLeavesPointerNull)
{
  // Arrange: disable ContentCache, enable PersistentCache
  gContentCacheParam.setParam(false);
  gPersistentCacheParam.setParam(true);

  DecodeManager dm(nullptr);

  EXPECT_EQ(dm.getContentCacheForTest(), nullptr);
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

TEST(DecodeManagerGating, BothCachesEnabledCreateBoth)
{
  gContentCacheParam.setParam(true);
  gPersistentCacheParam.setParam(true);

  DecodeManager dm(nullptr);

  EXPECT_NE(dm.getContentCacheForTest(), nullptr);
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

} // namespace

int main(int argc, char** argv)
{
  ::testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}
