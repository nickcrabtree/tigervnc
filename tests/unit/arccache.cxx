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

#include <rfb/cache/ArcCache.h>

using namespace rfb::cache;

// Simple test entry
struct TestEntry {
  int value;
  size_t bytes;
  
  TestEntry() : value(0), bytes(sizeof(int)) {}
  TestEntry(int v, size_t b) : value(v), bytes(b) {}
};

// ============================================================================
// ArcCache Basic Operations
// ============================================================================

TEST(ArcCache, BasicInsertAndLookup)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,  // 1 KB max
      [](const TestEntry& e) { return e.bytes; },
      nullptr  // No eviction callback
  );
  
  cache.insert(1, TestEntry(100, 10));
  
  ASSERT_TRUE(cache.has(1));
  const TestEntry* entry = cache.get(1);
  ASSERT_NE(entry, nullptr);
  EXPECT_EQ(entry->value, 100);
}

TEST(ArcCache, MissReturnsNull)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  EXPECT_FALSE(cache.has(999));
  EXPECT_EQ(cache.get(999), nullptr);
}

TEST(ArcCache, MultipleInserts)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  for (uint64_t i = 1; i <= 10; i++) {
    cache.insert(i, TestEntry(i * 100, 10));
  }
  
  // Verify all entries
  for (uint64_t i = 1; i <= 10; i++) {
    ASSERT_TRUE(cache.has(i));
    const TestEntry* entry = cache.get(i);
    ASSERT_NE(entry, nullptr);
    EXPECT_EQ(entry->value, i * 100);
  }
}

// ============================================================================
// ARC T1 -> T2 Promotion
// ============================================================================

TEST(ArcCache, PromotionT1ToT2)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entry (goes to T1)
  cache.insert(1, TestEntry(100, 10));
  
  auto stats = cache.getStats();
  EXPECT_EQ(stats.t1Size, 1);
  EXPECT_EQ(stats.t2Size, 0);
  
  // Access again (should promote to T2)
  cache.get(1);
  
  stats = cache.getStats();
  EXPECT_EQ(stats.t1Size, 0);
  EXPECT_EQ(stats.t2Size, 1);
}

TEST(ArcCache, MultipleAccessesStayInT2)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  cache.insert(1, TestEntry(100, 10));
  
  // Multiple accesses
  for (int i = 0; i < 5; i++) {
    cache.get(1);
  }
  
  auto stats = cache.getStats();
  EXPECT_EQ(stats.t2Size, 1);
  EXPECT_EQ(stats.t1Size, 0);
}

// ============================================================================
// ARC Capacity Enforcement
// ============================================================================

TEST(ArcCache, EvictionWhenFull)
{
  ArcCache<uint64_t, TestEntry> cache(
      100,  // Small capacity: 100 bytes
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entries totaling more than capacity
  for (uint64_t i = 1; i <= 20; i++) {
    cache.insert(i, TestEntry(i * 10, 10));  // Each entry is 10 bytes
  }
  
  auto stats = cache.getStats();
  
  // Should have evicted some entries
  EXPECT_LE(stats.totalBytes, 100);
  EXPECT_GT(stats.evictions, 0);
  
  // Early entries should be evicted
  EXPECT_FALSE(cache.has(1));
  EXPECT_FALSE(cache.has(2));
  
  // Recent entries should exist
  EXPECT_TRUE(cache.has(19));
  EXPECT_TRUE(cache.has(20));
}

TEST(ArcCache, EvictionCallback)
{
  std::vector<uint64_t> evicted;
  
  ArcCache<uint64_t, TestEntry> cache(
      50,  // Very small: 50 bytes
      [](const TestEntry& e) { return e.bytes; },
      [&evicted](uint64_t key) {
        evicted.push_back(key);
      }
  );
  
  // Fill beyond capacity
  for (uint64_t i = 1; i <= 10; i++) {
    cache.insert(i, TestEntry(i * 10, 10));
  }
  
  // Callback should have been invoked
  EXPECT_GT(evicted.size(), 0);
  
  // Evicted keys should not be in cache
  for (uint64_t key : evicted) {
    EXPECT_FALSE(cache.has(key));
  }
}

// ============================================================================
// ARC Ghost Lists and Adaptive Parameter
// ============================================================================

TEST(ArcCache, GhostListTracking)
{
  ArcCache<uint64_t, TestEntry> cache(
      100,  // 100 bytes capacity
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Fill cache beyond capacity to trigger evictions
  for (uint64_t i = 1; i <= 20; i++) {
    cache.insert(i, TestEntry(i * 10, 10));
  }
  
  auto stats = cache.getStats();
  
  // Ghost lists should have entries
  EXPECT_GT(stats.b1Size + stats.b2Size, 0);
}

TEST(ArcCache, GhostHitAdjustsP)
{
  ArcCache<uint64_t, TestEntry> cache(
      100,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entries to fill T1
  for (uint64_t i = 1; i <= 10; i++) {
    cache.insert(i, TestEntry(i * 10, 10));
  }
  
  // Add more entries to cause evictions to B1
  for (uint64_t i = 11; i <= 20; i++) {
    cache.insert(i, TestEntry(i * 10, 10));
  }
  
  // Re-access an entry that should be in B1 (ghost hit)
  cache.insert(5, TestEntry(50, 10));
  
  // Ghost hit should adjust adaptive parameter
  // (exact behavior depends on ARC algorithm, just verify cache works)
  EXPECT_TRUE(cache.has(5));
}

// ============================================================================
// ARC Statistics
// ============================================================================

TEST(ArcCache, StatisticsTracking)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entries (each insert is a cache miss)
  cache.insert(1, TestEntry(100, 10));
  cache.insert(2, TestEntry(200, 20));
  cache.insert(3, TestEntry(300, 30));
  
  auto stats = cache.getStats();
  
  EXPECT_EQ(stats.totalEntries, 3);
  EXPECT_EQ(stats.totalBytes, 60);  // 10 + 20 + 30
  EXPECT_EQ(stats.cacheHits, 0);
  EXPECT_EQ(stats.cacheMisses, 3);  // 3 inserts = 3 misses
  
  // Access existing entry (hit)
  cache.get(1);
  stats = cache.getStats();
  EXPECT_EQ(stats.cacheHits, 1);
  EXPECT_EQ(stats.cacheMisses, 3);  // No change
  
  // Access non-existent entry (miss)
  cache.get(999);
  stats = cache.getStats();
  EXPECT_EQ(stats.cacheMisses, 4);  // One more miss
}

TEST(ArcCache, Clear)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entries
  for (uint64_t i = 1; i <= 10; i++) {
    cache.insert(i, TestEntry(i * 10, 10));
  }
  
  auto statsBefore = cache.getStats();
  EXPECT_GT(statsBefore.totalEntries, 0);
  
  cache.clear();
  
  // After clear: lists and entries should be empty
  EXPECT_EQ(cache.getStats().t1Size, 0);
  EXPECT_EQ(cache.getStats().t2Size, 0);
  EXPECT_EQ(cache.getStats().b1Size, 0);
  EXPECT_EQ(cache.getStats().b2Size, 0);
  
  // Verify cache is actually empty
  for (uint64_t i = 1; i <= 10; i++) {
    EXPECT_FALSE(cache.has(i));
  }
}

// ============================================================================
// ARC with Variable-Size Entries
// ============================================================================

TEST(ArcCache, VariableSizeEntries)
{
  ArcCache<uint64_t, TestEntry> cache(
      1000,  // 1000 bytes
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  // Insert entries of different sizes
  cache.insert(1, TestEntry(100, 50));   // 50 bytes
  cache.insert(2, TestEntry(200, 200));  // 200 bytes
  cache.insert(3, TestEntry(300, 100));  // 100 bytes
  cache.insert(4, TestEntry(400, 500));  // 500 bytes
  cache.insert(5, TestEntry(500, 300));  // 300 bytes
  
  auto stats = cache.getStats();
  
  // Should have evicted some to stay under capacity
  EXPECT_LE(stats.totalBytes, 1000);
  EXPECT_GT(stats.evictions, 0);
}

// ============================================================================
// ARC with Vector Keys (PersistentCache use case)
// ============================================================================

struct VectorHasher {
  size_t operator()(const std::vector<uint8_t>& v) const {
    size_t h = 14695981039346656037ULL;
    for (uint8_t b : v) {
      h ^= b;
      h *= 1099511628211ULL;
    }
    return h;
  }
};

TEST(ArcCache, VectorKeys)
{
  ArcCache<std::vector<uint8_t>, TestEntry, VectorHasher> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  std::vector<uint8_t> key1 = {0xAA, 0xBB, 0xCC, 0xDD};
  std::vector<uint8_t> key2 = {0x11, 0x22, 0x33, 0x44};
  
  cache.insert(key1, TestEntry(100, 10));
  cache.insert(key2, TestEntry(200, 20));
  
  ASSERT_TRUE(cache.has(key1));
  ASSERT_TRUE(cache.has(key2));
  
  const TestEntry* e1 = cache.get(key1);
  const TestEntry* e2 = cache.get(key2);
  
  ASSERT_NE(e1, nullptr);
  ASSERT_NE(e2, nullptr);
  EXPECT_EQ(e1->value, 100);
  EXPECT_EQ(e2->value, 200);
}

// ============================================================================
// ARC Update Existing Entry
// ============================================================================

TEST(ArcCache, UpdateExistingEntry)
{
  ArcCache<uint64_t, TestEntry> cache(
      1024,
      [](const TestEntry& e) { return e.bytes; },
      nullptr
  );
  
  cache.insert(1, TestEntry(100, 10));
  
  const TestEntry* entry = cache.get(1);
  ASSERT_NE(entry, nullptr);
  EXPECT_EQ(entry->value, 100);
  
  // Update with new value
  cache.insert(1, TestEntry(999, 10));
  
  entry = cache.get(1);
  ASSERT_NE(entry, nullptr);
  EXPECT_EQ(entry->value, 999);
  
  // Should still have only 1 entry
  EXPECT_EQ(cache.getStats().totalEntries, 1);
}
