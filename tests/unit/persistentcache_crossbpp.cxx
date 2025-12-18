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

// Test for cross-bpp cache hit conversion in PersistentCache
// This test verifies that when a cache entry was stored with one pixel format
// (e.g., 32bpp) and is retrieved for use with a different framebuffer format
// (e.g., 8bpp), the conversion happens correctly.
//
// Bug context: The lazy lossless refresh can cause visual corruption when
// cached entries from a 32bpp session are retrieved during an 8bpp session.

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

// Helper to recursively remove a directory
static void removeDir(const std::string& path)
{
  std::string cmd = "rm -rf \"" + path + "\"";
  int ret = system(cmd.c_str());
  (void)ret;
}

} // namespace

// Test that 32bpp cache entries can be correctly blitted to 8bpp framebuffer
TEST(PersistentCacheCrossBpp, Cache32bppBlitTo8bpp)
{
  char tmpl[] = "/tmp/tigervnc_pcache_xbpp_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // 32bpp BGRX format (typical for cached entries)
  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // 8bpp rgb332 format (what the viewer might use in low-color mode)
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Create 32bpp test pixels with known colors
  // Each pixel: 4 bytes BGRX (little-endian: B, G, R, X)
  // Let's use pure red (R=255, G=0, B=0) -> BGRX = 00 00 FF 00
  std::vector<uint8_t> pixels32(testWidth * testHeight * 4);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels32[i * 4 + 0] = 0x00;  // B
    pixels32[i * 4 + 1] = 0x00;  // G
    pixels32[i * 4 + 2] = 0xFF;  // R
    pixels32[i * 4 + 3] = 0x00;  // X
  }
  
  std::vector<uint8_t> testHash(16);
  for (int i = 0; i < 16; i++) testHash[i] = (uint8_t)(i + 0x20);
  uint64_t canonicalHash = 0xABCDEF0123456789ULL;
  uint64_t actualHash = canonicalHash;

  // Phase 1: Store 32bpp entry
  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    cache.insert(canonicalHash, actualHash, testHash,
                 pixels32.data(), pf32,
                 testWidth, testHeight, testWidth, true);
    cache.flushDirtyEntries();
    ASSERT_TRUE(cache.saveToDisk());
  }

  // Phase 2: Load and blit to 8bpp framebuffer
  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    ASSERT_TRUE(cache.loadIndexFromDisk());
    
    const rfb::GlobalClientPersistentCache::CachedPixels* cached =
        cache.getByCanonicalHash(canonicalHash, testWidth, testHeight);
    
    ASSERT_NE(cached, nullptr);
    ASSERT_TRUE(cached->isHydrated());
    
    // Verify stored format is 32bpp
    EXPECT_EQ(cached->format.bpp, 32) << "Cached entry should be 32bpp";
    
    // Create 8bpp framebuffer and blit the cached pixels to it
    rfb::ManagedPixelBuffer fb8(pf8, testWidth, testHeight);
    core::Rect r(0, 0, testWidth, testHeight);
    
    // This is the key operation: convert 32bpp cached pixels to 8bpp fb
    fb8.imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
    
    // Verify the conversion: pure red in rgb332 should be 111 000 00 = 0xE0
    int stride;
    const uint8_t* result = fb8.getBuffer(r, &stride);
    
    // Check first pixel
    uint8_t expected8 = 0xE0;  // Red = 7 (max for 3 bits), G=0, B=0
    EXPECT_EQ(result[0], expected8) 
        << "First pixel mismatch: got 0x" << std::hex << (int)result[0]
        << ", expected 0x" << (int)expected8;
    
    // Check all pixels
    for (int i = 0; i < testWidth * testHeight; i++) {
      EXPECT_EQ(result[i], expected8) 
          << "Pixel " << i << " mismatch: got 0x" << std::hex << (int)result[i]
          << ", expected 0x" << (int)expected8;
    }
  }

  removeDir(cacheDir);
}

// Test with green color
TEST(PersistentCacheCrossBpp, Cache32bppGreenBlitTo8bpp)
{
  char tmpl[] = "/tmp/tigervnc_pcache_xbpp_g_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Pure green (R=0, G=255, B=0) -> BGRX = 00 FF 00 00
  std::vector<uint8_t> pixels32(testWidth * testHeight * 4);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels32[i * 4 + 0] = 0x00;  // B
    pixels32[i * 4 + 1] = 0xFF;  // G
    pixels32[i * 4 + 2] = 0x00;  // R
    pixels32[i * 4 + 3] = 0x00;  // X
  }
  
  std::vector<uint8_t> testHash(16);
  for (int i = 0; i < 16; i++) testHash[i] = (uint8_t)(i + 0x30);
  uint64_t canonicalHash = 0x1234567890ABCDEFULL;

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    cache.insert(canonicalHash, canonicalHash, testHash,
                 pixels32.data(), pf32,
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
    
    rfb::ManagedPixelBuffer fb8(pf8, testWidth, testHeight);
    core::Rect r(0, 0, testWidth, testHeight);
    fb8.imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
    
    int stride;
    const uint8_t* result = fb8.getBuffer(r, &stride);
    
    // Green in rgb332: R=0 (000), G=7 (111), B=0 (00) = 000 111 00 = 0x1C
    uint8_t expected8 = 0x1C;
    EXPECT_EQ(result[0], expected8) 
        << "Green pixel mismatch: got 0x" << std::hex << (int)result[0]
        << ", expected 0x" << (int)expected8;
  }

  removeDir(cacheDir);
}

// Test with blue color  
TEST(PersistentCacheCrossBpp, Cache32bppBlueBlitTo8bpp)
{
  char tmpl[] = "/tmp/tigervnc_pcache_xbpp_b_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  rfb::PixelFormat pf32(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  rfb::PixelFormat pf8(8, 8, false, true, 7, 7, 3, 5, 2, 0);
  
  uint16_t testWidth = 4;
  uint16_t testHeight = 4;
  
  // Pure blue (R=0, G=0, B=255) -> BGRX = FF 00 00 00
  std::vector<uint8_t> pixels32(testWidth * testHeight * 4);
  for (int i = 0; i < testWidth * testHeight; i++) {
    pixels32[i * 4 + 0] = 0xFF;  // B
    pixels32[i * 4 + 1] = 0x00;  // G
    pixels32[i * 4 + 2] = 0x00;  // R
    pixels32[i * 4 + 3] = 0x00;  // X
  }
  
  std::vector<uint8_t> testHash(16);
  for (int i = 0; i < 16; i++) testHash[i] = (uint8_t)(i + 0x40);
  uint64_t canonicalHash = 0xFEDCBA0987654321ULL;

  {
    rfb::GlobalClientPersistentCache cache(16, 32, 1, cacheDir);
    cache.insert(canonicalHash, canonicalHash, testHash,
                 pixels32.data(), pf32,
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
    
    rfb::ManagedPixelBuffer fb8(pf8, testWidth, testHeight);
    core::Rect r(0, 0, testWidth, testHeight);
    fb8.imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
    
    int stride;
    const uint8_t* result = fb8.getBuffer(r, &stride);
    
    // Blue in rgb332: R=0 (000), G=0 (000), B=3 (11) = 000 000 11 = 0x03
    uint8_t expected8 = 0x03;
    EXPECT_EQ(result[0], expected8) 
        << "Blue pixel mismatch: got 0x" << std::hex << (int)result[0]
        << ", expected 0x" << (int)expected8;
  }

  removeDir(cacheDir);
}
