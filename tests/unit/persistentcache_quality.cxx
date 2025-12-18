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

// Test for quality-aware cache lookup in PersistentCache
//
// Bug context: When the cache contains entries with different pixel format
// depths (e.g., 8bpp lossy entries from bandwidth-constrained sessions and
// 32bpp lossless entries), the lookup should prefer higher-quality entries.
// Previously, an 8bpp entry could be returned when 32bpp was expected,
// causing visible quality loss during "lossless refresh" operations.
//
// The fix introduces a 3-bit quality code in the index:
//   Bit 0: Lossy flag (0=lossless, 1=lossy)
//   Bits 1-2: Color depth (00=8bpp, 01=16bpp, 10=24/32bpp, 11=reserved)

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>

#include <rfb/GlobalClientPersistentCache.h>
#include <rfb/PixelFormat.h>
#include <rfb/PixelBuffer.h>

#include <sys/stat.h>
#include <unistd.h>
#include <cstdio>
#include <cstring>
#include <vector>

namespace {

static void removeDir(const std::string& path)
{
  std::string cmd = "rm -rf \"" + path + "\"";
  int ret = system(cmd.c_str());
  (void)ret;
}

// Helper to compute a simple content hash (not the real one, just for testing)
static uint64_t simpleHash(const uint8_t* data, size_t len) {
  uint64_t h = 0xcbf29ce484222325ULL;
  for (size_t i = 0; i < len; i++) {
    h ^= data[i];
    h *= 0x100000001b3ULL;
  }
  return h;
}

} // namespace

// Test that when cache has ONLY an 8bpp entry but viewer needs 32bpp quality,
// the lookup should return nullptr so the client requests fresh data from server.
//
// Scenario:
// - Previous session: bandwidth-constrained, viewer at 8bpp, stored 8bpp entries
// - Current session: full bandwidth, viewer at 32bpp
// - Server sends PersistentCachedRect (canonical hash matches an 8bpp entry)
// - BUG: Viewer retrieves 8bpp data, converts to 32bpp = quality loss
// - FIX: Lookup with minBpp=32 returns nullptr, viewer requests fresh 32bpp data
//
// The fix adds minBpp parameter to getByCanonicalHash to filter by quality.
TEST(PersistentCacheQuality, RejectLowQualityWhenHighNeeded)
{
  char tmpl[] = "/tmp/tigervnc_pcache_qual_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // Only 8bpp format available in cache
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Create 8bpp pixels
  std::vector<uint8_t> pixels8(testWidth * testHeight);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels8[i] = 0xE0;  // Red in rgb332
  }
  
  uint64_t canonicalHash = 0xCAFEBABE12345678ULL;
  uint64_t actualHash8 = simpleHash(pixels8.data(), pixels8.size());
  
  std::vector<uint8_t> hash8(16);
  memcpy(hash8.data(), &actualHash8, 8);

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    
    // Insert only 8bpp entry (from previous bandwidth-constrained session)
    cache.insert(canonicalHash, actualHash8, hash8,
                 pixels8.data(), pf8,
                 testWidth, testHeight, testWidth, true);
    
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  // Reload and test filtered lookups
  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    // Lookup with no filter should return 8bpp entry
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    ASSERT_NE(cached, nullptr) << "Unfiltered lookup should find entry";
    EXPECT_EQ(cached->format.bpp, 8);
    
    // Lookup requiring 8bpp minimum should return 8bpp entry
    cached = cache.getByCanonicalHash(canonicalHash, testWidth, testHeight, 8);
    ASSERT_NE(cached, nullptr) << "8bpp filter should match 8bpp entry";
    EXPECT_EQ(cached->format.bpp, 8);
    
    // KEY TEST: Lookup requiring 32bpp minimum should REJECT 8bpp entry
    // This prevents quality loss when upscaling from low to high quality
    cached = cache.getByCanonicalHash(canonicalHash, testWidth, testHeight, 32);
    EXPECT_EQ(cached, nullptr) 
        << "32bpp filter should reject 8bpp entry - upscaling causes quality loss";
  }

  removeDir(cacheDir);
}

// Test that when both 8bpp and 32bpp entries exist for the same canonical hash,
// the lookup prefers the higher-quality 32bpp entry even without minBpp filter.
TEST(PersistentCacheQuality, PreferHigherQualityWhenBothExist)
{
  char tmpl[] = "/tmp/tigervnc_pcache_qual_both_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // 32bpp pixels
  std::vector<uint8_t> pixels32(testWidth * testHeight * 4);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels32[i * 4 + 0] = 0x00;
    pixels32[i * 4 + 1] = 0x00;
    pixels32[i * 4 + 2] = 0xFF;
    pixels32[i * 4 + 3] = 0x00;
  }
  
  // 8bpp pixels
  std::vector<uint8_t> pixels8(testWidth * testHeight);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels8[i] = 0xE0;
  }
  
  // Same canonical hash
uint64_t canonicalHash = 0xB07E4711E5123456ULL;
  uint64_t actualHash32 = simpleHash(pixels32.data(), pixels32.size());
  uint64_t actualHash8 = simpleHash(pixels8.data(), pixels8.size());
  
  std::vector<uint8_t> hash32(16), hash8(16);
  memcpy(hash32.data(), &actualHash32, 8);
  memcpy(hash8.data(), &actualHash8, 8);

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    
    // Insert 8bpp first, then 32bpp
    cache.insert(canonicalHash, actualHash8, hash8,
                 pixels8.data(), pf8,
                 testWidth, testHeight, testWidth, true);
    cache.insert(canonicalHash, actualHash32, hash32,
                 pixels32.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  // Reload and verify higher quality is preferred
  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    // Unfiltered lookup should prefer 32bpp over 8bpp
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    ASSERT_NE(cached, nullptr);
    EXPECT_EQ(cached->format.bpp, 32) 
        << "Should prefer 32bpp entry when both exist";
  }

  removeDir(cacheDir);
}

// Test that 8bpp entries are still returned when no better option exists
TEST(PersistentCacheQuality, Use8bppWhenNoBetterOption)
{
  char tmpl[] = "/tmp/tigervnc_pcache_qual8_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  std::vector<uint8_t> pixels8(testWidth * testHeight);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels8[i] = 0xE0;
  }
  
  uint64_t canonicalHash = 0xDEADBEEF87654321ULL;
  uint64_t actualHash8 = simpleHash(pixels8.data(), pixels8.size());
  
  std::vector<uint8_t> hash8(16);
  memcpy(hash8.data(), &actualHash8, 8);

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    cache.insert(canonicalHash, actualHash8, hash8,
                 pixels8.data(), pf8,
                 testWidth, testHeight, testWidth, true);
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    
    // Should still return the 8bpp entry when it's the only option
    ASSERT_NE(cached, nullptr) << "Cache lookup should return 8bpp entry";
    EXPECT_EQ(cached->format.bpp, 8);
  }

  removeDir(cacheDir);
}

// Test that among same-depth entries, lossless is preferred over lossy
TEST(PersistentCacheQuality, PreferLosslessOverLossySameDepth)
{
  char tmpl[] = "/tmp/tigervnc_pcache_qual_ll_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Two slightly different 32bpp pixel sets
  std::vector<uint8_t> pixelsLossless(testWidth * testHeight * 4);
  std::vector<uint8_t> pixelsLossy(testWidth * testHeight * 4);
  
  for (int i = 0; i < testWidth * testHeight; i++) {
    // Lossless: pure red
    pixelsLossless[i * 4 + 0] = 0x00;
    pixelsLossless[i * 4 + 1] = 0x00;
    pixelsLossless[i * 4 + 2] = 0xFF;
    pixelsLossless[i * 4 + 3] = 0x00;
    
    // Lossy: slightly different red (JPEG artifact simulation)
    pixelsLossy[i * 4 + 0] = 0x02;
    pixelsLossy[i * 4 + 1] = 0x01;
    pixelsLossy[i * 4 + 2] = 0xFE;
    pixelsLossy[i * 4 + 3] = 0x00;
  }
  
  uint64_t canonicalHash = 0x1122334455667788ULL;
  uint64_t actualHashLossy = simpleHash(pixelsLossy.data(), pixelsLossy.size());
  
  std::vector<uint8_t> hashLossless(16), hashLossy(16);
  memcpy(hashLossless.data(), &canonicalHash, 8);  // actual == canonical
  memcpy(hashLossy.data(), &actualHashLossy, 8);

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    
    // Insert lossy entry first
    cache.insert(canonicalHash, actualHashLossy, hashLossy,
                 pixelsLossy.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    
    // Insert lossless entry second
    cache.insert(canonicalHash, canonicalHash, hashLossless,
                 pixelsLossless.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    
    ASSERT_NE(cached, nullptr);
    
    // Should prefer lossless (actual == canonical)
    EXPECT_TRUE(cached->isLossless()) 
        << "Expected lossless entry (actual==canonical) but got lossy";
    
    // Verify it's the lossless pixel data (pure red, not JPEG-artifacted)
    EXPECT_EQ(cached->pixels[2], 0xFF) << "Expected pure red (0xFF)";
    EXPECT_EQ(cached->pixels[0], 0x00) << "Expected no blue";
  }

  removeDir(cacheDir);
}

// Test that qualityCode is correctly computed and persisted in v7 format
TEST(PersistentCacheQuality, QualityCodePersistence)
{
  char tmpl[] = "/tmp/tigervnc_pcache_qc_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // Test different combinations of bpp and lossy/lossless
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  rfb::PixelFormat pf16(16, 16, false, true, 31, 63, 31, 11, 5, 0);
  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Create test pixels
  std::vector<uint8_t> pixels8(testWidth * testHeight);
  std::vector<uint8_t> pixels16(testWidth * testHeight * 2);
  std::vector<uint8_t> pixels32(testWidth * testHeight * 4);
  
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels8[i] = 0xE0;
    pixels16[i * 2] = 0x00;
    pixels16[i * 2 + 1] = 0xF8;
    pixels32[i * 4 + 0] = 0x00;
    pixels32[i * 4 + 1] = 0x00;
    pixels32[i * 4 + 2] = 0xFF;
    pixels32[i * 4 + 3] = 0x00;
  }
  
  // Different canonical hashes
  uint64_t canon8 = 0x1111111111111111ULL;
  uint64_t canon16 = 0x2222222222222222ULL;
  uint64_t canon32lossless = 0x3333333333333333ULL;
  uint64_t canon32lossy = 0x4444444444444444ULL;
  
  uint64_t actual8 = simpleHash(pixels8.data(), pixels8.size());
  uint64_t actual16 = simpleHash(pixels16.data(), pixels16.size());
  uint64_t actual32 = simpleHash(pixels32.data(), pixels32.size());
  
  std::vector<uint8_t> hash8(16), hash16(16), hash32ll(16), hash32lossy(16);
  memcpy(hash8.data(), &actual8, 8);
  memcpy(hash16.data(), &actual16, 8);
  memcpy(hash32ll.data(), &canon32lossless, 8);  // lossless: actual==canonical
  memcpy(hash32lossy.data(), &actual32, 8);

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    
    // 8bpp lossy (actual != canonical)
    cache.insert(canon8, actual8, hash8, pixels8.data(), pf8,
                 testWidth, testHeight, testWidth, true);
    
    // 16bpp lossy
    cache.insert(canon16, actual16, hash16, pixels16.data(), pf16,
                 testWidth, testHeight, testWidth, true);
    
    // 32bpp lossless (actual == canonical)
    cache.insert(canon32lossless, canon32lossless, hash32ll, pixels32.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    
    // 32bpp lossy
    cache.insert(canon32lossy, actual32, hash32lossy, pixels32.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  // Reload and verify quality filtering works correctly
  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    // 8bpp entry should be found with no filter or minBpp=8
    auto cached = cache.getByCanonicalHash(canon8, testWidth, testHeight);
    ASSERT_NE(cached, nullptr) << "Should find 8bpp entry";
    EXPECT_EQ(cached->format.bpp, 8);
    
    // 8bpp entry should be rejected with minBpp=16 or minBpp=32
    cached = cache.getByCanonicalHash(canon8, testWidth, testHeight, 16);
    EXPECT_EQ(cached, nullptr) << "8bpp should be rejected with minBpp=16";
    
    cached = cache.getByCanonicalHash(canon8, testWidth, testHeight, 32);
    EXPECT_EQ(cached, nullptr) << "8bpp should be rejected with minBpp=32";
    
    // 16bpp entry should be found with minBpp<=16, rejected with minBpp=32
    cached = cache.getByCanonicalHash(canon16, testWidth, testHeight, 16);
    ASSERT_NE(cached, nullptr) << "Should find 16bpp entry with minBpp=16";
    EXPECT_EQ(cached->format.bpp, 16);
    
    cached = cache.getByCanonicalHash(canon16, testWidth, testHeight, 32);
    EXPECT_EQ(cached, nullptr) << "16bpp should be rejected with minBpp=32";
    
    // 32bpp entries should be found with any minBpp<=32
    cached = cache.getByCanonicalHash(canon32lossless, testWidth, testHeight, 32);
    ASSERT_NE(cached, nullptr) << "Should find 32bpp lossless entry";
    EXPECT_EQ(cached->format.bpp, 32);
    EXPECT_TRUE(cached->isLossless());
    
    cached = cache.getByCanonicalHash(canon32lossy, testWidth, testHeight, 32);
    ASSERT_NE(cached, nullptr) << "Should find 32bpp lossy entry";
    EXPECT_EQ(cached->format.bpp, 32);
    EXPECT_FALSE(cached->isLossless());
  }

  removeDir(cacheDir);
}
