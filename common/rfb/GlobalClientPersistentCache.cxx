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
#include <limits>
#include <dirent.h>

#ifdef HAVE_GNUTLS
#include <gnutls/gnutls.h>
#include <gnutls/crypto.h>
#else
// Fallback to a simple checksum if GnuTLS not available
#include <zlib.h>
#endif

using namespace rfb;

static core::LogWriter vlog("PersistentCache");

// Helpers for the unified 16-byte CacheKey
static inline uint64_t cacheKeyFirstU64(const CacheKey& key) {
  uint64_t v = 0;
  std::memcpy(&v, key.bytes.data(), sizeof(v));
  return v;
}

static inline void cacheKeyToHex(const CacheKey& key, char out[33]) {
  static const char* hexd = "0123456789abcdef";
  for (int i = 0; i < 16; ++i) {
    uint8_t b = key.bytes[(size_t)i];
    out[i*2+0] = hexd[(b >> 4) & 0xF];
    out[i*2+1] = hexd[b & 0xF];
  }
  out[32] = '\0';
}


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

void PersistentCacheDebugLogger::logCacheHit(const char* cacheType, int x, int y, int w, int h,
                                              uint64_t cacheId, bool isLossless) {
  char buf[256];
  snprintf(buf, sizeof(buf), "%s HIT: rect [%d,%d %dx%d] id=0x%llx %s",
           cacheType, x, y, w, h, (unsigned long long)cacheId,
           isLossless ? "(lossless)" : "(lossy)");
  log(buf);
}

void PersistentCacheDebugLogger::logCacheMiss(const char* cacheType, int x, int y, int w, int h,
                                               uint64_t cacheId) {
  char buf[256];
  snprintf(buf, sizeof(buf), "%s MISS: rect [%d,%d %dx%d] id=0x%llx",
           cacheType, x, y, w, h, (unsigned long long)cacheId);
  log(buf);
}

void PersistentCacheDebugLogger::logCacheStore(const char* cacheType, int x, int y, int w, int h,
                                                uint64_t cacheId, int encoding, size_t bytes) {
  char buf[256];
  snprintf(buf, sizeof(buf), "%s STORE: rect [%d,%d %dx%d] id=0x%llx enc=%d bytes=%zu",
           cacheType, x, y, w, h, (unsigned long long)cacheId, encoding, bytes);
  log(buf);
}

void PersistentCacheDebugLogger::logCacheSeed(const char* cacheType, int x, int y, int w, int h,
                                               uint64_t cacheId, bool hashMatch) {
  char buf[256];
  snprintf(buf, sizeof(buf), "%s SEED: rect [%d,%d %dx%d] id=0x%llx %s",
           cacheType, x, y, w, h, (unsigned long long)cacheId,
           hashMatch ? "(hash match)" : "(hash mismatch)");
  log(buf);
}

void PersistentCacheDebugLogger::logStats(unsigned hits, unsigned misses, unsigned stores,
                                           size_t totalEntries, size_t totalBytes) {
  char buf[256];
  double hitRate = (hits + misses) > 0 ? (100.0 * hits / (hits + misses)) : 0.0;
  snprintf(buf, sizeof(buf), "STATS: hits=%u misses=%u stores=%u hitRate=%.1f%% entries=%zu bytes=%zu",
           hits, misses, stores, hitRate, totalEntries, totalBytes);
  log(buf);
}

static bool isSolidBlack(const uint8_t* pixels, size_t length)
{
  for (size_t i = 0; i < length; i++) {
    if (pixels[i] != 0)
      return false;
  }
  return true;
}

// Compute 3-bit quality code from pixel format and lossy flag
// Bit 0: lossy flag (0=lossless, 1=lossy)
// Bits 1-2: depth code (00=8bpp, 01=16bpp, 10=24/32bpp, 11=reserved)
uint8_t GlobalClientPersistentCache::computeQualityCode(const PixelFormat& pf, bool isLossy)
{
  uint8_t depthCode = 0;
  if (pf.bpp <= 8) {
    depthCode = 0;  // 8bpp
  } else if (pf.bpp <= 16) {
    depthCode = 1;  // 16bpp
  } else {
    depthCode = 2;  // 24/32bpp
  }
  return (depthCode << 1) | (isLossy ? 1 : 0);
}

static size_t mbToBytesClamped(size_t mb)
{
  const unsigned long long mul = 1024ULL * 1024ULL;
  const unsigned long long max =
    static_cast<unsigned long long>(std::numeric_limits<size_t>::max());

  if (static_cast<unsigned long long>(mb) > (max / mul))
    return std::numeric_limits<size_t>::max();

  return static_cast<size_t>(static_cast<unsigned long long>(mb) * mul);
}

static size_t mbDoubleClamped(size_t mb)
{
  if (mb > (std::numeric_limits<size_t>::max() / 2))
    return std::numeric_limits<size_t>::max();
  return mb * 2;
}

// ============================================================================
// GlobalClientPersistentCache Implementation - ARC Algorithm with Sharded Storage
// ============================================================================

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxMemorySizeMB,
                                                           size_t maxDiskSizeMB,
                                                           size_t shardSizeMB,
                                                           const std::string& cacheDirOverride)
  : maxMemorySize_(mbToBytesClamped(maxMemorySizeMB)),
    maxDiskSize_(mbToBytesClamped(maxDiskSizeMB == 0 ? mbDoubleClamped(maxMemorySizeMB) : maxDiskSizeMB)),
    shardSize_(mbToBytesClamped(shardSizeMB)),
    hydrationState_(HydrationState::Uninitialized),
    indexDirty_(false),
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
          pendingEvictions_.push_back(key);
          // Mark as cold - entry stays on disk but is evicted from memory
          auto it = indexMap_.find(fullHash);
          if (it != indexMap_.end()) {
            it->second.isCold = true;
            coldEntries_.insert(fullHash);
            indexDirty_ = true;
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
  
  // Stop coordinator first (releases lock, allows other viewers to become master)
  stopCoordinator();
  
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

const GlobalClientPersistentCache::CachedPixels*
GlobalClientPersistentCache::getByCanonicalHash(uint64_t canonicalHash, uint16_t width,
                                                 uint16_t height, uint8_t minBpp)
{
  // Lookup by canonical hash (server's lossless ID) and dimensions.
  // PREFERENCE ORDER (when minBpp allows):
  // 1) Highest bpp lossless entry
  // 2) Highest bpp lossy entry
  // 3) Lower bpp entries (only if minBpp=0 or entry meets minBpp)
  //
  // When minBpp > 0, entries with bpp < minBpp are rejected to prevent quality
  // loss from upscaling low-quality cached data to high-quality display format.

  vlog.debug("getByCanonicalHash: canonical=%llx dims=%dx%d minBpp=%d",
             (unsigned long long)canonicalHash, width, height, minBpp);

  if (!arcCache_)
    return nullptr;

  // Track best candidates at each quality level
  const CachedPixels* bestLossless = nullptr;
  const CachedPixels* bestLossy = nullptr;
  uint8_t bestLosslessBpp = 0;
  uint8_t bestLossyBpp = 0;
  int candidatesChecked = 0;
  int candidatesFiltered = 0;

  // 1) Scan hydrated entries in memory.
  for (const auto& kv : cache_) {
    const CachedPixels& entry = kv.second;
    if (entry.canonicalHash != canonicalHash ||
        entry.width != width ||
        entry.height != height)
      continue;

    const CachedPixels* e = arcCache_->get(kv.first);
    if (!e)
      continue;

    uint8_t entryBpp = e->format.bpp;
    candidatesChecked++;
    
    // Skip entries below minimum quality threshold
    if (minBpp > 0 && entryBpp < minBpp) {
      vlog.debug("  Filtering entry: canonical=%llx entryBpp=%d < minBpp=%d",
                 (unsigned long long)entry.canonicalHash, entryBpp, minBpp);
      candidatesFiltered++;
      continue;
    }

    if (e->isLossless()) {
      if (entryBpp > bestLosslessBpp || !bestLossless) {
        bestLossless = e;
        bestLosslessBpp = entryBpp;
      }
    } else {
      if (entryBpp > bestLossyBpp || !bestLossy) {
        bestLossy = e;
        bestLossyBpp = entryBpp;
      }
    }
  }

  // 2) Also check disk index for entries not yet hydrated
  for (const auto& idxKv : indexMap_) {
    const std::vector<uint8_t>& hash = idxKv.first;
    const IndexEntry& idx = idxKv.second;

    if (idx.width != width || idx.height != height)
      continue;
    if (idx.canonicalHash != canonicalHash)
      continue;

    uint8_t entryBpp = idx.format.bpp;
    
    // Skip entries below minimum quality threshold
    if (minBpp > 0 && entryBpp < minBpp)
      continue;

    // Check if this could be better than current best
    bool isLossless = (cacheKeyFirstU64(idx.key) == idx.canonicalHash);
    bool shouldHydrate = false;
    
    if (isLossless && entryBpp > bestLosslessBpp) {
      shouldHydrate = true;
    } else if (!isLossless && entryBpp > bestLossyBpp && !bestLossless) {
      shouldHydrate = true;
    }
    
    if (!shouldHydrate)
      continue;

    if (!hydrateEntry(hash))
      continue;

    auto itKey = hashToKey_.find(hash);
    if (itKey == hashToKey_.end())
      continue;

    const CachedPixels* e = arcCache_->get(itKey->second);
    if (!e)
      continue;

    coldEntries_.erase(hash);
    
    if (e->isLossless()) {
      if (e->format.bpp > bestLosslessBpp || !bestLossless) {
        bestLossless = e;
        bestLosslessBpp = e->format.bpp;
      }
    } else {
      if (e->format.bpp > bestLossyBpp || !bestLossy) {
        bestLossy = e;
        bestLossyBpp = e->format.bpp;
      }
    }
  }

  // Return best available entry: prefer lossless, then highest bpp lossy
  const CachedPixels* result = bestLossless ? bestLossless : bestLossy;
  
  vlog.debug("  Lookup result: checked=%d filtered=%d bestLossless=%p(bpp=%d) bestLossy=%p(bpp=%d)",
             candidatesChecked, candidatesFiltered,
             bestLossless, bestLosslessBpp, bestLossy, bestLossyBpp);
  
  if (result) {
    char fmtStr[256];
    result->format.print(fmtStr, sizeof(fmtStr));
    vlog.debug("  Returning entry: bpp=%d format=[%s] lossless=%s canonical=%llx actual=%llx",
               result->format.bpp, fmtStr,
               result->isLossless() ? "yes" : "no",
               (unsigned long long)result->canonicalHash,
               (unsigned long long)result->actualHash);
    stats_.cacheHits++;
    if (result->width * result->height > 1024 && 
        isSolidBlack(result->pixels.data(), result->pixels.size())) {
      vlog.info("PersistentCache WARNING: Retrieved solid black entry (Hit)! canonical=%llu size=%dx%d",
                (unsigned long long)canonicalHash, result->width, result->height);
    }
    return result;
  }

  vlog.debug("  No matching entry found - MISS");
  stats_.cacheMisses++;
  return nullptr;
}

void GlobalClientPersistentCache::insert(uint64_t canonicalHash,
                                         uint64_t actualHash,
                                         const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels,
                                         bool isPersistable)
{
  if (!arcCache_ || pixels == nullptr || width == 0 || height == 0)
    return;

  // Debug: log the format being stored
  char fmtStr[256];
  pf.print(fmtStr, sizeof(fmtStr));
  bool isLossless = (canonicalHash == actualHash);
  vlog.debug("INSERT: canonical=%llx actual=%llx dims=%dx%d format=[%s] bpp=%d lossless=%s",
             (unsigned long long)canonicalHash, (unsigned long long)actualHash,
             width, height, fmtStr, pf.bpp, isLossless ? "yes" : "no");

  // NEW DESIGN: Index by actualHash (client's computed hash) for fast direct
  // lookup, but store canonicalHash so we can also lookup by canonical.
  CacheKey key(hash.data()); // Unified key: 16-byte protocol hash

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
  entry.stridePixels = width;  // Stored contiguously
  entry.lastAccessTime = getCurrentTime();
  
  // NEW: Store both hashes
  entry.canonicalHash = canonicalHash;
  entry.actualHash = actualHash;
 
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

  // Keep persistence map in sync
  cache_[key] = entry;
  arcCache_->insert(key, entry);

  if (width * height > 1024 && isSolidBlack(entry.pixels.data(), entry.pixels.size())) {
     vlog.info("PersistentCache WARNING: Inserting solid black entry! canonical=%llu actual=%llu size=%dx%d",
               (unsigned long long)canonicalHash, (unsigned long long)actualHash, width, height);
  }

  // Maintain bidirectional mapping between key and full hash
  keyToHash_[key] = hash;
  hashToKey_[hash] = key;
  
  // NEW DESIGN: Both lossy and lossless entries persist to disk.
  // The isPersistable flag should always be true now, but we keep it for
  // compatibility during transition.
  if (isPersistable)
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


std::vector<CacheKey>
GlobalClientPersistentCache::getAllKeys() const
{
  std::unordered_set<CacheKey, CacheKeyHash> keys;
  keys.reserve(cache_.size() + indexMap_.size());

  // Hydrated entries
  for (const auto& kv : cache_) {
    keys.insert(kv.first);
  }

  // Index-only entries
  for (const auto& kv : indexMap_) {
    auto it = hashToKey_.find(kv.first);
    if (it != hashToKey_.end())
      keys.insert(it->second);
    else if (kv.first.size() >= 16)
      keys.insert(CacheKey(kv.first.data()));
  }

  return std::vector<CacheKey>(keys.begin(), keys.end());
}


void GlobalClientPersistentCache::clear()
{
  if (arcCache_) arcCache_->clear();
  cache_.clear();
  indexMap_.clear();
  coldEntries_.clear();
  dirtyEntries_.clear();
  indexDirty_ = false;
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


void GlobalClientPersistentCache::invalidateByKey(const CacheKey& key)
{
  auto itHash = keyToHash_.find(key);
  if (itHash == keyToHash_.end())
    return;

  const std::vector<uint8_t>& hash = itHash->second;

  cache_.erase(key);
  keyToHash_.erase(itHash);
  hashToKey_.erase(hash);

  indexMap_.erase(hash);
  coldEntries_.erase(hash);
  dirtyEntries_.erase(hash);
  hydrationQueue_.remove(hash);

  pendingEvictions_.erase(
      std::remove(pendingEvictions_.begin(), pendingEvictions_.end(), key),
      pendingEvictions_.end());

  if (arcCache_)
    arcCache_->clear();

  stats_.totalEntries = cache_.size();
  stats_.totalBytes = 0;
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
          pendingEvictions_.push_back(key);
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
  errno = 0;
  size_t written = fwrite(entry.pixels.data(), 1, entry.pixels.size(), currentShardHandle_);
  if (written != entry.pixels.size()) {
    int err = errno;
    vlog.error("PersistentCache: failed to write to shard %u (%zu/%zu bytes written): %s",
               currentShardId_, written, entry.pixels.size(), strerror(err));
    return false;
  }
  if (fflush(currentShardHandle_) != 0) {
    int err = errno;
    vlog.error("PersistentCache: failed to flush shard %u: %s",
               currentShardId_, strerror(err));
    return false;
  }
  
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
  idx.canonicalHash = entry.canonicalHash;
  
  // NEW in v7: compute quality code from pixel format and lossy flag
  bool isLossy = (entry.actualHash != entry.canonicalHash);
  idx.qualityCode = computeQualityCode(entry.format, isLossy);
  
  // Set key for index lookups (unified 16-byte hash)
  idx.key = CacheKey(hash.data());
  
  indexMap_[hash] = idx;
  indexDirty_ = true;
  
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

size_t GlobalClientPersistentCache::cleanupOrphanShardsOnDisk()
{
  // Build referenced shard set from the current index.
  std::unordered_set<uint16_t> referenced;
  referenced.reserve(indexMap_.size());
  for (const auto& kv : indexMap_) {
    referenced.insert(kv.second.shardId);
  }

  DIR* dirp = opendir(cacheDir_.c_str());
  if (!dirp)
    return 0;

  size_t reclaimed = 0;

  struct dirent* de;
  while ((de = readdir(dirp)) != nullptr) {
    const char* name = de->d_name;

    // shard_0000.dat
    unsigned shardIdTmp = 0;
    if (sscanf(name, "shard_%4u.dat", &shardIdTmp) != 1)
      continue;
    if (shardIdTmp > 0xFFFF)
      continue;

    uint16_t shardId = static_cast<uint16_t>(shardIdTmp);
    std::string path = getShardPath(shardId);

    struct stat st;
    if (stat(path.c_str(), &st) != 0)
      continue;
    if (!S_ISREG(st.st_mode))
      continue;

    if (referenced.find(shardId) == referenced.end()) {
      // Not referenced by index -> safe to delete.
      reclaimed += static_cast<size_t>(st.st_size);
      remove(path.c_str());
    } else {
      // Keep shardSizes_ aligned with actual file size.
      shardSizes_[shardId] = static_cast<size_t>(st.st_size);
    }
  }

  closedir(dirp);

  // Drop any shardSizes_ entries for shards that are no longer referenced.
  for (auto it = shardSizes_.begin(); it != shardSizes_.end();) {
    if (referenced.find(it->first) == referenced.end()) {
      it = shardSizes_.erase(it);
    } else {
      ++it;
    }
  }

  // Choose an append shard based on what's actually referenced.
  uint16_t maxId = 0;
  bool haveAny = false;
  for (const auto& kv : shardSizes_) {
    if (!haveAny || kv.first > maxId) {
      maxId = kv.first;
      haveAny = true;
    }
  }
  currentShardId_ = haveAny ? maxId : 0;
  currentShardSize_ = haveAny ? shardSizes_[currentShardId_] : 0;

  return reclaimed;
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
  
  // v4 introduces canonicalHash field
  // v5 changes ContentHash to include dimensions
  // v6 fixes PixelFormat serialization (was truncated at 24 bytes)
  // v7 adds qualityCode (3-bit field for depth + lossy flag)
  if (header.version != 6 && header.version != 7) {
    vlog.info("PersistentCache: unsupported index version %u (expected 6 or 7), starting fresh", header.version);
    fclose(f);
    remove(indexPath.c_str());
    hydrationState_ = HydrationState::FullyHydrated;
    return false;
  }
  
  bool isV7 = (header.version == 7);
  vlog.info("PersistentCache: loading v%u index (%llu entries, %u shards)",
            header.version, (unsigned long long)header.entryCount, header.maxShardId + 1);
  
  indexMap_.clear();
  hydrationQueue_.clear();
  coldEntries_.clear();
  dirtyEntries_.clear();
  indexDirty_ = false;
  keyToHash_.clear();
  hashToKey_.clear();
  
    // Read index entries
    // Format v6: hash(16) + shardId(2) + offset(4) + size(4) + width(2) + height(2) + stride(2)
    //            + PixelFormat(16 bytes VNC wire format) + flags(1) + canonicalHash(8)
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
      
      // Read PixelFormat in VNC wire format (16 bytes)
      // This matches the format used by PixelFormat::read()/write() methods
      struct {
        uint8_t bpp;
        uint8_t depth;
        uint8_t bigEndian;
        uint8_t trueColour;
        uint16_t redMax;
        uint16_t greenMax;
        uint16_t blueMax;
        uint8_t redShift;
        uint8_t greenShift;
        uint8_t blueShift;
        uint8_t padding[3];
      } pfData;  // 16 bytes total (VNC wire format)
      
      if (fread(&pfData, sizeof(pfData), 1, f) != 1) break;
      
      // Reconstruct PixelFormat from serialized data
      entry.format = PixelFormat(pfData.bpp, pfData.depth,
                                  pfData.bigEndian != 0, pfData.trueColour != 0,
                                  pfData.redMax, pfData.greenMax, pfData.blueMax,
                                  pfData.redShift, pfData.greenShift, pfData.blueShift);
      
      uint8_t flags;
      if (fread(&flags, 1, 1, f) != 1) break;
      entry.isCold = (flags & 0x01) != 0;

      // New in v4
      if (fread(&entry.canonicalHash, sizeof(entry.canonicalHash), 1, f) != 1) break;
      
      // New in v7: read qualityCode
      if (isV7) {
        if (fread(&entry.qualityCode, sizeof(entry.qualityCode), 1, f) != 1) break;
      } else {
        // v6 migration: compute qualityCode from existing data
        // Determine if lossy by comparing actualHash (from hash) to canonicalHash
        uint64_t actualFromHash = 0;
        if (!hash.empty()) {
          size_t n = std::min(hash.size(), sizeof(uint64_t));
          memcpy(&actualFromHash, hash.data(), n);
        }
        bool isLossy = (actualFromHash != entry.canonicalHash);
        entry.qualityCode = computeQualityCode(entry.format, isLossy);
      }

    entry.key = CacheKey(hash.data());
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
  
  // Delete any shard files that are no longer referenced by the index.
  // Without this, restarts can leak disk usage because shardSizes_ is rebuilt
  // solely from indexMap_ and would ignore orphan shard files left behind by
  // earlier GC/index rewrites.
  size_t orphanReclaimed = cleanupOrphanShardsOnDisk();
  if (orphanReclaimed > 0) {
    vlog.info("PersistentCache: removed %zuMB of orphan shard files during load",
              orphanReclaimed / (1024*1024));
  }

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
  
  // Restore hashes
  entry.canonicalHash = idx.canonicalHash;
  entry.actualHash = cacheKeyFirstU64(idx.key);
  
  // Use the key stored in the index so CacheKey/ContentHash mapping stays
  // consistent across disk and memory.
  CacheKey key = idx.key;
  cache_[key] = entry;
  if (arcCache_)
    arcCache_->insert(key, entry);
  
  // Mark as hot (no longer cold)
  it->second.isCold = false;
  coldEntries_.erase(hash);
  indexDirty_ = true;
  
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
  size_t flushed = 0;

  // Write dirty payloads to shard files. If disk is full, try to reclaim
  // space by evicting cold entries and deleting orphan shards, then retry.
  if (!dirtyEntries_.empty()) {
    std::vector<std::vector<uint8_t>> toWrite;
    toWrite.reserve(dirtyEntries_.size());
    for (const auto& h : dirtyEntries_)
      toWrite.push_back(h);

    for (const auto& hash : toWrite) {
      auto keyIt = hashToKey_.find(hash);
      if (keyIt == hashToKey_.end()) {
        dirtyEntries_.erase(hash);
        continue;
      }

      auto cacheIt = cache_.find(keyIt->second);
      if (cacheIt == cache_.end()) {
        // Entry was evicted from RAM before we could persist it.
        dirtyEntries_.erase(hash);
        continue;
      }

      bool ok = writeEntryToShard(hash, cacheIt->second);
      if (!ok) {
        // Best-effort recovery: trim cold entries and orphan shards to
        // free disk, then retry once.
        garbageCollect();
        cleanupOrphanShardsOnDisk();
        ok = writeEntryToShard(hash, cacheIt->second);
      }

      if (ok) {
        flushed++;
        dirtyEntries_.erase(hash);
        indexDirty_ = true;
      }
    }
  }

  // Save the index (atomically) if anything changed. If this fails due to
  // lack of disk space, keep indexDirty_=true so we can retry later without
  // corrupting the existing index.
  if (indexDirty_) {
    if (saveToDisk()) {
      indexDirty_ = false;
      vlog.debug("PersistentCache: index saved (%zu entries)", indexMap_.size());
    } else {
      // Try once more after reclaiming space.
      garbageCollect();
      cleanupOrphanShardsOnDisk();
      if (saveToDisk()) {
        indexDirty_ = false;
        vlog.debug("PersistentCache: index saved after GC (%zu entries)", indexMap_.size());
      }
    }
  }

  // Enforce maxDiskSize_ as best-effort. This is primarily a safety valve;
  // if the user requested a very large disk cache then maxDiskSize_ may be
  // larger than the available disk.
  size_t diskUsage = getDiskUsage();
  if (diskUsage > maxDiskSize_) {
    vlog.debug("PersistentCache: disk usage %zuMB exceeds limit %zuMB, triggering GC",
               diskUsage / (1024*1024), maxDiskSize_ / (1024*1024));
    size_t reclaimed = garbageCollect();
    if (reclaimed > 0)
      saveToDisk();
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
  // Remove from index and related maps.
  for (const auto& hash : toRemove) {
    auto itKey = hashToKey_.find(hash);
    if (itKey != hashToKey_.end()) {
      keyToHash_.erase(itKey->second);
      hashToKey_.erase(itKey);
    }
    indexMap_.erase(hash);
    coldEntries_.erase(hash);
    hydrationQueue_.remove(hash);
  }
  if (!toRemove.empty())
    indexDirty_ = true;

  // Delete any shard files that became unreferenced as a result of trimming.
  // This is the key step that actually reclaims disk space.
  size_t orphanReclaimed = cleanupOrphanShardsOnDisk();
  reclaimed += orphanReclaimed;

  if (reclaimed > 0) {
    vlog.debug("PersistentCache: GC reclaimed %zuKB (%zu entries, %zuKB orphan shards)",
               reclaimed / 1024,
               toRemove.size(),
               orphanReclaimed / 1024);
  }

  return reclaimed;
}

bool GlobalClientPersistentCache::saveToDisk()
{
  closeCurrentShard();

  if (!ensureCacheDir())
    return false;

  // Best-effort cleanup of orphan shards so we don't waste disk space.
  cleanupOrphanShardsOnDisk();

  std::string indexPath = getIndexPath();
  std::string tmpPath = indexPath + ".tmp";

  FILE* f = fopen(tmpPath.c_str(), "wb");
  if (!f) {
    vlog.error("PersistentCache: failed to open temporary index for writing: %s", strerror(errno));
    return false;
  }

  auto writeOrFail = [&](const void* ptr, size_t sz, size_t n, const char* what) -> bool {
    if (fwrite(ptr, sz, n, f) != n) {
      int err = errno;
      vlog.error("PersistentCache: failed writing %s to %s: %s", what, tmpPath.c_str(), strerror(err));
      return false;
    }
    return true;
  };

  // Find max shard ID
  uint16_t maxShardId = 0;
  for (const auto& entry : indexMap_) {
    if (entry.second.shardId > maxShardId)
      maxShardId = entry.second.shardId;
  }

  // Write v5 index header
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
  header.magic = 0x50435633;  // "PCV3" (magic stays same, version bumped)
  header.version = 7;  // v7 adds qualityCode
  header.entryCount = indexMap_.size();
  header.created = time(nullptr);
  header.lastAccess = time(nullptr);
  header.maxShardId = maxShardId;

  if (!writeOrFail(&header, sizeof(header), 1, "index header")) {
    fclose(f);
    remove(tmpPath.c_str());
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
    if (!writeOrFail(hashBuf, 1, 16, "hash")) {
      fclose(f);
      remove(tmpPath.c_str());
      return false;
    }

    if (!writeOrFail(&idx.shardId, sizeof(idx.shardId), 1, "shardId") ||
        !writeOrFail(&idx.payloadOffset, sizeof(idx.payloadOffset), 1, "payloadOffset") ||
        !writeOrFail(&idx.payloadSize, sizeof(idx.payloadSize), 1, "payloadSize") ||
        !writeOrFail(&idx.width, sizeof(idx.width), 1, "width") ||
        !writeOrFail(&idx.height, sizeof(idx.height), 1, "height") ||
        !writeOrFail(&idx.stridePixels, sizeof(idx.stridePixels), 1, "stridePixels")) {
      fclose(f);
      remove(tmpPath.c_str());
      return false;
    }
    
    // Write PixelFormat in VNC wire format (16 bytes)
    // This matches the format used by PixelFormat::read()/write() methods
    struct {
      uint8_t bpp;
      uint8_t depth;
      uint8_t bigEndian;
      uint8_t trueColour;
      uint16_t redMax;
      uint16_t greenMax;
      uint16_t blueMax;
      uint8_t redShift;
      uint8_t greenShift;
      uint8_t blueShift;
      uint8_t padding[3];
    } pfData;  // 16 bytes total (VNC wire format)
    
    pfData.bpp = (uint8_t)idx.format.bpp;
    pfData.depth = (uint8_t)idx.format.depth;
    pfData.trueColour = idx.format.trueColour ? 1 : 0;
    pfData.bigEndian = idx.format.isBigEndian() ? 1 : 0;
    
    // Access protected fields via raw memory (same platform, same layout)
    // These fields are at fixed offsets from the start of PixelFormat:
    // redMax at offset 12, greenMax at 16, blueMax at 20
    // redShift at 24, greenShift at 28, blueShift at 32
    const uint8_t* pfRaw = reinterpret_cast<const uint8_t*>(&idx.format);
    int32_t redMax, greenMax, blueMax, redShift, greenShift, blueShift;
    memcpy(&redMax, pfRaw + 12, 4);
    memcpy(&greenMax, pfRaw + 16, 4);
    memcpy(&blueMax, pfRaw + 20, 4);
    memcpy(&redShift, pfRaw + 24, 4);
    memcpy(&greenShift, pfRaw + 28, 4);
    memcpy(&blueShift, pfRaw + 32, 4);
    
    pfData.redMax = (uint16_t)redMax;
    pfData.greenMax = (uint16_t)greenMax;
    pfData.blueMax = (uint16_t)blueMax;
    pfData.redShift = (uint8_t)redShift;
    pfData.greenShift = (uint8_t)greenShift;
    pfData.blueShift = (uint8_t)blueShift;
    pfData.padding[0] = 0;
    pfData.padding[1] = 0;
    pfData.padding[2] = 0;
    
    if (!writeOrFail(&pfData, sizeof(pfData), 1, "PixelFormat")) {
      fclose(f);
      remove(tmpPath.c_str());
      return false;
    }

    uint8_t flags = idx.isCold ? 0x01 : 0x00;
    if (!writeOrFail(&flags, 1, 1, "flags") ||
        !writeOrFail(&idx.canonicalHash, sizeof(idx.canonicalHash), 1, "canonicalHash") ||
        !writeOrFail(&idx.qualityCode, sizeof(idx.qualityCode), 1, "qualityCode")) {
      fclose(f);
      remove(tmpPath.c_str());
      return false;
    }
  }

  if (fclose(f) != 0) {
    int err = errno;
    vlog.error("PersistentCache: failed to close %s: %s", tmpPath.c_str(), strerror(err));
    remove(tmpPath.c_str());
    return false;
  }

  // Atomically replace the old index. If rename fails, keep the old index.
  if (rename(tmpPath.c_str(), indexPath.c_str()) != 0) {
    int err = errno;
    vlog.error("PersistentCache: failed to rename %s to %s: %s",
               tmpPath.c_str(), indexPath.c_str(), strerror(err));
    remove(tmpPath.c_str());
    return false;
  }

  vlog.debug("PersistentCache: saved v7 index with %zu entries", indexMap_.size());

  return true;
}

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}

std::string GlobalClientPersistentCache::dumpDebugState(const std::string& outputDir) const
{
  // Generate timestamped filename
  time_t now = time(nullptr);
  struct tm tmNow;
  localtime_r(&now, &tmNow);
  char timestamp[32];
  strftime(timestamp, sizeof(timestamp), "%Y%m%d_%H%M%S", &tmNow);
  
  std::string dumpPath = outputDir + "/pcache_debug_" + timestamp + ".txt";
  
  FILE* f = fopen(dumpPath.c_str(), "w");
  if (!f) {
    vlog.error("Failed to open debug dump file: %s", dumpPath.c_str());
    return "";
  }
  
  fprintf(f, "=== PersistentCache Debug Dump ===\n");
  fprintf(f, "Timestamp: %s\n", timestamp);
  fprintf(f, "Cache directory: %s\n", cacheDir_.c_str());
  fprintf(f, "\n=== Configuration ===\n");
  fprintf(f, "Max memory size: %zu bytes (%.1f MB)\n", maxMemorySize_, maxMemorySize_ / (1024.0 * 1024.0));
  fprintf(f, "Max disk size: %zu bytes (%.1f MB)\n", maxDiskSize_, maxDiskSize_ / (1024.0 * 1024.0));
  fprintf(f, "Shard size: %zu bytes (%.1f MB)\n", shardSize_, shardSize_ / (1024.0 * 1024.0));
  
  fprintf(f, "\n=== Statistics ===\n");
  fprintf(f, "Total entries: %zu\n", stats_.totalEntries);
  fprintf(f, "Total bytes: %zu\n", stats_.totalBytes);
  fprintf(f, "Cache hits: %llu\n", (unsigned long long)stats_.cacheHits);
  fprintf(f, "Cache misses: %llu\n", (unsigned long long)stats_.cacheMisses);
  fprintf(f, "Evictions: %llu\n", (unsigned long long)stats_.evictions);
  fprintf(f, "T1 size: %zu, T2 size: %zu\n", stats_.t1Size, stats_.t2Size);
  fprintf(f, "B1 size: %zu, B2 size: %zu\n", stats_.b1Size, stats_.b2Size);
  fprintf(f, "Target T1 size (p): %zu\n", stats_.targetT1Size);
  
  fprintf(f, "\n=== Hydration State ===\n");
  const char* stateStr = "Unknown";
  switch (hydrationState_) {
    case HydrationState::Uninitialized: stateStr = "Uninitialized"; break;
    case HydrationState::IndexLoaded: stateStr = "IndexLoaded"; break;
    case HydrationState::PartiallyHydrated: stateStr = "PartiallyHydrated"; break;
    case HydrationState::FullyHydrated: stateStr = "FullyHydrated"; break;
  }
  fprintf(f, "Hydration state: %s\n", stateStr);
  fprintf(f, "Hydration queue size: %zu\n", hydrationQueue_.size());
  fprintf(f, "Cold entries (on disk, not in memory): %zu\n", coldEntries_.size());
  fprintf(f, "Dirty entries (pending disk write): %zu\n", dirtyEntries_.size());
  fprintf(f, "Index dirty: %s\n", indexDirty_ ? "yes" : "no");
  
  fprintf(f, "\n=== In-Memory Cache Entries (%zu) ===\n", cache_.size());
  size_t entryNum = 0;
  for (const auto& kv : cache_) {
    const CacheKey& key = kv.first;
    const CachedPixels& entry = kv.second;
    
    char hexKey[33];
    cacheKeyToHex(key, hexKey);
    fprintf(f, "\nEntry %zu:\n", entryNum++);
    fprintf(f, "  Key: %s (u64=0x%016llx)\n",
            hexKey, (unsigned long long)cacheKeyFirstU64(key));
    fprintf(f, "  Canonical hash: 0x%016llx\n", (unsigned long long)entry.canonicalHash);
    fprintf(f, "  Actual hash: 0x%016llx\n", (unsigned long long)entry.actualHash);
    fprintf(f, "  Is lossless: %s\n", entry.isLossless() ? "yes" : "no");
    fprintf(f, "  Dimensions: %ux%u, stride=%u pixels\n", entry.width, entry.height, entry.stridePixels);
    fprintf(f, "  Pixel format: depth=%d bpp=%d\n", entry.format.depth, entry.format.bpp);
    fprintf(f, "  Pixel data size: %zu bytes\n", entry.pixels.size());
    fprintf(f, "  Is hydrated: %s\n", entry.isHydrated() ? "yes" : "no");
    fprintf(f, "  Last access time: %u\n", entry.lastAccessTime);
    
    // Check for suspicious patterns in pixel data
    if (entry.isHydrated() && !entry.pixels.empty()) {
      bool allZero = true;
      bool allSame = true;
      uint8_t firstByte = entry.pixels[0];
      for (size_t i = 0; i < entry.pixels.size() && (allZero || allSame); i++) {
        if (entry.pixels[i] != 0) allZero = false;
        if (entry.pixels[i] != firstByte) allSame = false;
      }
      if (allZero) {
        fprintf(f, "  WARNING: All pixel bytes are zero (black rect)!\n");
      } else if (allSame) {
        fprintf(f, "  Note: All pixel bytes are same value (0x%02x)\n", firstByte);
      }
      
      // Sample first few bytes for debugging
      fprintf(f, "  First 16 bytes: ");
      for (size_t i = 0; i < std::min(entry.pixels.size(), (size_t)16); i++) {
        fprintf(f, "%02x ", entry.pixels[i]);
      }
      fprintf(f, "\n");
    }
    
    // Limit output for very large caches
    if (entryNum >= 100) {
      fprintf(f, "\n... (truncated, %zu more entries)\n", cache_.size() - entryNum);
      break;
    }
  }
  
  fprintf(f, "\n=== Index Map Entries (%zu) ===\n", indexMap_.size());
  size_t idxNum = 0;
  for (const auto& kv : indexMap_) {
    const std::vector<uint8_t>& hash = kv.first;
    const IndexEntry& idx = kv.second;
    
    fprintf(f, "\nIndex entry %zu:\n", idxNum++);
    fprintf(f, "  Hash (first 16 bytes): ");
    for (size_t i = 0; i < std::min(hash.size(), (size_t)16); i++) {
      fprintf(f, "%02x", hash[i]);
    }
    fprintf(f, "\n");
    fprintf(f, "  Shard: %u, offset: %u, size: %u\n", idx.shardId, idx.payloadOffset, idx.payloadSize);
    fprintf(f, "  Dimensions: %ux%u, stride=%u\n", idx.width, idx.height, idx.stridePixels);
    fprintf(f, "  Canonical hash: 0x%016llx\n", (unsigned long long)idx.canonicalHash);
    fprintf(f, "  Is cold: %s\n", idx.isCold ? "yes" : "no");
    
    if (idxNum >= 100) {
      fprintf(f, "\n... (truncated, %zu more index entries)\n", indexMap_.size() - idxNum);
      break;
    }
  }
  
  fprintf(f, "\n=== Pending Evictions (%zu) ===\n", pendingEvictions_.size());
  for (size_t i = 0; i < std::min(pendingEvictions_.size(), (size_t)20); i++) {
    const CacheKey& k = pendingEvictions_[i];
    fprintf(f, "  ");
    // Print first 8 bytes of the key for compactness
    for (size_t j = 0; j < 8; j++) {
      fprintf(f, "%02x", (unsigned)k.bytes[j]);
    }
    fprintf(f, "\n");
  }
  
  fprintf(f, "\n=== End of Debug Dump ===\n");
  fclose(f);
  
  vlog.info("PersistentCache debug state dumped to: %s", dumpPath.c_str());
  return dumpPath;
}

// ============================================================================
// Multi-Viewer Cache Coordination
// ============================================================================

bool GlobalClientPersistentCache::startCoordinator()
{
  std::lock_guard<std::mutex> lock(coordinatorMutex_);
  
  if (coordinator_ && coordinator_->isRunning())
    return true;
  
  // Create coordinator with callbacks
  auto indexCb = [this](const std::vector<cache::WireIndexEntry>& entries) {
    onIndexUpdate(entries);
  };
  auto writeCb = [this](const cache::WireIndexEntry& entry,
                        const std::vector<uint8_t>& payload,
                        cache::WireIndexEntry& result) -> bool {
    return onWriteRequest(entry, payload, result);
  };
  
  coordinator_ = cache::CacheCoordinator::create(cacheDir_, indexCb, writeCb);
  
  if (!coordinator_) {
    vlog.error("Failed to create cache coordinator");
    return false;
  }
  
  if (!coordinator_->start()) {
    vlog.error("Failed to start cache coordinator");
    coordinator_.reset();
    return false;
  }
  
  const char* roleStr = "unknown";
  switch (coordinator_->role()) {
    case cache::CacheCoordinator::Role::Master: roleStr = "master"; break;
    case cache::CacheCoordinator::Role::Slave: roleStr = "slave"; break;
    case cache::CacheCoordinator::Role::Standalone: roleStr = "standalone"; break;
    default: break;
  }
  vlog.info("Cache coordinator started as %s", roleStr);
  
  return true;
}

void GlobalClientPersistentCache::stopCoordinator()
{
  std::lock_guard<std::mutex> lock(coordinatorMutex_);
  
  if (coordinator_) {
    coordinator_->stop();
    coordinator_.reset();
    vlog.debug("Cache coordinator stopped");
  }
}

cache::CacheCoordinator::Role GlobalClientPersistentCache::getCoordinatorRole() const
{
  std::lock_guard<std::mutex> lock(coordinatorMutex_);
  if (!coordinator_)
    return cache::CacheCoordinator::Role::Uninitialized;
  return coordinator_->role();
}

cache::CacheCoordinator::Stats GlobalClientPersistentCache::getCoordinatorStats() const
{
  std::lock_guard<std::mutex> lock(coordinatorMutex_);
  if (!coordinator_)
    return cache::CacheCoordinator::Stats{};
  return coordinator_->getStats();
}

void GlobalClientPersistentCache::onIndexUpdate(const std::vector<cache::WireIndexEntry>& entries)
{
  // Called when we (as slave) receive index updates from master.
  // Add these entries to our index so we can hydrate them on demand.
  vlog.debug("Received %zu index updates from coordinator", entries.size());
  
  for (const auto& wireEntry : entries) {
    // Convert WireIndexEntry to internal IndexEntry
    std::vector<uint8_t> hash(wireEntry.hash, wireEntry.hash + 16);
    
    // Check if we already have this entry
    if (indexMap_.find(hash) != indexMap_.end())
      continue;
    
    IndexEntry idx;
    idx.shardId = wireEntry.shardId;
    idx.payloadOffset = wireEntry.payloadOffset;
    idx.payloadSize = wireEntry.payloadSize;
    idx.width = wireEntry.width;
    idx.height = wireEntry.height;
    idx.stridePixels = wireEntry.width;  // Stored contiguously
    idx.canonicalHash = wireEntry.canonicalHash;
    idx.qualityCode = wireEntry.qualityCode;
    idx.isCold = true;  // Not in our memory yet
    
    // Reconstruct pixel format from qualityCode
    uint8_t depthCode = (wireEntry.qualityCode >> 1) & 0x03;
    switch (depthCode) {
      case 0: idx.format.bpp = 8; idx.format.depth = 8; break;
      case 1: idx.format.bpp = 16; idx.format.depth = 16; break;
      default: idx.format.bpp = 32; idx.format.depth = 24; break;
    }
    
    // Set up CacheKey (unified)
    idx.key = CacheKey(wireEntry.hash);
    
    indexMap_[hash] = idx;
    coldEntries_.insert(hash);
    hashToKey_[hash] = idx.key;
    
    // Add to hydration queue for potential background loading
    hydrationQueue_.push_back(hash);
  }
}

bool GlobalClientPersistentCache::onWriteRequest(const cache::WireIndexEntry& wireEntry,
                                                 const std::vector<uint8_t>& payload,
                                                 cache::WireIndexEntry& resultEntry)
{
  // Called when we (as master) receive a write request from a slave.
  // We need to write the payload to our shard and return the result.
  
  std::vector<uint8_t> hash(wireEntry.hash, wireEntry.hash + 16);
  
  // Check if we already have this entry
  if (indexMap_.find(hash) != indexMap_.end()) {
    // Already have it - return existing entry info
    const IndexEntry& existing = indexMap_[hash];
    memcpy(resultEntry.hash, hash.data(), 16);
    resultEntry.shardId = existing.shardId;
    resultEntry.payloadOffset = existing.payloadOffset;
    resultEntry.payloadSize = existing.payloadSize;
    resultEntry.width = existing.width;
    resultEntry.height = existing.height;
    resultEntry.canonicalHash = existing.canonicalHash;
    resultEntry.actualHash = cacheKeyFirstU64(existing.key);
    resultEntry.qualityCode = existing.qualityCode;
    return true;
  }
  
  // Build a CachedPixels from the wire entry and payload
  CachedPixels entry;
  entry.pixels = payload;
  entry.width = wireEntry.width;
  entry.height = wireEntry.height;
  entry.stridePixels = wireEntry.width;  // Stored contiguously
  entry.canonicalHash = wireEntry.canonicalHash;
  entry.actualHash = wireEntry.actualHash;
  entry.lastAccessTime = getCurrentTime();
  
  // Reconstruct pixel format from qualityCode
  uint8_t depthCode = (wireEntry.qualityCode >> 1) & 0x03;
  switch (depthCode) {
    case 0: entry.format.bpp = 8; entry.format.depth = 8; break;
    case 1: entry.format.bpp = 16; entry.format.depth = 16; break;
    default: entry.format.bpp = 32; entry.format.depth = 24; break;
  }
  
  // Write to shard
  if (!writeEntryToShard(hash, entry)) {
    vlog.error("Failed to write slave's entry to shard");
    return false;
  }
  
  // Return result
  const IndexEntry& idx = indexMap_[hash];
  memcpy(resultEntry.hash, hash.data(), 16);
  resultEntry.shardId = idx.shardId;
  resultEntry.payloadOffset = idx.payloadOffset;
  resultEntry.payloadSize = idx.payloadSize;
  resultEntry.width = idx.width;
  resultEntry.height = idx.height;
  resultEntry.canonicalHash = idx.canonicalHash;
  resultEntry.actualHash = wireEntry.actualHash;
  resultEntry.qualityCode = idx.qualityCode;
  
  vlog.debug("Wrote entry for slave: %dx%d, shard=%u, offset=%u",
             idx.width, idx.height, idx.shardId, idx.payloadOffset);
  
  return true;
}
