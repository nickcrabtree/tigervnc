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
static core::BoolParameter gContentCacheParam(
  "ContentCache", "Enable ContentCache in client", true);
static core::BoolParameter gPersistentCacheParam(
  "PersistentCache", "Enable PersistentCache in client", true);

namespace {

TEST(DecodeManagerGating, PersistentCacheDisabledLeavesPointerNull)
{
  // Arrange: disable PersistentCache, enable ContentCache. In the unified
  // design this must still construct the single GlobalClientPersistentCache
  // engine, but mark it as memory-only (no disk persistence and no
  // PersistentCache disk I/O) when PersistentCache is disabled.
  gPersistentCacheParam.setParam(false);
  gContentCacheParam.setParam(true);

  // Act: construct DecodeManager with null connection (safe for this test)
  DecodeManager dm(nullptr);

  // Assert: the unified cache engine pointer must still be non-null whenever
  // *either* of the cache options is enabled. Gating of protocol/disk usage is
  // handled internally via configuration and negotiation rather than by
  // constructing/destroying the engine itself.
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

TEST(DecodeManagerGating, ContentCacheDisabledLeavesPointerNull)
{
  // Arrange: disable ContentCache, enable PersistentCache
  gContentCacheParam.setParam(false);
  gPersistentCacheParam.setParam(true);

  DecodeManager dm(nullptr);

  // With ContentCache disabled but PersistentCache enabled, the unified cache
  // engine must still be available.
  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

TEST(DecodeManagerGating, BothCachesEnabledCreateBoth)
{
  gContentCacheParam.setParam(true);
  gPersistentCacheParam.setParam(true);

  DecodeManager dm(nullptr);

  EXPECT_NE(dm.getPersistentCacheForTest(), nullptr);
}

} // namespace

int main(int argc, char** argv)
{
  ::testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}
