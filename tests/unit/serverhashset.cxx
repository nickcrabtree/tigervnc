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

#include <rfb/cache/ServerHashSet.h>

using namespace rfb;

// ============================================================================
// ServerHashSet with uint64_t (ContentCache use case)
// ============================================================================

TEST(ServerHashSet, BasicAddAndHas)
{
  ServerHashSet<uint64_t> hashSet;
  
  hashSet.add(123);
  
  EXPECT_TRUE(hashSet.has(123));
  EXPECT_FALSE(hashSet.has(456));
  EXPECT_EQ(hashSet.size(), 1);
}

TEST(ServerHashSet, MultipleAdds)
{
  ServerHashSet<uint64_t> hashSet;
  
  for (uint64_t i = 1; i <= 100; i++) {
    hashSet.add(i);
  }
  
  EXPECT_EQ(hashSet.size(), 100);
  
  // Verify all are present
  for (uint64_t i = 1; i <= 100; i++) {
    EXPECT_TRUE(hashSet.has(i));
  }
  
  // Verify others are not
  EXPECT_FALSE(hashSet.has(0));
  EXPECT_FALSE(hashSet.has(101));
}

TEST(ServerHashSet, DuplicateAdd)
{
  ServerHashSet<uint64_t> hashSet;
  
  hashSet.add(42);
  hashSet.add(42);  // Duplicate
  hashSet.add(42);  // Another duplicate
  
  EXPECT_EQ(hashSet.size(), 1);
  EXPECT_TRUE(hashSet.has(42));
}

TEST(ServerHashSet, Remove)
{
  ServerHashSet<uint64_t> hashSet;
  
  hashSet.add(10);
  hashSet.add(20);
  hashSet.add(30);
  
  EXPECT_EQ(hashSet.size(), 3);
  
  bool removed = hashSet.remove(20);
  
  EXPECT_TRUE(removed);
  EXPECT_EQ(hashSet.size(), 2);
  EXPECT_TRUE(hashSet.has(10));
  EXPECT_FALSE(hashSet.has(20));
  EXPECT_TRUE(hashSet.has(30));
}

TEST(ServerHashSet, RemoveNonExistent)
{
  ServerHashSet<uint64_t> hashSet;
  
  hashSet.add(100);
  
  bool removed = hashSet.remove(999);
  
  EXPECT_FALSE(removed);
  EXPECT_EQ(hashSet.size(), 1);
}

TEST(ServerHashSet, RemoveMultiple)
{
  ServerHashSet<uint64_t> hashSet;
  
  for (uint64_t i = 1; i <= 10; i++) {
    hashSet.add(i);
  }
  
  std::vector<uint64_t> toRemove = {2, 4, 6, 8, 10};
  size_t removed = hashSet.removeMultiple(toRemove);
  
  EXPECT_EQ(removed, 5);
  EXPECT_EQ(hashSet.size(), 5);
  
  // Verify correct ones remain
  EXPECT_TRUE(hashSet.has(1));
  EXPECT_FALSE(hashSet.has(2));
  EXPECT_TRUE(hashSet.has(3));
  EXPECT_FALSE(hashSet.has(4));
  EXPECT_TRUE(hashSet.has(5));
}

TEST(ServerHashSet, RemoveMultipleWithSomeNonExistent)
{
  ServerHashSet<uint64_t> hashSet;
  
  hashSet.add(10);
  hashSet.add(20);
  hashSet.add(30);
  
  std::vector<uint64_t> toRemove = {10, 999, 30, 888};
  size_t removed = hashSet.removeMultiple(toRemove);
  
  EXPECT_EQ(removed, 2);  // Only 10 and 30 were present
  EXPECT_EQ(hashSet.size(), 1);
  EXPECT_TRUE(hashSet.has(20));
}

TEST(ServerHashSet, Clear)
{
  ServerHashSet<uint64_t> hashSet;
  
  for (uint64_t i = 1; i <= 50; i++) {
    hashSet.add(i);
  }
  
  EXPECT_EQ(hashSet.size(), 50);
  
  hashSet.clear();
  
  EXPECT_EQ(hashSet.size(), 0);
  
  // Verify all are gone
  for (uint64_t i = 1; i <= 50; i++) {
    EXPECT_FALSE(hashSet.has(i));
  }
}

TEST(ServerHashSet, Statistics)
{
  ServerHashSet<uint64_t> hashSet;
  
  // Add 10 items
  for (uint64_t i = 1; i <= 10; i++) {
    hashSet.add(i);
  }
  
  auto stats = hashSet.getStats();
  EXPECT_EQ(stats.currentSize, 10);
  EXPECT_EQ(stats.totalAdded, 10);
  EXPECT_EQ(stats.totalEvicted, 0);
  
  // Remove 3 items
  hashSet.remove(5);
  hashSet.remove(6);
  hashSet.remove(7);
  
  stats = hashSet.getStats();
  EXPECT_EQ(stats.currentSize, 7);
  EXPECT_EQ(stats.totalAdded, 10);
  EXPECT_EQ(stats.totalEvicted, 3);
  
  // Add more items
  hashSet.add(100);
  hashSet.add(200);
  
  stats = hashSet.getStats();
  EXPECT_EQ(stats.currentSize, 9);
  EXPECT_EQ(stats.totalAdded, 12);
  EXPECT_EQ(stats.totalEvicted, 3);
}

TEST(ServerHashSet, StatisticsAfterClear)
{
  ServerHashSet<uint64_t> hashSet;
  
  for (uint64_t i = 1; i <= 20; i++) {
    hashSet.add(i);
  }
  
  hashSet.remove(5);
  hashSet.remove(10);
  
  auto statsBefore = hashSet.getStats();
  EXPECT_GT(statsBefore.totalAdded, 0);
  
  hashSet.clear();
  
  auto statsAfter = hashSet.getStats();
  EXPECT_EQ(statsAfter.currentSize, 0);
  EXPECT_EQ(statsAfter.totalAdded, 0);
  EXPECT_EQ(statsAfter.totalEvicted, 0);
}

// ============================================================================
// ServerHashSet with vector<uint8_t> (PersistentCache use case)
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

TEST(ServerHashSet, VectorKeys)
{
  ServerHashSet<std::vector<uint8_t>, VectorHasher> hashSet;
  
  std::vector<uint8_t> hash1 = {0xAA, 0xBB, 0xCC, 0xDD};
  std::vector<uint8_t> hash2 = {0x11, 0x22, 0x33, 0x44};
  std::vector<uint8_t> hash3 = {0xFF, 0xEE, 0xDD, 0xCC};
  
  hashSet.add(hash1);
  hashSet.add(hash2);
  
  EXPECT_TRUE(hashSet.has(hash1));
  EXPECT_TRUE(hashSet.has(hash2));
  EXPECT_FALSE(hashSet.has(hash3));
  EXPECT_EQ(hashSet.size(), 2);
}

TEST(ServerHashSet, VectorKeysRemove)
{
  ServerHashSet<std::vector<uint8_t>, VectorHasher> hashSet;
  
  std::vector<uint8_t> hash1 = {0x01, 0x02, 0x03};
  std::vector<uint8_t> hash2 = {0x04, 0x05, 0x06};
  std::vector<uint8_t> hash3 = {0x07, 0x08, 0x09};
  
  hashSet.add(hash1);
  hashSet.add(hash2);
  hashSet.add(hash3);
  
  EXPECT_EQ(hashSet.size(), 3);
  
  bool removed = hashSet.remove(hash2);
  
  EXPECT_TRUE(removed);
  EXPECT_EQ(hashSet.size(), 2);
  EXPECT_TRUE(hashSet.has(hash1));
  EXPECT_FALSE(hashSet.has(hash2));
  EXPECT_TRUE(hashSet.has(hash3));
}

TEST(ServerHashSet, VectorKeysRemoveMultiple)
{
  ServerHashSet<std::vector<uint8_t>, VectorHasher> hashSet;
  
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 10; i++) {
    std::vector<uint8_t> hash = {
      static_cast<uint8_t>(i),
      static_cast<uint8_t>(i + 1),
      static_cast<uint8_t>(i + 2)
    };
    hashes.push_back(hash);
    hashSet.add(hash);
  }
  
  EXPECT_EQ(hashSet.size(), 10);
  
  // Remove some hashes
  std::vector<std::vector<uint8_t>> toRemove = {
    hashes[2],
    hashes[5],
    hashes[8]
  };
  
  size_t removed = hashSet.removeMultiple(toRemove);
  
  EXPECT_EQ(removed, 3);
  EXPECT_EQ(hashSet.size(), 7);
  
  EXPECT_TRUE(hashSet.has(hashes[0]));
  EXPECT_TRUE(hashSet.has(hashes[1]));
  EXPECT_FALSE(hashSet.has(hashes[2]));
  EXPECT_TRUE(hashSet.has(hashes[3]));
}

TEST(ServerHashSet, VectorKeysDifferentLengths)
{
  ServerHashSet<std::vector<uint8_t>, VectorHasher> hashSet;
  
  std::vector<uint8_t> short_hash = {0xAA, 0xBB};
  std::vector<uint8_t> medium_hash = {0xCC, 0xDD, 0xEE, 0xFF};
  std::vector<uint8_t> long_hash = {0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88};
  
  hashSet.add(short_hash);
  hashSet.add(medium_hash);
  hashSet.add(long_hash);
  
  EXPECT_EQ(hashSet.size(), 3);
  EXPECT_TRUE(hashSet.has(short_hash));
  EXPECT_TRUE(hashSet.has(medium_hash));
  EXPECT_TRUE(hashSet.has(long_hash));
}

// ============================================================================
// Edge Cases and Stress Tests
// ============================================================================

TEST(ServerHashSet, EmptySet)
{
  ServerHashSet<uint64_t> hashSet;
  
  EXPECT_EQ(hashSet.size(), 0);
  EXPECT_FALSE(hashSet.has(0));
  EXPECT_FALSE(hashSet.has(123));
  
  auto stats = hashSet.getStats();
  EXPECT_EQ(stats.currentSize, 0);
  EXPECT_EQ(stats.totalAdded, 0);
  EXPECT_EQ(stats.totalEvicted, 0);
}

TEST(ServerHashSet, RemoveFromEmpty)
{
  ServerHashSet<uint64_t> hashSet;
  
  bool removed = hashSet.remove(123);
  
  EXPECT_FALSE(removed);
  EXPECT_EQ(hashSet.size(), 0);
}

TEST(ServerHashSet, RemoveMultipleFromEmpty)
{
  ServerHashSet<uint64_t> hashSet;
  
  std::vector<uint64_t> toRemove = {1, 2, 3, 4, 5};
  size_t removed = hashSet.removeMultiple(toRemove);
  
  EXPECT_EQ(removed, 0);
  EXPECT_EQ(hashSet.size(), 0);
}

TEST(ServerHashSet, LargeSet)
{
  ServerHashSet<uint64_t> hashSet;
  
  // Add 10,000 items
  for (uint64_t i = 1; i <= 10000; i++) {
    hashSet.add(i);
  }
  
  EXPECT_EQ(hashSet.size(), 10000);
  
  // Verify random samples
  EXPECT_TRUE(hashSet.has(1));
  EXPECT_TRUE(hashSet.has(5000));
  EXPECT_TRUE(hashSet.has(10000));
  EXPECT_FALSE(hashSet.has(10001));
  
  // Remove many items
  std::vector<uint64_t> toRemove;
  for (uint64_t i = 1; i <= 5000; i++) {
    toRemove.push_back(i);
  }
  
  size_t removed = hashSet.removeMultiple(toRemove);
  
  EXPECT_EQ(removed, 5000);
  EXPECT_EQ(hashSet.size(), 5000);
  
  // Verify correct items remain
  EXPECT_FALSE(hashSet.has(1));
  EXPECT_FALSE(hashSet.has(2500));
  EXPECT_TRUE(hashSet.has(5001));
  EXPECT_TRUE(hashSet.has(10000));
}

TEST(ServerHashSet, AddRemoveAddPattern)
{
  ServerHashSet<uint64_t> hashSet;
  
  // Add
  hashSet.add(100);
  EXPECT_TRUE(hashSet.has(100));
  
  // Remove
  hashSet.remove(100);
  EXPECT_FALSE(hashSet.has(100));
  
  // Add again
  hashSet.add(100);
  EXPECT_TRUE(hashSet.has(100));
  
  EXPECT_EQ(hashSet.size(), 1);
  
  auto stats = hashSet.getStats();
  EXPECT_EQ(stats.totalAdded, 2);  // Added twice
  EXPECT_EQ(stats.totalEvicted, 1);  // Removed once
}
