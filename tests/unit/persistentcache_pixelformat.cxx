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

// Test for PixelFormat serialization in PersistentCache
// This test verifies that PixelFormat fields survive a round-trip through
// disk serialization (save + load in separate cache instances).
//
// Bug context: Prior to the v6 index format fix, only 24 bytes of PixelFormat
// were serialized, truncating redShift/greenShift/blueShift which start at
// offset 24. This caused visual corruption when cached entries were loaded
// from disk in subsequent sessions.

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <rfb/GlobalClientPersistentCache.h>
#include <rfb/PixelFormat.h>

#include <sys/stat.h>
#include <unistd.h>
#include <cstdio>
#include <cstring>
#include <vector>

namespace {

// Helper to recursively remove a directory
static void removeDir(const std::string& path)
{
  std::string cmd = "rm -rf \"" + path + "\"";
  int ret = system(cmd.c_str());
  (void)ret;  // Ignore return value in test cleanup
}

// Create a simple test pixel pattern
static std::vector<uint8_t> createTestPixels(uint16_t width, uint16_t height,
                                              const rfb::PixelFormat& pf)
{
  size_t bpp = pf.bpp / 8;
  size_t size = width * height * bpp;
  std::vector<uint8_t> pixels(size);
  
  // Fill with a recognizable pattern
  for (size_t i = 0; i < size; i++) {
    pixels[i] = (uint8_t)(i % 256);
  }
  
  return pixels;
}

} // namespace

// Test that PixelFormat fields survive disk round-trip
TEST(PersistentCachePixelFormat, ShiftFieldsPreservedAcrossSessions)
{
  char tmpl[] = "/tmp/tigervnc_pcache_pf_test_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // Use a non-trivial pixel format with specific shift values
  // This is a typical 32bpp BGRX format (like macOS uses)
  // bpp=32, depth=24, bigEndian=false, trueColour=true
  // redMax=255, greenMax=255, blueMax=255
  // redShift=16, greenShift=8, blueShift=0
  rfb::PixelFormat testFormat(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  uint16_t testWidth = 64;
  uint16_t testHeight = 64;
  std::vector<uint8_t> testPixels = createTestPixels(testWidth, testHeight, testFormat);
  
  // Create a test hash
  std::vector<uint8_t> testHash(16);
  for (int i = 0; i < 16; i++) testHash[i] = (uint8_t)(i + 1);
  uint64_t canonicalHash = 0x123456789ABCDEF0ULL;
  uint64_t actualHash = canonicalHash;  // Lossless entry

  // Phase 1: Create cache, insert entry, save to disk
  {
    rfb::GlobalClientPersistentCache cache1(/*maxMemorySizeMB*/ 16,
                                            /*maxDiskSizeMB*/ 32,
                                            /*shardSizeMB*/ 1,
                                            /*cacheDirOverride*/ cacheDir);
    
    cache1.insert(canonicalHash, actualHash, testHash,
                  testPixels.data(), testFormat,
                  testWidth, testHeight, testWidth,
                  /*isPersistable*/ true);
    
    // Flush dirty entries to shard files
    cache1.flushDirtyEntries();
    
    // Save the index
    ASSERT_TRUE(cache1.saveToDisk());
  }
  // cache1 is now destroyed

  // Phase 2: Create new cache, load from disk, verify format
  {
    rfb::GlobalClientPersistentCache cache2(/*maxMemorySizeMB*/ 16,
                                            /*maxDiskSizeMB*/ 32,
                                            /*shardSizeMB*/ 1,
                                            /*cacheDirOverride*/ cacheDir);
    
    // Load the index from disk
    ASSERT_TRUE(cache2.loadIndexFromDisk());
    
    // Lookup the entry by canonical hash
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache2.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    
    ASSERT_NE(cached, nullptr) << "Entry not found after reload from disk";
    ASSERT_TRUE(cached->isHydrated()) << "Entry was not hydrated on lookup";
    
    // Verify the pixel format fields
    // These are the critical assertions - the bug caused shift fields to be 0
    EXPECT_EQ(cached->format.bpp, 32) << "bpp not preserved";
    EXPECT_EQ(cached->format.depth, 24) << "depth not preserved";
    EXPECT_EQ(cached->format.trueColour, true) << "trueColour not preserved";
    EXPECT_EQ(cached->format.isBigEndian(), false) << "bigEndian not preserved";
    
    // Access protected fields via the same method we use in serialization
    const uint8_t* pfRaw = reinterpret_cast<const uint8_t*>(&cached->format);
    int32_t redMax, greenMax, blueMax, redShift, greenShift, blueShift;
    memcpy(&redMax, pfRaw + 12, 4);
    memcpy(&greenMax, pfRaw + 16, 4);
    memcpy(&blueMax, pfRaw + 20, 4);
    memcpy(&redShift, pfRaw + 24, 4);
    memcpy(&greenShift, pfRaw + 28, 4);
    memcpy(&blueShift, pfRaw + 32, 4);
    
    EXPECT_EQ(redMax, 255) << "redMax not preserved";
    EXPECT_EQ(greenMax, 255) << "greenMax not preserved";
    EXPECT_EQ(blueMax, 255) << "blueMax not preserved";
    
    // These are the fields that were truncated in the buggy v5 format
    EXPECT_EQ(redShift, 16) << "redShift not preserved (was truncated in v5 format)";
    EXPECT_EQ(greenShift, 8) << "greenShift not preserved (was truncated in v5 format)";
    EXPECT_EQ(blueShift, 0) << "blueShift not preserved (was truncated in v5 format)";
    
    // Verify pixel data is also preserved
    ASSERT_EQ(cached->pixels.size(), testPixels.size()) << "Pixel data size mismatch";
    EXPECT_EQ(memcmp(cached->pixels.data(), testPixels.data(), testPixels.size()), 0)
        << "Pixel data corrupted after reload";
  }

  // Cleanup
  removeDir(cacheDir);
}

// Test with a different pixel format (RGB888 with different shifts)
TEST(PersistentCachePixelFormat, RGBFormatPreserved)
{
  char tmpl[] = "/tmp/tigervnc_pcache_pf_rgb_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // RGB format: redShift=0, greenShift=8, blueShift=16
  rfb::PixelFormat testFormat(32, 24, false, true, 255, 255, 255, 0, 8, 16);
  
  uint16_t testWidth = 32;
  uint16_t testHeight = 32;
  std::vector<uint8_t> testPixels = createTestPixels(testWidth, testHeight, testFormat);
  
  std::vector<uint8_t> testHash(16);
  for (int i = 0; i < 16; i++) testHash[i] = (uint8_t)(i + 0x10);
  uint64_t canonicalHash = 0xFEDCBA9876543210ULL;
  uint64_t actualHash = canonicalHash;

  // Phase 1: Create and save
  {
    rfb::GlobalClientPersistentCache cache1(16, 32, 1, cacheDir);
    cache1.insert(canonicalHash, actualHash, testHash,
                  testPixels.data(), testFormat,
                  testWidth, testHeight, testWidth, true);
    cache1.flushDirtyEntries();
    ASSERT_TRUE(cache1.saveToDisk());
  }

  // Phase 2: Load and verify
  {
    rfb::GlobalClientPersistentCache cache2(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache2.loadIndexFromDisk());
    
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache2.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    
    ASSERT_NE(cached, nullptr);
    
    // Verify shift values for RGB format
    const uint8_t* pfRaw = reinterpret_cast<const uint8_t*>(&cached->format);
    int32_t redShift, greenShift, blueShift;
    memcpy(&redShift, pfRaw + 24, 4);
    memcpy(&greenShift, pfRaw + 28, 4);
    memcpy(&blueShift, pfRaw + 32, 4);
    
    EXPECT_EQ(redShift, 0) << "redShift not preserved for RGB format";
    EXPECT_EQ(greenShift, 8) << "greenShift not preserved for RGB format";
    EXPECT_EQ(blueShift, 16) << "blueShift not preserved for RGB format";
  }

  removeDir(cacheDir);
}
