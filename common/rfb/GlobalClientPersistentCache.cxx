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

#include <rfb/GlobalClientPersistentCache.h>
#include <core/LogWriter.h>
#include <rfb/cache/ArcCache.h>

#include <time.h>
#include <cstring>
#include <algorithm>
#include <fstream>
#include <sys/stat.h>
#include <errno.h>
#include <iostream>
#include <iomanip>

#ifdef HAVE_GNUTLS
#include <gnutls/gnutls.h>
#include <gnutls/crypto.h>
#else
// Fallback to a simple checksum if GnuTLS not available
#include <zlib.h>
#endif

using namespace rfb;

static core::LogWriter vlog("PersistentCache");

// ============================================================================
// PersistentCache Debug Logger Implementation
// ============================================================================

PersistentCacheDebugLogger::PersistentCacheDebugLogger() {
  auto now = std::chrono::system_clock::now();
  auto time_val = std::chrono::system_clock::to_time_t(now);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
    now.time_since_epoch()) % 1000;
  
  std::string timestamp = std::to_string(time_val) + "_" + std::to_string(ms.count());
  logFilename_ = "/tmp/persistentcache_debug_" + timestamp + ".log";
  
  logFile_.open(logFilename_, std::ios::out | std::ios::app);
  if (logFile_.is_open()) {
    std::cout << "PersistentCache debug log: " << logFilename_ << std::endl;
    log("=== PersistentCache Debug Log Started ===");
  } else {
    std::cerr << "Failed to open PersistentCache debug log: " << logFilename_ << std::endl;
  }
}

PersistentCacheDebugLogger::~PersistentCacheDebugLogger() {
  if (logFile_.is_open()) {
    log("=== PersistentCache Debug Log Ended ===");
    logFile_.close();
  }
}

void PersistentCacheDebugLogger::log(const std::string& message) {
  std::lock_guard<std::mutex> lock(logMutex_);
  if (logFile_.is_open()) {
    auto now = std::chrono::system_clock::now();
    auto time_val = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
      now.time_since_epoch()) % 1000;
    
    logFile_ << "[" << time_val << "." << std::setfill('0') << std::setw(3) << ms.count()
             << "] " << message << std::endl;
    logFile_.flush();
  }
}

// ============================================================================
// GlobalClientPersistentCache Implementation - ARC Algorithm with Sharded Storage
// ============================================================================

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxMemorySizeMB,
                                                           size_t maxDiskSizeMB,
                                                           size_t shardSizeMB,
                                                           const std::string& cacheDirOverride)
  : maxMemorySize_(maxMemorySizeMB * 1024 * 1024),
    maxDiskSize_(maxDiskSizeMB == 0 ? maxMemorySizeMB * 2 * 1024 * 1024 : maxDiskSizeMB * 1024 * 1024),
    shardSize_(shardSizeMB * 1024 * 1024),
    hydrationState_(HydrationState::Uninitialized),
    currentShardId_(0),
    currentShardHandle_(nullptr),
    currentShardSize_(0)
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor ENTER: memMB=" + 
    std::to_string(maxMemorySizeMB) + " diskMB=" + std::to_string(maxDiskSize_ / (1024*1024)));
  
  memset(&stats_, 0, sizeof(stats_));
  
  // Determine cache directory: allow viewer parameter override
  if (!cacheDirOverride.empty()) {
    cacheDir_ = cacheDirOverride;
  } else {
    const char* home = getenv("HOME");
    if (home) {
      cacheDir_ = std::string(home) + "/.cache/tigervnc/persistentcache";
    } else {
      cacheDir_ = "/tmp/tigervnc_persistentcache";
    }
  }
  
  vlog.debug("PersistentCache v3 (sharded): memory=%zuMB, disk=%zuMB, shard=%zuMB, dir=%s", 
             maxMemorySizeMB, maxDiskSize_ / (1024*1024), shardSizeMB, cacheDir_.c_str());

  // Create ARC cache with byte-based capacity; value size is measured via
  // CachedPixels::byteSize(). On eviction we record the full protocol hash
  // so DecodeManager can notify the server via eviction messages.
  arcCache_.reset(new rfb::cache::ArcCache<CacheKey, CachedPixels, CacheKeyHash>(
      maxMemorySize_,
      [](const CachedPixels& e) { return e.byteSize(); },
      [this](const CacheKey& key) {
        auto itHash = keyToHash_.find(key);
        if (itHash != keyToHash_.end()) {
          const std::vector<uint8_t>& fullHash = itHash->second;
          pendingEvictions_.push_back(fullHash);
          // Mark as cold - entry stays on disk but is evicted from memory
          auto it = indexMap_.find(fullHash);
          if (it != indexMap_.end()) {
            it->second.isCold = true;
            coldEntries_.insert(fullHash);
          }
          // Remove from dirty set (already written to shard)
          dirtyEntries_.erase(fullHash);
        }
      }
  ));
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor EXIT: cacheDir=" + cacheDir_);
}

GlobalClientPersistentCache::~GlobalClientPersistentCache()
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor ENTER: entries=" + std::to_string(cache_.size()));
  
  // Close current shard handle if open
  closeCurrentShard();
  
  vlog.debug("PersistentCache destroyed: %zu entries (%zu cold)", cache_.size(), coldEntries_.size());
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor EXIT");
}

bool GlobalClientPersistentCache::has(const std::vector<uint8_t>& hash) const
{
  // Translate protocol-level hash to shared in-memory key
  auto itKey = hashToKey_.find(hash);
  if (itKey != hashToKey_.end() && arcCache_ && arcCache_->has(itKey->second))
    return true;
  // Also check indexMap_ for entries loaded but not yet hydrated
  return indexMap_.find(hash) != indexMap_.end();
}

const GlobalClientPersistentCache::CachedPixels* 
GlobalClientPersistentCache::get(const std::vector<uint8_t>& hash)
{
  if (!arcCache_) return nullptr;
  
  // First check if already in ARC cache (hot in memory)
  const CachedPixels* e = nullptr;
  auto itKey = hashToKey_.find(hash);
  if (itKey != hashToKey_.end()) {
    e = arcCache_->get(itKey->second);
  }
  if (e != nullptr) {
    stats_.cacheHits++;
    return e;
  }
  
  // Check if in index (either cold on disk, or not yet hydrated from startup)
  auto indexIt = indexMap_.find(hash);
  if (indexIt != indexMap_.end()) {
    // Entry exists on disk but not in memory - hydrate it now (on-demand)
    // This handles both:
    //   1. Initial lazy load (entry never hydrated)
    //   2. Cold entry re-hydration (was evicted from ARC but still on disk)
    if (hydrateEntry(hash)) {
      // Re-fetch from ARC cache after hydration
      auto itKey2 = hashToKey_.find(hash);
      if (itKey2 != hashToKey_.end())
        e = arcCache_->get(itKey2->second);
      if (e != nullptr) {
        stats_.cacheHits++;
        // If this was a cold entry, it's now hot again
        coldEntries_.erase(hash);
        return e;
      }
    }
    // Hydration failed - treat as miss
  }
  
  // Not found anywhere
  stats_.cacheMisses++;
  return nullptr;
}

const GlobalClientPersistentCache::CachedPixels*
GlobalClientPersistentCache::getByKey(const CacheKey& key)
{
  // Fast path: look up full hash corresponding to this CacheKey and
  // delegate to the existing get(const std::vector<uint8_t>&) path so
  // we reuse all hydration and stats logic.
  auto it = keyToHash_.find(key);
  if (it == keyToHash_.end()) {
    // No mapping from key→hash means we have never seen this content.
    stats_.cacheMisses++;
    return nullptr;
  }
  return get(it->second);
}

void GlobalClientPersistentCache::insert(uint64_t cacheId,
                                         const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels,
                                         bool isLossless)
{
  if (!arcCache_ || pixels == nullptr || width == 0 || height == 0)
    return;

  // Construct the shared ContentKey using the explicit cacheId for
  // contentHash rather than deriving it from the hash bytes. This keeps
  // the on-wire 64-bit ID and the in-memory key perfectly aligned while
  // allowing the full hash vector to remain a pure ContentHash of the
  // decoded pixels.
  CacheKey key;
  key.width = width;
  key.height = height;
  key.contentHash = cacheId;

  // Update ARC statistics: treat new inserts as misses and
  // re-initialisations of existing entries as hits.
  if (arcCache_->has(key)) {
    stats_.cacheHits++;
  } else {
    stats_.cacheMisses++;
  }

  // Build CachedPixels entry and copy rows respecting stride (pixels)
  CachedPixels entry;
  entry.format = pf;
  entry.width = width;
  entry.height = height;
  // Store pixels contiguously in our cache to simplify later blits
  // NOTE: stridePixels in this struct is in PIXELS for the stored buffer.
  // Since we copy rows tightly, the stored stride is exactly the width.
  entry.stridePixels = width;
  entry.lastAccessTime = getCurrentTime();
 
  const size_t bppBytes = pf.bpp / 8;
  const size_t rowBytes = (size_t)width * bppBytes;
  const size_t srcStrideBytes = (size_t)stridePixels * bppBytes;
  entry.pixels.resize((size_t)height * rowBytes);
  const uint8_t* src = pixels;
  uint8_t* dst = entry.pixels.data();
  for (uint16_t y = 0; y < height; y++) {
    memcpy(dst, src, rowBytes);
    src += srcStrideBytes;
    dst += rowBytes;
  }

  // Keep persistence map in sync (used for save/load); note this duplicates
  // memory temporarily. cache_ is keyed by CacheKey, while index/disk are
  // keyed by the full hash.
  cache_[key] = entry;
  arcCache_->insert(key, entry);

  // Maintain bidirectional mapping between key and full hash
  keyToHash_[key] = hash;
  hashToKey_[hash] = key;
  
  // Mark as dirty for incremental save (only new entries, not re-hydrated
  // ones) if this payload is suitable for disk persistence. Lossy entries
  // remain memory-only: they participate in the ARC cache but are never
  // written out to shards/index.
  if (isLossless)
    dirtyEntries_.insert(hash);
}

std::vector<std::vector<uint8_t>> 
GlobalClientPersistentCache::getAllHashes() const
{
  std::vector<std::vector<uint8_t>> hashes;
  // Include both hydrated entries (cache_) and index-only entries (indexMap_)
  hashes.reserve(cache_.size() + indexMap_.size());
  for (const auto& kv : cache_) {
    auto itHash = keyToHash_.find(kv.first);
    if (itHash != keyToHash_.end())
      hashes.push_back(itHash->second);
  }
  // Add index-only entries that haven't been hydrated yet
  for (const auto& entry : indexMap_) {
    // Skip if already in cache_ (would be duplicate)
    auto itKey = hashToKey_.find(entry.first);
    if (itKey == hashToKey_.end() || cache_.find(itKey->second) == cache_.end()) {
      hashes.push_back(entry.first);
    }
  }
  return hashes;
}

std::vector<uint64_t>
GlobalClientPersistentCache::getAllContentIds() const
{
  // Derive the 64-bit contentHash IDs from the CacheKey mapping. Multiple
  // full hashes may share the same 64-bit prefix; de-duplicate via a set.
  std::unordered_set<uint64_t> ids;
  ids.reserve(keyToHash_.size());
  for (const auto& kv : keyToHash_) {
    ids.insert(kv.first.contentHash);
  }
  return std::vector<uint64_t>(ids.begin(), ids.end());
}

void GlobalClientPersistentCache::clear()
{
  if (arcCache_) arcCache_->clear();
  cache_.clear();
  indexMap_.clear();
  coldEntries_.clear();
  dirtyEntries_.clear();
  hydrationQueue_.clear();
  pendingEvictions_.clear();
  keyToHash_.clear();
  hashToKey_.clear();
  stats_.totalEntries = 0;
  stats_.totalBytes = 0;
  vlog.debug("PersistentCache cleared");
}

GlobalClientPersistentCache::Stats 
GlobalClientPersistentCache::getStats() const
{
  Stats current = stats_;
  size_t totalEntries = cache_.size();
  size_t totalBytes = 0;
  size_t t1Count = 0, t2Count = 0, b1Count = 0, b2Count = 0, target = 0;
  if (arcCache_) {
    auto s = arcCache_->getStats();
    totalEntries = s.totalEntries;
    totalBytes = s.totalBytes;
    t1Count = s.t1Size;
    t2Count = s.t2Size;
    b1Count = s.b1Size;
    b2Count = s.b2Size;
    target = s.targetT1Size;
  }
  current.totalEntries = totalEntries;
  current.totalBytes = totalBytes;
  current.t1Size = t1Count;
  current.t2Size = t2Count;
  current.b1Size = b1Count;
  current.b2Size = b2Count;
  current.targetT1Size = target;
  return current;
}

void GlobalClientPersistentCache::resetStats()
{
  stats_.cacheHits = 0;
  stats_.cacheMisses = 0;
  stats_.evictions = 0;
}

void GlobalClientPersistentCache::setMaxSize(size_t maxSizeMB)
{
  maxMemorySize_ = maxSizeMB * 1024 * 1024;
  vlog.debug("PersistentCache memory size set to %zuMB", maxSizeMB);
  // Recreate arc cache to apply new capacity
  arcCache_.reset(new rfb::cache::ArcCache<CacheKey, CachedPixels, CacheKeyHash>(
      maxMemorySize_,
      [](const CachedPixels& e) { return e.byteSize(); },
      [this](const CacheKey& key) {
        auto itHash = keyToHash_.find(key);
        if (itHash != keyToHash_.end()) {
          const std::vector<uint8_t>& fullHash = itHash->second;
          pendingEvictions_.push_back(fullHash);
          auto it = indexMap_.find(fullHash);
          if (it != indexMap_.end()) {
            it->second.isCold = true;
            coldEntries_.insert(fullHash);
          }
          dirtyEntries_.erase(fullHash);
        }
      }
  ));
}

// ============================================================================
// v3 Sharded Storage Helper Methods
// ============================================================================

std::string GlobalClientPersistentCache::getIndexPath() const
{
  return cacheDir_ + "/index.dat";
}

std::string GlobalClientPersistentCache::getShardPath(uint16_t shardId) const
{
  char buf[32];
  snprintf(buf, sizeof(buf), "/shard_%04u.dat", shardId);
  return cacheDir_ + buf;
}

bool GlobalClientPersistentCache::ensureCacheDir()
{
  // Create directory (mkdir -p equivalent)
  std::string currentPath;
  for (size_t i = 0; i < cacheDir_.length(); i++) {
    if (cacheDir_[i] == '/' && i > 0) {
      currentPath = cacheDir_.substr(0, i);
      mkdir(currentPath.c_str(), 0755);
    }
  }
  mkdir(cacheDir_.c_str(), 0755);
  
  // Check if directory exists
  struct stat st;
  return stat(cacheDir_.c_str(), &st) == 0 && S_ISDIR(st.st_mode);
}

bool GlobalClientPersistentCache::openCurrentShard()
{
  if (currentShardHandle_)
    return true;  // Already open
  
  if (!ensureCacheDir())
    return false;
  
  std::string path = getShardPath(currentShardId_);
  currentShardHandle_ = fopen(path.c_str(), "ab");  // Append mode
  if (!currentShardHandle_) {
    vlog.error("PersistentCache: failed to open shard %u: %s", currentShardId_, strerror(errno));
    return false;
  }
  
  // Get current size
  fseek(currentShardHandle_, 0, SEEK_END);
  currentShardSize_ = ftell(currentShardHandle_);
  shardSizes_[currentShardId_] = currentShardSize_;
  
  return true;
}

void GlobalClientPersistentCache::closeCurrentShard()
{
  if (currentShardHandle_) {
    fclose(currentShardHandle_);
    currentShardHandle_ = nullptr;
  }
}

bool GlobalClientPersistentCache::writeEntryToShard(const std::vector<uint8_t>& hash, 
                                                     const CachedPixels& entry)
{
  // Check if current shard is full
  if (currentShardSize_ >= shardSize_) {
    closeCurrentShard();
    currentShardId_++;
    currentShardSize_ = 0;
  }
  
  if (!openCurrentShard())
    return false;
  
  // Record position before write
  uint32_t offset = currentShardSize_;
  
  // Write pixel data to shard
  size_t written = fwrite(entry.pixels.data(), 1, entry.pixels.size(), currentShardHandle_);
  if (written != entry.pixels.size()) {
    vlog.error("PersistentCache: failed to write to shard");
    return false;
  }
  fflush(currentShardHandle_);
  
  currentShardSize_ += written;
  shardSizes_[currentShardId_] = currentShardSize_;
  
  // Update index entry
  IndexEntry idx;
  idx.shardId = currentShardId_;
  idx.payloadOffset = offset;
  idx.payloadSize = entry.pixels.size();
  idx.width = entry.width;
  idx.height = entry.height;
  idx.stridePixels = entry.stridePixels;
  idx.format = entry.format;
  idx.isCold = false;
  
  indexMap_[hash] = idx;
  
  return true;
}

size_t GlobalClientPersistentCache::getDiskUsage() const
{
  size_t total = 0;
  for (const auto& entry : shardSizes_) {
    total += entry.second;
  }
  return total;
}

// ============================================================================
// Disk I/O Methods - v3 Sharded Format
// ============================================================================

bool GlobalClientPersistentCache::loadFromDisk()
{
  // Check for legacy v1/v2 single-file format and delete if found
  std::string legacyPath = cacheDir_ + ".dat";  // Old single-file path
  struct stat st;
  if (stat(legacyPath.c_str(), &st) == 0 && S_ISREG(st.st_mode)) {
    vlog.info("PersistentCache: detected legacy single-file format, deleting");
    remove(legacyPath.c_str());
  }
  
  // Also check for v2 format in the old default location
  const char* home = getenv("HOME");
  if (home) {
    std::string oldDefault = std::string(home) + "/.cache/tigervnc/persistentcache.dat";
    if (stat(oldDefault.c_str(), &st) == 0 && S_ISREG(st.st_mode)) {
      vlog.info("PersistentCache: detected v2 format at old location, deleting");
      remove(oldDefault.c_str());
    }
  }
  
  hydrationState_ = HydrationState::FullyHydrated;  // Start fresh
  return false;
}

bool GlobalClientPersistentCache::loadIndexFromDisk()
{
  std::string indexPath = getIndexPath();
  FILE* f = fopen(indexPath.c_str(), "rb");
  if (!f) {
    vlog.info("PersistentCache: no index file at %s (fresh start)", indexPath.c_str());
    hydrationState_ = HydrationState::FullyHydrated;
    return false;
  }
  
  // v3 index header
  struct IndexHeader {
    uint32_t magic;
    uint32_t version;
    uint64_t entryCount;
    uint64_t created;
    uint64_t lastAccess;
    uint16_t maxShardId;
    uint8_t reserved[30];
  } header;
  
  if (fread(&header, sizeof(header), 1, f) != 1) {
    vlog.error("PersistentCache: failed to read index header");
    fclose(f);
    return false;
  }
  
  const uint32_t MAGIC_V3 = 0x50435633;  // "PCV3"
  if (header.magic != MAGIC_V3) {
    vlog.info("PersistentCache: invalid/old index format, starting fresh");
    fclose(f);
    remove(indexPath.c_str());
    hydrationState_ = HydrationState::FullyHydrated;
    return false;
  }
  
  if (header.version != 3) {
    vlog.info("PersistentCache: unsupported index version %u, starting fresh", header.version);
    fclose(f);
    remove(indexPath.c_str());
    hydrationState_ = HydrationState::FullyHydrated;
    return false;
  }
  
  vlog.info("PersistentCache: loading v3 index (%llu entries, %u shards)",
            (unsigned long long)header.entryCount, header.maxShardId + 1);
  
  indexMap_.clear();
  hydrationQueue_.clear();
  coldEntries_.clear();
  keyToHash_.clear();
  hashToKey_.clear();
  
  // Read index entries
  // Format: hash(16) + shardId(2) + offset(4) + size(4) + width(2) + height(2) + stride(2) + PixelFormat(24) + flags(1)
  for (uint64_t i = 0; i < header.entryCount; i++) {
    std::vector<uint8_t> hash(16);
    if (fread(hash.data(), 1, 16, f) != 16) break;
    
    IndexEntry entry;
    if (fread(&entry.shardId, sizeof(entry.shardId), 1, f) != 1) break;
    if (fread(&entry.payloadOffset, sizeof(entry.payloadOffset), 1, f) != 1) break;
    if (fread(&entry.payloadSize, sizeof(entry.payloadSize), 1, f) != 1) break;
    if (fread(&entry.width, sizeof(entry.width), 1, f) != 1) break;
    if (fread(&entry.height, sizeof(entry.height), 1, f) != 1) break;
    if (fread(&entry.stridePixels, sizeof(entry.stridePixels), 1, f) != 1) break;
    if (fread(&entry.format, 24, 1, f) != 1) break;
    
    uint8_t flags;
    if (fread(&flags, 1, 1, f) != 1) break;
    entry.isCold = (flags & 0x01) != 0;

    entry.key.width = entry.width;
    entry.key.height = entry.height;
    entry.key.contentHash = 0;
    if (!hash.empty()) {
      size_t n = std::min(hash.size(), sizeof(uint64_t));
      memcpy(&entry.key.contentHash, hash.data(), n);
    }

    indexMap_[hash] = entry;
    hydrationQueue_.push_back(hash);

    // Maintain bidirectional mapping so in-memory ARC/cache use ContentKey
    keyToHash_[entry.key] = hash;
    hashToKey_[hash] = entry.key;
    
    // Track shard sizes
    if (shardSizes_.find(entry.shardId) == shardSizes_.end()) {
      shardSizes_[entry.shardId] = 0;
    }
    size_t endOffset = entry.payloadOffset + entry.payloadSize;
    if (endOffset > shardSizes_[entry.shardId]) {
      shardSizes_[entry.shardId] = endOffset;
    }
  }
  
  fclose(f);
  
  // Set current shard to continue appending
  currentShardId_ = header.maxShardId;
  auto it = shardSizes_.find(currentShardId_);
  currentShardSize_ = (it != shardSizes_.end()) ? it->second : 0;
  
  hydrationState_ = HydrationState::IndexLoaded;
  
  vlog.info("PersistentCache: index loaded, %zu entries pending hydration",
            hydrationQueue_.size());
  
  return true;
}

bool GlobalClientPersistentCache::hydrateEntry(const std::vector<uint8_t>& hash)
{
  // Check if already hydrated in the ARC cache. Since the ARC key is
  // CacheKey, translate the protocol-level hash via hashToKey_.
  if (arcCache_) {
    auto itKey = hashToKey_.find(hash);
    if (itKey != hashToKey_.end() && arcCache_->has(itKey->second))
      return true;
  }
  
  // Find in index
  auto it = indexMap_.find(hash);
  if (it == indexMap_.end())
    return false;
  
  const IndexEntry& idx = it->second;
  
  // Open the shard file for reading
  std::string shardPath = getShardPath(idx.shardId);
  FILE* f = fopen(shardPath.c_str(), "rb");
  if (!f) {
    vlog.error("PersistentCache: cannot open shard %u for hydration", idx.shardId);
    return false;
  }
  
  // Seek to payload
  if (fseek(f, idx.payloadOffset, SEEK_SET) != 0) {
    vlog.error("PersistentCache: failed to seek in shard %u", idx.shardId);
    fclose(f);
    return false;
  }
  
  // Read pixel data
  std::vector<uint8_t> pixelData(idx.payloadSize);
  if (fread(pixelData.data(), 1, idx.payloadSize, f) != idx.payloadSize) {
    vlog.error("PersistentCache: failed to read from shard %u", idx.shardId);
    fclose(f);
    return false;
  }
  
  fclose(f);
  
  // Build CachedPixels entry
  CachedPixels entry;
  entry.format = idx.format;
  entry.width = idx.width;
  entry.height = idx.height;
  entry.stridePixels = idx.stridePixels;
  entry.lastAccessTime = getCurrentTime();
  entry.pixels = std::move(pixelData);
  
  // Use the key stored in the index so CacheKey/ContentHash mapping stays
  // consistent across disk and memory.
  CacheKey key = idx.key;
  cache_[key] = entry;
  if (arcCache_)
    arcCache_->insert(key, entry);
  
  // Mark as hot (no longer cold)
  it->second.isCold = false;
  coldEntries_.erase(hash);
  
  // Remove from hydration queue
  hydrationQueue_.remove(hash);
  
  // Update hydration state
  if (hydrationQueue_.empty()) {
    hydrationState_ = HydrationState::FullyHydrated;
    vlog.debug("PersistentCache: fully hydrated");
  } else {
    hydrationState_ = HydrationState::PartiallyHydrated;
  }
  
  return true;
}

size_t GlobalClientPersistentCache::hydrateNextBatch(size_t maxEntries)
{
  if (hydrationQueue_.empty())
    return 0;
  
  size_t hydrated = 0;
  
  while (hydrated < maxEntries && !hydrationQueue_.empty()) {
    std::vector<uint8_t> hash = hydrationQueue_.front();
    
    if (hydrateEntry(hash)) {
      hydrated++;
    } else {
      // Failed to hydrate, remove from queue
      hydrationQueue_.pop_front();
    }
  }
  
  if (hydrated > 0) {
    vlog.debug("PersistentCache: proactively hydrated %zu entries, %zu remaining",
               hydrated, hydrationQueue_.size());
  }
  
  return hydrated;
}

size_t GlobalClientPersistentCache::flushDirtyEntries()
{
  if (dirtyEntries_.empty())
    return 0;
  
  size_t flushed = 0;
  
  // Write each dirty entry to the current shard
  for (const auto& hash : dirtyEntries_) {
    auto keyIt = hashToKey_.find(hash);
    if (keyIt == hashToKey_.end())
      continue;
    auto cacheIt = cache_.find(keyIt->second);
    if (cacheIt == cache_.end())
      continue;  // Entry was evicted
    
    if (writeEntryToShard(hash, cacheIt->second)) {
      flushed++;
    }
  }
  
  dirtyEntries_.clear();
  
  // Save index after flushing
  if (flushed > 0) {
    saveToDisk();
    vlog.debug("PersistentCache: flushed %zu entries to shard %u", flushed, currentShardId_);
  }
  
  // Check disk usage and trigger GC if needed
  size_t diskUsage = getDiskUsage();
  if (diskUsage > maxDiskSize_) {
    vlog.debug("PersistentCache: disk usage %zuMB exceeds limit %zuMB, triggering GC",
               diskUsage / (1024*1024), maxDiskSize_ / (1024*1024));
    garbageCollect();
  }
  
  return flushed;
}

size_t GlobalClientPersistentCache::garbageCollect()
{
  // GC strategy: remove entries from oldest shards that are cold
  // and exceed the disk limit
  
  size_t diskUsage = getDiskUsage();
  if (diskUsage <= maxDiskSize_ && coldEntries_.empty()) {
    return 0;  // Nothing to collect
  }
  
  size_t reclaimed = 0;
  
  // Find the oldest shard with cold entries
  // For simplicity, just remove cold entries until we're under the limit
  std::vector<std::vector<uint8_t>> toRemove;
  for (const auto& hash : coldEntries_) {
    if (diskUsage <= maxDiskSize_ * 0.9)  // Target 90% of limit
      break;
    
    auto it = indexMap_.find(hash);
    if (it != indexMap_.end()) {
      reclaimed += it->second.payloadSize;
      diskUsage -= it->second.payloadSize;
      toRemove.push_back(hash);
    }
  }
  
  // Remove from index
  for (const auto& hash : toRemove) {
    indexMap_.erase(hash);
    coldEntries_.erase(hash);
  }
  
  if (reclaimed > 0) {
    vlog.debug("PersistentCache: GC reclaimed %zuKB from %zu cold entries",
               reclaimed / 1024, toRemove.size());
    // Note: actual shard files are not compacted here; that would require
    // rewriting shards which is expensive. We just mark entries as removed
    // in the index. Shards can be compacted on next full save.
  }
  
  return reclaimed;
}

bool GlobalClientPersistentCache::saveToDisk()
{
  closeCurrentShard();
  
  if (!ensureCacheDir())
    return false;
  
  std::string indexPath = getIndexPath();
  FILE* f = fopen(indexPath.c_str(), "wb");
  if (!f) {
    vlog.error("PersistentCache: failed to open index for writing: %s", strerror(errno));
    return false;
  }
  
  // Find max shard ID
  uint16_t maxShardId = 0;
  for (const auto& entry : indexMap_) {
    if (entry.second.shardId > maxShardId)
      maxShardId = entry.second.shardId;
  }
  
  // Write v3 index header
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
  header.magic = 0x50435633;  // "PCV3"
  header.version = 3;
  header.entryCount = indexMap_.size();
  header.created = time(nullptr);
  header.lastAccess = time(nullptr);
  header.maxShardId = maxShardId;
  
  if (fwrite(&header, sizeof(header), 1, f) != 1) {
    vlog.error("PersistentCache: failed to write index header");
    fclose(f);
    return false;
  }
  
  // Write index entries
  for (const auto& entry : indexMap_) {
    const std::vector<uint8_t>& hash = entry.first;
    const IndexEntry& idx = entry.second;
    
    // Pad/truncate hash to 16 bytes
    uint8_t hashBuf[16] = {0};
    size_t copyLen = std::min(hash.size(), (size_t)16);
    memcpy(hashBuf, hash.data(), copyLen);
    fwrite(hashBuf, 1, 16, f);
    
    fwrite(&idx.shardId, sizeof(idx.shardId), 1, f);
    fwrite(&idx.payloadOffset, sizeof(idx.payloadOffset), 1, f);
    fwrite(&idx.payloadSize, sizeof(idx.payloadSize), 1, f);
    fwrite(&idx.width, sizeof(idx.width), 1, f);
    fwrite(&idx.height, sizeof(idx.height), 1, f);
    fwrite(&idx.stridePixels, sizeof(idx.stridePixels), 1, f);
    fwrite(&idx.format, 24, 1, f);
    
    uint8_t flags = idx.isCold ? 0x01 : 0x00;
    fwrite(&flags, 1, 1, f);
  }
  
  fclose(f);
  
  vlog.info("PersistentCache: saved v3 index with %zu entries", indexMap_.size());
  return true;
}

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
