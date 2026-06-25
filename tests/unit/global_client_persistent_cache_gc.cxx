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

#include <rfb/GlobalClientPersistentCache.h>
#include <rfb/PixelFormat.h>

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

namespace {

static void writeFileWithSize(const char* path, size_t size) {
  int fd = ::open(path, O_CREAT | O_TRUNC | O_WRONLY, 0644);
  ASSERT_GE(fd, 0);
  // Ensure the file has the desired size; contents don't matter for the test.
  ASSERT_EQ(::ftruncate(fd, (off_t)size), 0);
  ::close(fd);
}

static void writeEmptyV5Index(const std::string& dir) {
  std::string indexPath = dir + "/index.dat";
  FILE* f = fopen(indexPath.c_str(), "wb");
  ASSERT_NE(f, nullptr);

  struct IndexHeader {
    uint32_t magic;
    uint32_t version;
    uint64_t entryCount;
    uint64_t created;
    uint64_t lastAccess;
    uint16_t maxShardId;
    uint8_t reserved[30];
  } header;

  memset(&header, 0, sizeof(header));
  header.magic = 0x50435633; // "PCV3"
  header.version = 5;
  header.entryCount = 0;
  header.maxShardId = 0;

  ASSERT_EQ(fwrite(&header, sizeof(header), 1, f), 1u);
  fclose(f);
}

static bool fileExists(const std::string& path) {
  struct stat st;
  return stat(path.c_str(), &st) == 0;
}

// Build a unique 16-byte protocol hash from a small integer.
static std::vector<uint8_t> makeHash(uint64_t id) {
  std::vector<uint8_t> hash(16, 0);
  // First 8 bytes carry the id (used as the CacheKey identity); fill the
  // remainder so distinct ids never collide.
  memcpy(hash.data(), &id, sizeof(id));
  uint64_t mixed = id * 0x9E3779B97F4A7C15ULL + 0x1234567ULL;
  memcpy(hash.data() + 8, &mixed, sizeof(mixed));
  return hash;
}

// Distinct, verifiable pixel pattern for entry `id`.
static std::vector<uint8_t> makePixels(uint64_t id, size_t pixelCount) {
  std::vector<uint8_t> px(pixelCount * 4);
  uint8_t base = (uint8_t)(id * 7 + 1);
  for (size_t i = 0; i < px.size(); i++)
    px[i] = (uint8_t)(base + (uint8_t)i);
  return px;
}

static void removeDirRecursive(const std::string& path) {
  std::string cmd = "rm -rf \"" + path + "\"";
  int ret = system(cmd.c_str());
  (void)ret;
}

} // namespace

TEST(GlobalClientPersistentCache, LoadIndexDeletesOrphanShards) {
  char tmpl[] = "/tmp/tigervnc_pcache_test_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);

  std::string cacheDir(dir);

  // Create orphan shard files (not referenced by the index).
  writeFileWithSize((cacheDir + "/shard_0000.dat").c_str(), 1024 * 1024);
  writeFileWithSize((cacheDir + "/shard_0001.dat").c_str(), 1024 * 1024);

  // Create an empty v5 index (0 entries). The correct behavior is to delete
  // any shard files that are not referenced by the index.
  writeEmptyV5Index(cacheDir);

  ASSERT_TRUE(fileExists(cacheDir + "/shard_0000.dat"));
  ASSERT_TRUE(fileExists(cacheDir + "/shard_0001.dat"));

  rfb::GlobalClientPersistentCache cache(/*maxMemorySizeMB*/ 1,
                                         /*maxDiskSizeMB*/ 1,
                                         /*shardSizeMB*/ 1,
                                         /*cacheDirOverride*/ cacheDir);

  cache.loadIndexFromDisk();

  EXPECT_FALSE(fileExists(cacheDir + "/shard_0000.dat"));
  EXPECT_FALSE(fileExists(cacheDir + "/shard_0001.dat"));
  EXPECT_TRUE(fileExists(cacheDir + "/index.dat"));

  // Cleanup temp directory.
  remove((cacheDir + "/index.dat").c_str());
  rmdir(cacheDir.c_str());
}

// Regression test for the "wedged cache" bug.
//
// Once the on-disk cache fills to its limit, garbageCollect() must be able to
// reclaim space. The original implementation only deleted shard files whose
// entries were *all* cold. In a real workload, hot (recently accessed) and
// cold (evicted) entries are interleaved across shards, so no shard is ever
// fully cold and GC reclaimed nothing. The cache then stayed pinned at the
// limit forever, re-running a no-op GC on every cached rect and crippling the
// refresh rate.
//
// This test builds exactly that state -- every shard contains a mix of hot and
// cold entries, with total disk usage at the limit -- and asserts that GC
// reclaims space, brings usage under the limit, preserves the live entries
// (with correct pixel data after relocation), and drops cold ones.
TEST(GlobalClientPersistentCache, GcReclaimsPartiallyColdShards) {
  char tmpl[] = "/tmp/tigervnc_pcache_gc_XXXXXX";
  char* dir = mkdtemp(tmpl);
  ASSERT_NE(dir, nullptr);
  std::string cacheDir(dir);

  // 32bpp little-endian RGBX.
  rfb::PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);

  // ~128 KiB per entry: 256x128 * 4 bytes.
  const uint16_t W = 256, H = 128;
  const size_t pixelCount = (size_t)W * H;
  const size_t entryBytes = pixelCount * 4;

  // 40 entries of 128 KiB = 5 MiB across 5 shards of 1 MiB (8 entries each).
  // Memory holds only 16 entries (2 MiB), so sustained inserts evict older
  // entries to "cold" (still on disk). Disk limit is exactly the working set,
  // so the cache ends up full.
  const int kEntries = 40;
  const int kPerShard = 8;
  const size_t diskLimit = 5u * 1024 * 1024;

  // One representative live entry pinned hot in every shard. This guarantees no
  // shard is fully cold (so phase-1 whole-shard deletion cannot help) and gives
  // us known-good entries to verify survive and stay intact across compaction.
  const int pinned[] = {0, 8, 16, 24, 32};

  auto insertEntry = [&](rfb::GlobalClientPersistentCache& cache, int i) {
    std::vector<uint8_t> hash = makeHash(i);
    std::vector<uint8_t> px = makePixels(i, pixelCount);
    uint64_t h64;
    memcpy(&h64, hash.data(), sizeof(h64));
    cache.insert(h64, h64, hash, px.data(), pf, W, H, W, true); // lossless
  };

  rfb::GlobalClientPersistentCache cache(/*memMB*/ 2, /*diskMB*/ 5,
                                         /*shardMB*/ 1, cacheDir);

  // Insert in batches, flushing each batch to disk *before* the next batch
  // evicts it from memory. An entry is only marked cold once it is on disk, so
  // this ordering is what produces genuine cold-on-disk entries.
  int next = 0;
  for (int batch = 0; batch < kEntries; batch += kPerShard * 2) {
    int end = std::min(batch + kPerShard * 2, kEntries);
    for (; next < end; next++)
      insertEntry(cache, next);
    ASSERT_GT(cache.flushDirtyEntries(), 0u);
  }
  ASSERT_TRUE(cache.saveToDisk());

  // Sustained inserts should have evicted older entries to cold.
  ASSERT_GT(cache.getColdEntryCount(), 0u) << "setup failed to create cold entries";

  const size_t usageBefore = cache.getDiskUsage();
  ASSERT_EQ(usageBefore, diskLimit) << "cache should be exactly full";

  // Re-access one entry per shard so each shard keeps a hot entry: no shard is
  // fully cold, defeating phase-1 whole-shard deletion entirely.
  for (int i : pinned) {
    std::vector<uint8_t> hash = makeHash(i);
    ASSERT_NE(cache.get(hash), nullptr) << "failed to pin entry " << i;
  }

  // The bug: GC reclaims nothing because no shard is fully cold. The fix
  // compacts the partially-cold shards instead.
  const size_t reclaimed = cache.garbageCollect();
  EXPECT_GT(reclaimed, 0u) << "GC must reclaim space from partially-cold shards";
  EXPECT_LE(cache.getDiskUsage(), diskLimit) << "GC must bring disk usage back under the limit";

  // Pinned (live) entries must survive GC with their pixels intact.
  for (int i : pinned) {
    std::vector<uint8_t> hash = makeHash(i);
    const rfb::GlobalClientPersistentCache::CachedPixels* e = cache.get(hash);
    ASSERT_NE(e, nullptr) << "pinned entry " << i << " lost to GC";
    ASSERT_EQ(e->pixels.size(), entryBytes);
    std::vector<uint8_t> expected = makePixels(i, pixelCount);
    EXPECT_EQ(memcmp(e->pixels.data(), expected.data(), entryBytes), 0) << "pinned entry " << i << " corrupted by GC";
  }

  EXPECT_LT(cache.getAllHashes().size(), (size_t)kEntries) << "GC should have dropped some cold entries";

  ASSERT_TRUE(cache.saveToDisk());

  // Reload from disk and confirm relocated payloads are at their rewritten
  // offsets (a fresh instance must hydrate them straight from the shard files).
  {
    rfb::GlobalClientPersistentCache reloaded(/*memMB*/ 2, /*diskMB*/ 5,
                                              /*shardMB*/ 1, cacheDir);
    ASSERT_TRUE(reloaded.loadIndexFromDisk());
    EXPECT_LE(reloaded.getDiskUsage(), diskLimit);

    for (int i : pinned) {
      std::vector<uint8_t> hash = makeHash(i);
      const rfb::GlobalClientPersistentCache::CachedPixels* e = reloaded.get(hash);
      ASSERT_NE(e, nullptr) << "pinned entry " << i << " missing after reload";
      ASSERT_EQ(e->pixels.size(), entryBytes);
      std::vector<uint8_t> expected = makePixels(i, pixelCount);
      EXPECT_EQ(memcmp(e->pixels.data(), expected.data(), entryBytes), 0)
          << "pinned entry " << i << " has corrupt pixels after compaction+reload";
    }
  }

  removeDirRecursive(cacheDir);
}
