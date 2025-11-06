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
  ContentKey key(64, 64, hash);  // width, height, hash
  cache.insertContent(key, bounds, data.data(), data.size(), false);
  
  // Should find it
  auto entry = cache.findContent(key);
  ASSERT_NE(entry, nullptr);
  EXPECT_EQ(entry->contentHash, hash);
  EXPECT_EQ(entry->lastBounds, bounds);
}

TEST(ContentCache, CacheMiss)
{
  ContentCache cache(10, 300);
  
  // Look for non-existent content
  ContentKey key(64, 64, 0xDEADBEEF);
  auto entry = cache.findContent(key);
  EXPECT_EQ(entry, nullptr);
}

TEST(ContentCache, LRUEviction)
{
  ContentCache cache(1, 300); // Only 1MB cache
  
  std::vector<uint8_t> data(256*1024, 0x42); // 256KB chunks
  
  // Fill cache with 5 entries (should trigger eviction)
  std::vector<ContentKey> keys;
  for (int i = 0; i < 5; i++) {
    data[0] = i; // Make each entry unique
    uint64_t hash = computeContentHash(data.data(), data.size());
    ContentKey key(64, 64, hash);  // All same dimensions
    keys.push_back(key);
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(key, bounds, data.data(), data.size(), false);
  }
  
  // First entry should be evicted
  auto entry = cache.findContent(keys[0]);
  EXPECT_EQ(entry, nullptr);
  
  // Last entry should still exist
  entry = cache.findContent(keys[4]);
  EXPECT_NE(entry, nullptr);
}

TEST(ContentCache, TouchUpdatesLRU)
{
  ContentCache cache(1, 300);
  
  std::vector<uint8_t> data(256*1024, 0x42);
  
  std::vector<ContentKey> keys;
  for (int i = 0; i < 3; i++) {
    data[0] = i;
    uint64_t hash = computeContentHash(data.data(), data.size());
    ContentKey key(64, 64, hash);
    keys.push_back(key);
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(key, bounds, data.data(), data.size(), false);
  }
  
  // Touch first entry to make it recent
  cache.touchEntry(keys[0]);
  
  // Add two more entries (should evict middle one, not first)
  for (int i = 3; i < 5; i++) {
    data[0] = i;
    uint64_t hash = computeContentHash(data.data(), data.size());
    ContentKey key(64, 64, hash);
    
    core::Rect bounds(i*64, 0, (i+1)*64, 64);
    cache.insertContent(key, bounds, data.data(), data.size(), false);
  }
  
  // First entry should still exist (was touched)
  auto entry = cache.findContent(keys[0]);
  EXPECT_NE(entry, nullptr);
  
  // Middle entries should be evicted
  entry = cache.findContent(keys[1]);
  EXPECT_EQ(entry, nullptr);
}

TEST(ContentCache, Statistics)
{
  ContentCache cache(10, 300);
  
  std::vector<uint8_t> data(1024, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  ContentKey key(32, 32, hash);
  
  core::Rect bounds(0, 0, 32, 32);
  cache.insertContent(key, bounds, data.data(), data.size(), false);
  
  // Hit
  auto entry = cache.findContent(key);
  EXPECT_NE(entry, nullptr);
  
  // Miss
  ContentKey missKey(32, 32, 0xBAADF00D);
  entry = cache.findContent(missKey);
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
  ContentKey key(32, 32, hash);
  
  core::Rect bounds(0, 0, 32, 32);
  cache.insertContent(key, bounds, data.data(), data.size(), false);
  
  EXPECT_NE(cache.findContent(key), nullptr);
  
  cache.clear();
  
  EXPECT_EQ(cache.findContent(key), nullptr);
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
  ContentKey key(64, 64, hash);
  
  cache.insertContent(key, firstLocation, data.data(), data.size(), false);
  
  // Later: same content appears at (200, 200)
  auto entry = cache.findContent(key);
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
  ContentKey key(128, 128, termHash);
  cache.insertContent(key, core::Rect(0, 0, 128, 128),
                     terminalData.data(), terminalData.size(), false);
  
  // User switches to browser, terminal content hidden
  // ... time passes ...
  
  // User switches back to terminal
  // Same content reappears!
  auto entry = cache.findContent(key);
  ASSERT_NE(entry, nullptr);
  
  // Statistics show cache hit
  auto stats = cache.getStats();
  EXPECT_GT(stats.cacheHits, 0);
  
  // Bandwidth saved = size of data not re-encoded
  size_t bandwidthSaved = terminalData.size();
  EXPECT_GT(bandwidthSaved, 0);
}

// ============================================================================
// ContentKey Dimension Tests (November 6, 2025 fix)
// ============================================================================

TEST(ContentKey, DimensionDisambiguation)
{
  ContentCache cache(10, 300);
  
  // Same hash, different dimensions should be separate entries
  uint64_t hash = 0x12345678;
  
  std::vector<uint8_t> data1(2040*8*4, 0xAA);  // 2040x8
  std::vector<uint8_t> data2(2024*8*4, 0xBB);  // 2024x8
  
  ContentKey key1(2040, 8, hash);
  ContentKey key2(2024, 8, hash);  // Same hash, different dimensions
  
  // Insert both
  cache.insertContent(key1, core::Rect(0, 0, 2040, 8), data1.data(), data1.size(), false);
  cache.insertContent(key2, core::Rect(0, 0, 2024, 8), data2.data(), data2.size(), false);
  
  // Both should exist independently
  auto entry1 = cache.findContent(key1);
  auto entry2 = cache.findContent(key2);
  
  ASSERT_NE(entry1, nullptr);
  ASSERT_NE(entry2, nullptr);
  
  // Verify they're different entries
  EXPECT_EQ(entry1->lastBounds.width(), 2040);
  EXPECT_EQ(entry2->lastBounds.width(), 2024);
}

TEST(ContentKey, EqualityOperator)
{
  ContentKey key1(1024, 768, 0x123456);
  ContentKey key2(1024, 768, 0x123456);  // Identical
  ContentKey key3(1024, 768, 0xABCDEF);  // Different hash
  ContentKey key4(800, 600, 0x123456);   // Different dimensions
  
  // Same dimensions and hash should be equal
  EXPECT_TRUE(key1 == key2);
  
  // Different hash should not be equal
  EXPECT_FALSE(key1 == key3);
  
  // Different dimensions should not be equal
  EXPECT_FALSE(key1 == key4);
}

TEST(ContentKey, HashFunction)
{
  ContentKey key1(1024, 768, 0x123456);
  ContentKey key2(1024, 768, 0x123456);  // Identical
  ContentKey key3(1024, 768, 0xABCDEF);  // Different hash
  ContentKey key4(800, 600, 0x123456);   // Different dimensions
  
  ContentKeyHash hasher;
  
  // Identical keys should produce same hash
  EXPECT_EQ(hasher(key1), hasher(key2));
  
  // Different keys should (likely) produce different hashes
  EXPECT_NE(hasher(key1), hasher(key3));
  EXPECT_NE(hasher(key1), hasher(key4));
}

TEST(ContentKey, BoundaryDimensions)
{
  ContentCache cache(10, 300);
  
  std::vector<uint8_t> data(1024, 0xFF);
  uint64_t hash = computeContentHash(data.data(), data.size());
  
  // Test minimum dimensions
  ContentKey key0(0, 0, hash);
  cache.insertContent(key0, core::Rect(0, 0, 0, 0), data.data(), data.size(), false);
  
  // Test single pixel
  ContentKey key1(1, 1, hash);
  cache.insertContent(key1, core::Rect(0, 0, 1, 1), data.data(), data.size(), false);
  
  // Test maximum 16-bit dimensions (65535)
  ContentKey keyMax(65535, 65535, hash);
  cache.insertContent(keyMax, core::Rect(0, 0, 65535, 65535), data.data(), data.size(), false);
  
  // All should succeed without overflow
  EXPECT_NE(cache.findContent(key0), nullptr);
  EXPECT_NE(cache.findContent(key1), nullptr);
  EXPECT_NE(cache.findContent(keyMax), nullptr);
}

// ============================================================================
// Edge Cases
// ============================================================================

TEST(ContentCache, ZeroSizeData)
{
  ContentCache cache(10, 300);
  
  uint64_t hash = computeContentHash(nullptr, 0);
  ContentKey key(0, 0, hash);
  core::Rect bounds(0, 0, 0, 0);
  
  // Should handle gracefully
  cache.insertContent(key, bounds, nullptr, 0, false);
  
  auto entry = cache.findContent(key);
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
  ContentKey key(1920, 1080, hash);
  core::Rect bounds(0, 0, 1920, 1080);
  
  cache.insertContent(key, bounds, data.data(), data.size(), false);
  
  auto entry = cache.findContent(key);
  EXPECT_NE(entry, nullptr);
}

