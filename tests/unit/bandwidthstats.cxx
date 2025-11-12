/* Copyright (C) 2026 TigerVNC Team
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

#include <rfb/cache/BandwidthStats.h>
#include <rfb/PixelFormat.h>
#include <core/Rect.h>

using namespace rfb::cache;

// ============================================================================
// ContentCache Tracking
// ============================================================================

TEST(BandwidthStats, ContentCacheRefBasic)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  core::Rect rect(0, 0, 64, 64);
  
  trackContentCacheRef(stats, rect, pf);
  
  // ContentCache reference: 20 bytes total (12 header + 8 ID)
  EXPECT_EQ(stats.cachedRectBytes, 20);
  EXPECT_EQ(stats.cachedRectCount, 1);
  
  // Alternative: 16 overhead + estimated compressed (pixels * bpp / 8 / 10)
  uint32_t pixels = 64 * 64;
  uint32_t bytesPerPixel = pf.bpp / 8;
  uint32_t estimated = 16 + (pixels * bytesPerPixel) / 10;
  EXPECT_EQ(stats.alternativeBytes, estimated);
}

TEST(BandwidthStats, ContentCacheRefMultiple)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  trackContentCacheRef(stats, core::Rect(0, 0, 64, 64), pf);
  trackContentCacheRef(stats, core::Rect(64, 0, 128, 64), pf);
  trackContentCacheRef(stats, core::Rect(0, 64, 64, 128), pf);
  
  EXPECT_EQ(stats.cachedRectBytes, 60);  // 20 * 3
  EXPECT_EQ(stats.cachedRectCount, 3);
  
  // Each rect: 16 + (64*64*4)/10 = 16 + 1638 = 1654, times 3
  EXPECT_EQ(stats.alternativeBytes, 3 * (16 + (64 * 64 * 4) / 10));
}

TEST(BandwidthStats, ContentCacheInit)
{
  CacheProtocolStats stats;
  
  // Simulate init with 1024 bytes of compressed data
  trackContentCacheInit(stats, 1024);
  
  // Init: 24 byte overhead (12 header + 8 cacheId + 4 encoding) + compressed
  EXPECT_EQ(stats.cachedRectInitBytes, 1048);  // 24 + 1024
  EXPECT_EQ(stats.cachedRectInitCount, 1);
}

TEST(BandwidthStats, ContentCacheMixed)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // 5 refs, 2 inits
  for (int i = 0; i < 5; i++) {
    trackContentCacheRef(stats, core::Rect(0, 0, 64, 64), pf);
  }
  trackContentCacheInit(stats, 800);
  trackContentCacheInit(stats, 1200);
  
  EXPECT_EQ(stats.cachedRectBytes, 100);         // 5 * 20
  EXPECT_EQ(stats.cachedRectInitBytes, 2048);    // 24+800 + 24+1200
  EXPECT_EQ(stats.cachedRectCount, 5);
  EXPECT_EQ(stats.cachedRectInitCount, 2);
}

// ============================================================================
// PersistentCache Tracking
// ============================================================================

TEST(BandwidthStats, PersistentCacheRefBasic)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  core::Rect rect(0, 0, 64, 64);
  
  trackPersistentCacheRef(stats, rect, pf, 32);  // SHA-256 hash
  
  // PersistentCachedRect: 12 header + 1 hashLen + 32 hash = 45 bytes
  EXPECT_EQ(stats.cachedRectBytes, 45);
  EXPECT_EQ(stats.cachedRectCount, 1);
  
  // Alternative: 16 + (64*64*4)/10
  EXPECT_EQ(stats.alternativeBytes, 16 + (64 * 64 * 4) / 10);
}

TEST(BandwidthStats, PersistentCacheRefVariableHash)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  core::Rect rect(0, 0, 64, 64);
  
  // Different hash lengths
  trackPersistentCacheRef(stats, rect, pf, 16);  // MD5
  trackPersistentCacheRef(stats, rect, pf, 32);  // SHA-256
  trackPersistentCacheRef(stats, rect, pf, 64);  // SHA-512
  
  // 12+1+16=29, 12+1+32=45, 12+1+64=77 → total 151
  EXPECT_EQ(stats.cachedRectBytes, 151);
  EXPECT_EQ(stats.cachedRectCount, 3);
}

TEST(BandwidthStats, PersistentCacheInit)
{
  CacheProtocolStats stats;
  
  trackPersistentCacheInit(stats, 32, 1024);  // SHA-256, 1KB payload
  
  // PersistentCachedRectInit: 12+1+32+4 overhead + 1024 payload = 1073
  EXPECT_EQ(stats.cachedRectInitBytes, 1073);
  EXPECT_EQ(stats.cachedRectInitCount, 1);
}

TEST(BandwidthStats, PersistentCacheMixed)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // 3 refs, 1 init
  trackPersistentCacheRef(stats, core::Rect(0, 0, 64, 64), pf, 32);
  trackPersistentCacheRef(stats, core::Rect(64, 0, 128, 64), pf, 32);
  trackPersistentCacheRef(stats, core::Rect(0, 64, 64, 128), pf, 32);
  trackPersistentCacheInit(stats, 32, 512);
  
  EXPECT_EQ(stats.cachedRectBytes, 135);        // 3 * 45
  EXPECT_EQ(stats.cachedRectInitBytes, 561);    // 12+1+32+4+512
  EXPECT_EQ(stats.cachedRectCount, 3);
  EXPECT_EQ(stats.cachedRectInitCount, 1);
}

// ============================================================================
// Savings Calculations
// ============================================================================

TEST(BandwidthStats, SavingsBasic)
{
  CacheProtocolStats stats;
  stats.alternativeBytes = 1000000;      // 1 MB baseline
  stats.cachedRectBytes = 5000;          // 5 KB refs
  stats.cachedRectInitBytes = 45000;     // 45 KB inits
  
  // Saved: 1000000 - 50000 = 950000
  EXPECT_EQ(stats.bandwidthSaved(), 950000);
  
  // Reduction: 95%
  EXPECT_NEAR(stats.reductionPercentage(), 95.0, 0.01);
}

TEST(BandwidthStats, SavingsZeroBaseline)
{
  CacheProtocolStats stats;
  stats.alternativeBytes = 0;
  stats.cachedRectBytes = 1000;
  
  EXPECT_EQ(stats.bandwidthSaved(), 0);
  EXPECT_EQ(stats.reductionPercentage(), 0.0);
}

TEST(BandwidthStats, SavingsNegative)
{
  CacheProtocolStats stats;
  // Cache overhead exceeds baseline (pathological case)
  stats.alternativeBytes = 100;
  stats.cachedRectBytes = 50;
  stats.cachedRectInitBytes = 100;
  
  EXPECT_EQ(stats.bandwidthSaved(), 0);
  EXPECT_EQ(stats.reductionPercentage(), 0.0);
}

TEST(BandwidthStats, SavingsHighHitRate)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // Simulate high hit rate: 100 refs, 5 inits
  for (int i = 0; i < 100; i++) {
    trackContentCacheRef(stats, core::Rect(0, 0, 64, 64), pf);
  }
  for (int i = 0; i < 5; i++) {
    trackContentCacheInit(stats, 800);
  }
  
  // Refs: 100 * 20 = 2000
  // Inits: 5 * 824 = 4120
  // Total used: 6120
  // Alternative is estimated compressed, much larger
  
  // Just verify we have significant savings (>90%)
  EXPECT_GT(stats.reductionPercentage(), 90.0);
}

// ============================================================================
// Format Summary
// ============================================================================

TEST(BandwidthStats, FormatSummary)
{
  CacheProtocolStats stats;
  stats.alternativeBytes = 52428800;     // 50 MB
  stats.cachedRectBytes = 1048576;       // 1 MB
  stats.cachedRectInitBytes = 4194304;   // 4 MB
  
  std::string summary = stats.formatSummary("TestCache");
  
  // Should contain cache name
  EXPECT_NE(summary.find("TestCache"), std::string::npos);
  
  // Should show MiB (megabytes)
  EXPECT_NE(summary.find("MiB"), std::string::npos);
  
  // Should show percentage
  EXPECT_NE(summary.find("%"), std::string::npos);
  
  // 90% reduction: (50 - 5) / 50 = 0.9
  EXPECT_NE(summary.find("90"), std::string::npos);
}

TEST(BandwidthStats, FormatSummarySmall)
{
  CacheProtocolStats stats;
  stats.alternativeBytes = 10000;        // 10 KB
  stats.cachedRectBytes = 1000;          // 1 KB
  stats.cachedRectInitBytes = 2000;      // 2 KB
  
  std::string summary = stats.formatSummary();
  
  // Should show KiB for small values
  EXPECT_NE(summary.find("KiB"), std::string::npos);
  
  // 70% reduction
  EXPECT_NE(summary.find("70"), std::string::npos);
}

TEST(BandwidthStats, FormatSummaryNoSavings)
{
  CacheProtocolStats stats;
  stats.alternativeBytes = 1000;
  stats.cachedRectBytes = 0;
  stats.cachedRectInitBytes = 0;
  
  std::string summary = stats.formatSummary();
  
  // Should handle zero gracefully
  EXPECT_FALSE(summary.empty());
}

// ============================================================================
// Realistic Scenarios
// ============================================================================

TEST(BandwidthStats, RealisticContentCacheWorkload)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // Simulate typical workload:
  // - 200 cache hits (refs)
  // - 20 cache misses (inits)
  // - Mix of rect sizes
  
  for (int i = 0; i < 200; i++) {
    int size = 32 + (i % 128);  // Variable sizes 32-159
    trackContentCacheRef(stats, core::Rect(0, 0, size, size), pf);
  }
  
  for (int i = 0; i < 20; i++) {
    trackContentCacheInit(stats, 500 + i * 50);  // Variable compressed sizes
  }
  
  // Should achieve >95% savings with 90% hit rate
  EXPECT_GT(stats.reductionPercentage(), 95.0);
  EXPECT_EQ(stats.cachedRectCount, 200);
  EXPECT_EQ(stats.cachedRectInitCount, 20);
}

TEST(BandwidthStats, RealisticPersistentCacheWorkload)
{
  CacheProtocolStats stats;
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // Persistent cache with cross-session hits
  // - 500 refs (many from previous sessions)
  // - 10 inits (only new content)
  
  for (int i = 0; i < 500; i++) {
    trackPersistentCacheRef(stats, core::Rect(0, 0, 64, 64), pf, 32);
  }
  
  for (int i = 0; i < 10; i++) {
    trackPersistentCacheInit(stats, 32, 1000);
  }
  
  // Very high hit rate (98%) should achieve >90% bandwidth savings
  EXPECT_GT(stats.reductionPercentage(), 90.0);
  EXPECT_EQ(stats.cachedRectCount, 500);
  EXPECT_EQ(stats.cachedRectInitCount, 10);
}
