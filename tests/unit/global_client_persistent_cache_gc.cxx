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

#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <cstdio>
#include <cstring>

namespace {

static void writeFileWithSize(const char* path, size_t size)
{
  int fd = ::open(path, O_CREAT | O_TRUNC | O_WRONLY, 0644);
  ASSERT_GE(fd, 0);
  // Ensure the file has the desired size; contents don't matter for the test.
  ASSERT_EQ(::ftruncate(fd, (off_t)size), 0);
  ::close(fd);
}

static void writeEmptyV5Index(const std::string& dir)
{
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

static bool fileExists(const std::string& path)
{
  struct stat st;
  return stat(path.c_str(), &st) == 0;
}

} // namespace

TEST(GlobalClientPersistentCache, LoadIndexDeletesOrphanShards)
{
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
