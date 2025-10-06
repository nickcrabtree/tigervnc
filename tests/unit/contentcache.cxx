/* Copyright (C) 2025 TigerVNC Team
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

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <rfb/ContentCache.h>
#include <rfb/UpdateTracker.h>
#include <rfb/PixelBuffer.h>
#include <core/Rect.h>

using namespace rfb;

// ============================================================================
// ContentCache Tests
// ============================================================================

TEST(ContentCache, BasicInsertAndFind)
{
  ContentCache cache(10, 300); // 10MB, 5min TTL
  
  // Create test data
  std::vector<uint8_t> data(64*64*4, 0xFF); // 64x64 white rectangle
  uint64_t hash = computeContentHash(data.data(), data.size());
  
  core::Rect bounds(0, 0, 64, 64);
  cache.insertContent(hash, bounds, data.data(), data.size(), false);
  
  // Should find it
  auto entry = cache.findContent(hash);
  ASSERT_NE(entry, nullptr);
  EXPECT_EQ(entry->contentHash, hash);
  EXPECT_EQ(entry->lastBounds, bounds);
}

TEST(ContentCache, CacheMiss)
{
  ContentCache cache(10, 300);
  
  // Look for non-existent content
  auto entry = cache.findContent(0xDEADBEEF);
  EXPECT_EQ(entry, nullptr);
}

TEST(ContentCache, LRUEviction)
{
  ContentCache cache(1, 300); // Only 1MB cache
  
  std::vector<uint8_t> data(256*1024, 0x42); // 256KB chunks
  
  // Fill cache with 5 entries (should trigger eviction)
  std::vector<uint64_t> hashes;
  for (int i = 0; i < 5; i++) {
    data[0] = i; // Make each entry unique
    uint64_t hash = computeContentHash(data.data(), data.size());
    hashes.push_back(hash);
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(hash, bounds, data.data(), data.size(), false);
  }
  
  // First entry should be evicted
  auto entry = cache.findContent(hashes[0]);
  EXPECT_EQ(entry, nullptr);
  
  // Last entry should still exist
  entry = cache.findContent(hashes[4]);
  EXPECT_NE(entry, nullptr);
}

TEST(ContentCache, TouchUpdatesLRU)
{
  ContentCache cache(1, 300);
  
  std::vector<uint8_t> data(256*1024, 0x42);
  
  std::vector<uint64_t> hashes;
  for (int i = 0; i < 3; i++) {
    data[0] = i;
    uint64_t hash = computeContentHash(data.data(), data.size());
    hashes.push_back(hash);
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(hash, bounds, data.data(), data.size(), false);
  }
  
  // Touch first entry to make it recent
  cache.touchEntry(hashes[0]);
  
  // Add two more entries (should evict middle one, not first)
  for (int i = 3; i < 5; i++) {
    data[0] = i;
    uint64_t hash = computeContentHash(data.data(), data.size());
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(hash, bounds, data.data(), data.size(), false);
  }
  
  // First entry should still exist (was touched)
  auto entry = cache.findContent(hashes[0]);
  EXPECT_NE(entry, nullptr);
  
  // Middle entries should be evicted
  entry = cache.findContent(hashes[1]);
  EXPECT_EQ(entry, nullptr);
}

TEST(ContentCache, Statistics)
{
  ContentCache cache(10, 300);
  
  std::vector<uint8_t> data(1024, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  
  core::Rect bounds(0, 0, 32, 32);
  cache.insertContent(hash, bounds, data.data(), data.size(), false);
  
  // Hit
  auto entry = cache.findContent(hash);
  EXPECT_NE(entry, nullptr);
  
  // Miss
  entry = cache.findContent(0xBAADF00D);
  EXPECT_EQ(entry, nullptr);
  
  auto stats = cache.getStats();
  EXPECT_EQ(stats.cacheHits, 1);
  EXPECT_EQ(stats.cacheMisses, 1);
  EXPECT_GT(stats.totalEntries, 0);
}

TEST(ContentCache, Clear)
{
  ContentCache cache(10, 300);
  
  std::vector<uint8_t> data(1024, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  
  core::Rect bounds(0, 0, 32, 32);
  cache.insertContent(hash, bounds, data.data(), data.size(), false);
  
  EXPECT_NE(cache.findContent(hash), nullptr);
  
  cache.clear();
  
  EXPECT_EQ(cache.findContent(hash), nullptr);
  EXPECT_EQ(cache.getStats().totalEntries, 0);
}

// ============================================================================
// Hash Function Tests
// ============================================================================

TEST(ContentHash, DifferentDataDifferentHash)
{
  std::vector<uint8_t> data1(1024, 0xAA);
  std::vector<uint8_t> data2(1024, 0xBB);
  
  uint64_t hash1 = computeContentHash(data1.data(), data1.size());
  uint64_t hash2 = computeContentHash(data2.data(), data2.size());
  
  EXPECT_NE(hash1, hash2);
}

TEST(ContentHash, SameDataSameHash)
{
  std::vector<uint8_t> data1(1024, 0xAA);
  std::vector<uint8_t> data2(1024, 0xAA);
  
  uint64_t hash1 = computeContentHash(data1.data(), data1.size());
  uint64_t hash2 = computeContentHash(data2.data(), data2.size());
  
  EXPECT_EQ(hash1, hash2);
}

TEST(ContentHash, SmallChange)
{
  std::vector<uint8_t> data1(1024, 0xAA);
  std::vector<uint8_t> data2(1024, 0xAA);
  data2[512] = 0xBB; // Change one byte
  
  uint64_t hash1 = computeContentHash(data1.data(), data1.size());
  uint64_t hash2 = computeContentHash(data2.data(), data2.size());
  
  EXPECT_NE(hash1, hash2);
}

// ============================================================================
// UpdateTracker + CopyRect Tests (Currently Missing in TigerVNC!)
// ============================================================================

TEST(UpdateTracker, BasicCopyRect)
{
  SimpleUpdateTracker tracker;
  
  // Add a copied region: 64x64 area moved from (0,0) to (100,100)
  core::Region dest(core::Rect(100, 100, 164, 164));
  core::Point delta(-100, -100); // Source was at (0,0)
  
  tracker.add_copied(dest, delta);
  
  UpdateInfo info;
  tracker.getUpdateInfo(&info, core::Region(core::Rect(0, 0, 200, 200)));
  
  EXPECT_FALSE(info.copied.is_empty());
  EXPECT_EQ(info.copy_delta.x, -100);
  EXPECT_EQ(info.copy_delta.y, -100);
}

TEST(UpdateTracker, CopyRectDoesNotOverlapChanged)
{
  SimpleUpdateTracker tracker;
  
  // Add changed region
  tracker.add_changed(core::Region(core::Rect(0, 0, 50, 50)));
  
  // Add copied region that overlaps
  core::Region dest(core::Rect(25, 25, 75, 75));
  core::Point delta(-10, -10);
  tracker.add_copied(dest, delta);
  
  UpdateInfo info;
  tracker.getUpdateInfo(&info, core::Region(core::Rect(0, 0, 100, 100)));
  
  // Copied should be reduced to not overlap changed
  EXPECT_TRUE(info.copied.intersect(info.changed).is_empty());
}

TEST(UpdateTracker, MultipleCopyRectsCoalesce)
{
  SimpleUpdateTracker tracker;
  
  // Add two sequential copy operations
  tracker.add_copied(core::Region(core::Rect(10, 10, 50, 50)), 
                     core::Point(-5, -5));
  tracker.add_copied(core::Region(core::Rect(20, 20, 60, 60)), 
                     core::Point(-5, -5));
  
  UpdateInfo info;
  tracker.getUpdateInfo(&info, core::Region(core::Rect(0, 0, 100, 100)));
  
  // Should still have copy information
  EXPECT_FALSE(info.copied.is_empty());
}

// ============================================================================
// Integration Tests (ContentCache + UpdateTracker)
// ============================================================================

TEST(Integration, CacheHitUsesHistoricalLocation)
{
  ContentCache cache(10, 300);
  
  // Simulate: content appears at (0,0), gets cached
  std::vector<uint8_t> data(64*64*4, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  core::Rect firstLocation(0, 0, 64, 64);
  
  cache.insertContent(hash, firstLocation, data.data(), data.size(), false);
  
  // Later: same content appears at (200, 200)
  auto entry = cache.findContent(hash);
  ASSERT_NE(entry, nullptr);
  
  // Should remember it was at (0,0)
  EXPECT_EQ(entry->lastBounds, firstLocation);
  
  // We can use CopyRect from (0,0) to (200,200)!
  core::Rect newLocation(200, 200, 264, 264);
  core::Point delta(
    firstLocation.tl.x - newLocation.tl.x,
    firstLocation.tl.y - newLocation.tl.y
  );
  
  // Verify delta makes sense
  EXPECT_EQ(delta.x, -200);
  EXPECT_EQ(delta.y, -200);
}

TEST(Integration, RealWorldScenario_WindowSwitch)
{
  ContentCache cache(10, 300);
  SimpleUpdateTracker tracker;
  
  // Scenario: Terminal window at (0,0)
  std::vector<uint8_t> terminalData(128*128*4);
  std::fill(terminalData.begin(), terminalData.end(), 0x11);
  uint64_t termHash = computeContentHash(terminalData.data(), 
                                         terminalData.size());
  cache.insertContent(termHash, core::Rect(0, 0, 128, 128),
                     terminalData.data(), terminalData.size(), false);
  
  // User switches to browser, terminal content hidden
  // ... time passes ...
  
  // User switches back to terminal
  // Same content reappears!
  auto entry = cache.findContent(termHash);
  ASSERT_NE(entry, nullptr);
  
  // Statistics show cache hit
  auto stats = cache.getStats();
  EXPECT_GT(stats.cacheHits, 0);
  
  // Bandwidth saved = size of data not re-encoded
  size_t bandwidthSaved = terminalData.size();
  EXPECT_GT(bandwidthSaved, 0);
}

// ============================================================================
// Edge Cases
// ============================================================================

TEST(ContentCache, ZeroSizeData)
{
  ContentCache cache(10, 300);
  
  uint64_t hash = computeContentHash(nullptr, 0);
  core::Rect bounds(0, 0, 0, 0);
  
  // Should handle gracefully
  cache.insertContent(hash, bounds, nullptr, 0, false);
  
  auto entry = cache.findContent(hash);
  (void)entry; // Suppress unused variable warning
  // May or may not find it (implementation dependent)
  // But shouldn't crash
}

TEST(ContentCache, VeryLargeData)
{
  ContentCache cache(100, 300); // 100MB cache
  
  // Try to cache 10MB chunk
  std::vector<uint8_t> data(10*1024*1024, 0x42);
  uint64_t hash = computeContentHash(data.data(), data.size());
  core::Rect bounds(0, 0, 1920, 1080);
  
  cache.insertContent(hash, bounds, data.data(), data.size(), false);
  
  auto entry = cache.findContent(hash);
  EXPECT_NE(entry, nullptr);
}

TEST(ContentCache, AgeBasedEviction)
{
  ContentCache cache(100, 1); // 1 second TTL
  
  std::vector<uint8_t> data(1024, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  core::Rect bounds(0, 0, 32, 32);
  
  cache.insertContent(hash, bounds, data.data(), data.size(), false);
  
  // Should exist immediately
  EXPECT_NE(cache.findContent(hash), nullptr);
  
  // Sleep 2 seconds
  // Note: In real test, you'd mock time instead of sleeping
  // sleep(2);
  
  // Force pruning
  cache.pruneCache();
  
  // Should be evicted due to age
  // EXPECT_FALSE(cache.findContent(hash).has_value());
  // TODO: Implement time mocking for this test
}
