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

#include <rfb/ContentCache.h>
#include <core/LogWriter.h>

#include <time.h>
#include <cstring>
#include <algorithm>
#include <iomanip>     //DebugContentCache_2025-10-14

using namespace rfb;

static core::LogWriter vlog("ContentCache");

//DebugContentCache_2025-10-14 - Start debug logger implementation
ContentCacheDebugLogger::ContentCacheDebugLogger() {
  auto now = std::chrono::system_clock::now();
  auto time_val = std::chrono::system_clock::to_time_t(now);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
    now.time_since_epoch()) % 1000;
  
  std::string timestamp = std::to_string(time_val) + "_" + std::to_string(ms.count());
  logFilename_ = "/tmp/contentcache_debug_" + timestamp + ".log";
  
  logFile_.open(logFilename_, std::ios::out | std::ios::app);
  if (logFile_.is_open()) {
    std::cout << "ContentCache debug log: " << logFilename_ << std::endl;
    log("=== ContentCache Debug Log Started ===");
  } else {
    std::cerr << "Failed to open ContentCache debug log: " << logFilename_ << std::endl;
  }
}

ContentCacheDebugLogger::~ContentCacheDebugLogger() {
  if (logFile_.is_open()) {
    log("=== ContentCache Debug Log Ended ===");
    logFile_.close();
  }
}

void ContentCacheDebugLogger::log(const std::string& message) {
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
//DebugContentCache_2025-10-14 - End debug logger implementation

// ============================================================================
// Fast Hash Function (FNV-1a variant)
// ============================================================================

// FNV-1a is simple, fast, and has good distribution
// Good enough for content comparison without crypto overhead
static uint64_t fnv1a_hash(const uint8_t* data, size_t len)
{
  const uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
  const uint64_t FNV_PRIME = 0x100000001b3ULL;
  
  uint64_t hash = FNV_OFFSET;
  for (size_t i = 0; i < len; i++) {
    hash ^= data[i];
    hash *= FNV_PRIME;
  }
  return hash;
}

uint64_t rfb::computeContentHash(const uint8_t* data, size_t len)
{
  if (data == nullptr || len == 0)
    return 0;
  
  return fnv1a_hash(data, len);
}

uint64_t rfb::computeSampledHash(const uint8_t* data,
                                  size_t width, size_t height,
                                  size_t stridePixels, size_t bytesPerPixel,
                                  size_t sampleRate)
{
  if (data == nullptr || width == 0 || height == 0)
    return 0;
  
  const uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
  const uint64_t FNV_PRIME = 0x100000001b3ULL;
  
  uint64_t hash = FNV_OFFSET;
  
  // Sample every Nth pixel
  for (size_t y = 0; y < height; y += sampleRate) {
    const uint8_t* row = data + (y * stridePixels * bytesPerPixel);
    for (size_t x = 0; x < width; x += sampleRate) {
      const uint8_t* pixel = row + (x * bytesPerPixel);
      for (size_t b = 0; b < bytesPerPixel; b++) {
        hash ^= pixel[b];
        hash *= FNV_PRIME;
      }
    }
  }
  
  return hash;
}

// ============================================================================
// ContentCache Implementation - ARC Algorithm
// ============================================================================

ContentCache::ContentCache(size_t maxSizeMB, uint32_t maxAgeSec)
  : nextCacheId_(1),  // Start from 1 (0 is reserved for "clear all")
    p_(0),
    maxCacheSize_(maxSizeMB * 1024 * 1024),
    maxAge_(maxAgeSec),
    t1Size_(0),
    t2Size_(0),
    maxPixelCacheSize_(maxSizeMB * 1024 * 1024)  // Same limit as hash cache
{
  //DebugContentCache_2025-10-14
  ContentCacheDebugLogger::getInstance().log("ContentCache constructor ENTER: maxSizeMB=" + std::to_string(maxSizeMB) + ", maxAgeSec=" + std::to_string(maxAgeSec));
  
  memset(&stats_, 0, sizeof(stats_));
  vlog.debug("ContentCache created with ARC: maxSize=%zuMB, maxAge=%us",
             maxSizeMB, maxAgeSec);
             
  // Initialize shared ArcCache for client pixel cache
  arcPixelCache_.reset(new rfb::cache::ArcCache<ContentKey, CachedPixels, ContentKeyHash>(
      maxPixelCacheSize_,
      [](const CachedPixels& e) { return e.bytes; },
      nullptr /* no eviction notification from pixel cache at present */
  ));

  //DebugContentCache_2025-10-14
  ContentCacheDebugLogger::getInstance().log("ContentCache constructor EXIT: initialized successfully");
}

ContentCache::~ContentCache()
{
  vlog.debug("ContentCache destroyed: %zu entries, T1=%zu T2=%zu",
             cache_.size(), t1_.size(), t2_.size());
}

ContentCache::CacheEntry* ContentCache::findContent(const ContentKey& key)
{
  auto it = cache_.find(key);
  if (it == cache_.end()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  stats_.cacheHits++;
  it->second.hitCount++;
  it->second.lastSeenTime = getCurrentTime();
  
  // ARC policy: move from T1 to T2 on second access
  auto listIt = listMap_.find(key);
  if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
    moveToT2(key);
  }
  
  return &(it->second);
}

uint64_t ContentCache::insertContent(const ContentKey& key,
                                    const core::Rect& bounds,
                                    const uint8_t* data,
                                    size_t dataLen,
                                    bool keepData)
{
  // Check if already in cache
  auto cacheIt = cache_.find(key);
  if (cacheIt != cache_.end()) {
    // Update existing entry
    cacheIt->second.lastBounds = bounds;
    cacheIt->second.lastSeenTime = getCurrentTime();
    
    // Move to T2 if currently in T1
    auto listIt = listMap_.find(key);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(key);
    }
    return cacheIt->second.cacheId;
  }
  
  // Check ghost lists (recently evicted)
  auto listIt = listMap_.find(key);
  
  if (listIt != listMap_.end() && listIt->second.list == LIST_B1) {
    // Cache hit in B1: adapt by increasing p (favor recency)
    size_t delta = (b2_.size() >= b1_.size()) ? 1 : (b2_.size() / b1_.size());
    p_ = std::min(maxCacheSize_, p_ + delta * dataLen);
    
    // Make room and insert into T2 (it's been accessed twice now)
    replace(key, dataLen);
    
    // Remove from B1
    b1_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else if (listIt != listMap_.end() && listIt->second.list == LIST_B2) {
    // Cache hit in B2: adapt by decreasing p (favor frequency)
    size_t delta = (b1_.size() >= b2_.size()) ? 1 : (b1_.size() / b2_.size());
    p_ = (delta * dataLen > p_) ? 0 : p_ - delta * dataLen;
    
    // Make room and insert into T2
    replace(key, dataLen);
    
    // Remove from B2
    b2_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else {
    // New entry: make room and insert into T1
    replace(key, dataLen);
  }
  
  // Create the entry
  CacheEntry entry(key.contentHash, bounds, getCurrentTime());
  entry.dataSize = dataLen;
  entry.cacheId = getNextCacheId();  // Assign new cache ID
  
  if (keepData && data != nullptr && dataLen > 0) {
    entry.data.assign(data, data + dataLen);
  }
  
  // Insert into cache and T2 (or T1 for new items)
  cache_[key] = entry;
  
  // Register cache ID mappings
  keyToCacheId_[key] = entry.cacheId;
  cacheIdToKey_[entry.cacheId] = key;
  
  // Determine which list to add to
  bool wasInGhost = (listIt != listMap_.end() && 
                     (listIt->second.list == LIST_B1 || listIt->second.list == LIST_B2));
  
  if (wasInGhost) {
    // Was in ghost list, add to T2
    t2_.push_front(key);
    listMap_[key].list = LIST_T2;
    listMap_[key].iter = t2_.begin();
    t2Size_ += dataLen;
  } else {
    // New entry, add to T1
    t1_.push_front(key);
    listMap_[key].list = LIST_T1;
    listMap_[key].iter = t1_.begin();
    t1Size_ += dataLen;
  }
  
  stats_.totalEntries++;
  stats_.totalBytes += dataLen;
  
  vlog.debug("Inserted: key=(%ux%u,hash=%016llx) cacheId=%llu size=%zu T1=%zu/%zu T2=%zu p=%zu",
             key.width, key.height, (unsigned long long)key.contentHash, 
             (unsigned long long)entry.cacheId, dataLen, 
             t1Size_, t1_.size(), t2_.size(), p_);
  
  return entry.cacheId;
}

void ContentCache::touchEntry(const ContentKey& key)
{
  auto it = cache_.find(key);
  if (it != cache_.end()) {
    it->second.lastSeenTime = getCurrentTime();
    
    // Move from T1 to T2 if accessed again
    auto listIt = listMap_.find(key);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(key);
    }
  }
}

void ContentCache::pruneCache()
{
  uint32_t now = getCurrentTime();
  size_t evictedCount = 0;
  
  // Remove aged entries from all lists
  auto pruneList = [&](std::list<ContentKey>& list, size_t& listSize, 
                       bool removeData) {
    for (auto it = list.begin(); it != list.end(); ) {
      auto cacheIt = cache_.find(*it);
      
      if (cacheIt != cache_.end() && 
          now - cacheIt->second.lastSeenTime > maxAge_) {
        
        if (removeData) {
          listSize -= cacheIt->second.dataSize;
          cache_.erase(cacheIt);
          stats_.totalEntries--;
        }
        
        listMap_.erase(*it);
        it = list.erase(it);
        evictedCount++;
      } else {
        ++it;
      }
    }
  };
  
  pruneList(t1_, t1Size_, true);
  pruneList(t2_, t2Size_, true);
  pruneList(b1_, t1Size_, false);  // Ghost entries don't affect size
  pruneList(b2_, t2Size_, false);
  
  stats_.evictions += evictedCount;
  
  if (evictedCount > 0) {
    vlog.debug("Pruned %zu aged entries", evictedCount);
  }
}

void ContentCache::clear()
{
  cache_.clear();
  t1_.clear();
  t2_.clear();
  b1_.clear();
  b2_.clear();
  listMap_.clear();
  
  t1Size_ = 0;
  t2Size_ = 0;
  p_ = 0;
  
  stats_.totalEntries = 0;
  stats_.totalBytes = 0;
  
  vlog.debug("Cache cleared");
}

ContentCache::Stats ContentCache::getStats() const
{
  Stats current = stats_;
  
  // If pixel ArcCache exists, report pixel cache stats (client-side)
  // Otherwise report hash cache stats (server-side)
  if (arcPixelCache_) {
    auto ps = arcPixelCache_->getStats();
    current.totalEntries = ps.totalEntries;
    current.totalBytes = ps.totalBytes;
    current.t1Size = ps.t1Size;
    current.t2Size = ps.t2Size;
    current.b1Size = ps.b1Size;
    current.b2Size = ps.b2Size;
    current.targetT1Size = ps.targetT1Size;
  } else {
    // Use hash cache stats (already in stats_)
    current.t1Size = t1_.size();
    current.t2Size = t2_.size();
    current.b1Size = b1_.size();
    current.b2Size = b2_.size();
    current.targetT1Size = p_;
  }
  
  return current;
}

void ContentCache::resetStats()
{
  stats_.cacheHits = 0;
  stats_.cacheMisses = 0;
  stats_.evictions = 0;
  stats_.collisions = 0;
}

void ContentCache::logArcStats() const
{
  Stats s = getStats();
  
  uint64_t totalLookups = s.cacheHits + s.cacheMisses;
  double hitRate = (totalLookups > 0) ? (100.0 * s.cacheHits / totalLookups) : 0.0;
  
  size_t usedMB = s.totalBytes / (1024 * 1024);
  size_t maxMB = maxCacheSize_ / (1024 * 1024);
  double utilization = (maxCacheSize_ > 0) ? (100.0 * s.totalBytes / maxCacheSize_) : 0.0;
  
  // ARC balance: p_ is target size for T1 in bytes
  double t1Target = (maxCacheSize_ > 0) ? (100.0 * s.targetT1Size / maxCacheSize_) : 0.0;
  double t1Actual = (maxCacheSize_ > 0) ? (100.0 * t1Size_ / maxCacheSize_) : 0.0;
  double t2Actual = (maxCacheSize_ > 0) ? (100.0 * t2Size_ / maxCacheSize_) : 0.0;
  
  vlog.info("=== ARC Cache Statistics ===");
  vlog.info("Hit rate: %.1f%% (%llu hits, %llu misses, %llu total)",
            hitRate, (unsigned long long)s.cacheHits, 
            (unsigned long long)s.cacheMisses, (unsigned long long)totalLookups);
  vlog.info("Memory: %zuMB / %zuMB (%.1f%% used), %zu entries, %llu evictions",
            usedMB, maxMB, utilization, s.totalEntries, 
            (unsigned long long)s.evictions);
  vlog.info("ARC balance: T1=%zu (%.1f%%, target %.1f%%), T2=%zu (%.1f%%)",
            s.t1Size, t1Actual, t1Target, s.t2Size, t2Actual);
  vlog.info("Ghost lists: B1=%zu, B2=%zu (adaptation hints)",
            s.b1Size, s.b2Size);
  
  if (s.collisions > 0) {
    vlog.info("Hash collisions: %llu", (unsigned long long)s.collisions);
  }
}

size_t ContentCache::getTotalBytes() const
{
  // Calculate total bytes from hash cache (server-side)
  size_t hashCacheBytes = t1Size_ + t2Size_;
  
  // Calculate total bytes from pixel cache (client-side)
  size_t pixelCacheBytes = 0;
  if (arcPixelCache_) {
    auto ps = arcPixelCache_->getStats();
    pixelCacheBytes = ps.totalBytes;
  }
  
  return hashCacheBytes + pixelCacheBytes;
}

void ContentCache::setMaxSize(size_t maxSizeMB)
{
  maxCacheSize_ = maxSizeMB * 1024 * 1024;
  vlog.debug("Cache max size set to %zuMB", maxSizeMB);
  // Recreate pixel ArcCache with new capacity
  arcPixelCache_.reset(new rfb::cache::ArcCache<ContentKey, CachedPixels, ContentKeyHash>(
    maxPixelCacheSize_,
    [](const CachedPixels& e) { return e.bytes; },
    nullptr));
  
  // Evict from hash cache if over the limit
  while (t1Size_ + t2Size_ > maxCacheSize_) {
    if (!t1_.empty() || !t2_.empty()) {
      replace(ContentKey(), 0);  // Force eviction with dummy key
    } else {
      break;
    }
  }
}

void ContentCache::setMaxAge(uint32_t maxAgeSec)
{
  maxAge_ = maxAgeSec;
  vlog.debug("Cache max age set to %us", maxAgeSec);
}

// ============================================================================
// ARC Algorithm Implementation
// ============================================================================

void ContentCache::replace(const ContentKey&, size_t size)
{
  // Make room for new entry of given size
  while (t1Size_ + t2Size_ + size > maxCacheSize_) {
    
    // Case 1: T1 is larger than target p, evict from T1
    if (!t1_.empty() && 
        (t1Size_ > p_ || (t2_.empty() && t1Size_ == p_))) {
      
      ContentKey victim = t1_.back();
      t1_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        t1Size_ -= cacheIt->second.dataSize;
        
        // Move to B1 (ghost list) - keep metadata but remove data
        b1_.push_front(victim);
        listMap_[victim].list = LIST_B1;
        listMap_[victim].iter = b1_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B1 size
        while (b1_.size() > maxCacheSize_ / (1024 * 16)) {  // ~16KB ghost entries
          ContentKey oldGhost = b1_.back();
          b1_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } 
    // Case 2: Evict from T2
    else if (!t2_.empty()) {
      
      ContentKey victim = t2_.back();
      t2_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        t2Size_ -= cacheIt->second.dataSize;
        
        // Move to B2 (ghost list)
        b2_.push_front(victim);
        listMap_[victim].list = LIST_B2;
        listMap_[victim].iter = b2_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B2 size
        while (b2_.size() > maxCacheSize_ / (1024 * 16)) {
          ContentKey oldGhost = b2_.back();
          b2_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } else {
      // Both lists empty, nothing to evict
      break;
    }
  }
}

void ContentCache::moveToT2(const ContentKey& key)
{
  auto listIt = listMap_.find(key);
  if (listIt == listMap_.end() || listIt->second.list != LIST_T1) {
    return;
  }
  
  size_t size = getEntrySize(key);
  
  // Remove from T1
  t1_.erase(listIt->second.iter);
  t1Size_ -= size;
  
  // Add to T2
  t2_.push_front(key);
  t2Size_ += size;
  
  listMap_[key].list = LIST_T2;
  listMap_[key].iter = t2_.begin();
}

void ContentCache::moveToB1(const ContentKey& key)
{
  removeFromList(key);
  
  b1_.push_front(key);
  listMap_[key].list = LIST_B1;
  listMap_[key].iter = b1_.begin();
}

void ContentCache::moveToB2(const ContentKey& key)
{
  removeFromList(key);
  
  b2_.push_front(key);
  listMap_[key].list = LIST_B2;
  listMap_[key].iter = b2_.begin();
}

void ContentCache::removeFromList(const ContentKey& key)
{
  auto listIt = listMap_.find(key);
  if (listIt == listMap_.end()) {
    return;
  }
  
  size_t size = getEntrySize(key);
  
  switch (listIt->second.list) {
    case LIST_T1:
      t1_.erase(listIt->second.iter);
      t1Size_ -= size;
      break;
    case LIST_T2:
      t2_.erase(listIt->second.iter);
      t2Size_ -= size;
      break;
    case LIST_B1:
      b1_.erase(listIt->second.iter);
      break;
    case LIST_B2:
      b2_.erase(listIt->second.iter);
      break;
    case LIST_NONE:
      break;
  }
  
  listMap_.erase(listIt);
}

size_t ContentCache::getEntrySize(const ContentKey& key) const
{
  auto it = cache_.find(key);
  return (it != cache_.end()) ? it->second.dataSize : 0;
}

uint32_t ContentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}

// ============================================================================
// Pixel Cache ARC now uses shared ArcCache
// ============================================================================

std::vector<uint64_t> ContentCache::getPendingEvictions()
{
  std::vector<uint64_t> result;
  result.swap(pendingEvictions_);
  return result;
}

// ============================================================================
// Cache ID Management Methods (for protocol extension)
// ============================================================================

uint64_t ContentCache::getNextCacheId()
{
  return nextCacheId_.fetch_add(1);
}

ContentCache::CacheEntry* ContentCache::findByCacheId(uint64_t cacheId)
{
  // Look up ContentKey from cache ID
  auto keyIt = cacheIdToKey_.find(cacheId);
  if (keyIt == cacheIdToKey_.end()) {
    return nullptr;
  }
  
  return findContent(keyIt->second);
}

ContentCache::CacheEntry* ContentCache::findByKey(const ContentKey& key, uint64_t* outCacheId)
{
  // Check if key has a cache ID assigned
  auto idIt = keyToCacheId_.find(key);
  if (idIt != keyToCacheId_.end() && outCacheId != nullptr) {
    *outCacheId = idIt->second;
  }
  
  return findContent(key);
}

void ContentCache::insertWithId(uint64_t cacheId,
                               const ContentKey& key,
                               const core::Rect& bounds,
                               const uint8_t* data,
                               size_t dataLen,
                               bool keepData)
{
  // Use the standard insertion
  insertContent(key, bounds, data, dataLen, keepData);
  
  // Associate cache ID with this key
  auto cacheIt = cache_.find(key);
  if (cacheIt != cache_.end()) {
    cacheIt->second.cacheId = cacheId;
    keyToCacheId_[key] = cacheId;
    cacheIdToKey_[cacheId] = key;
    
    vlog.debug("Assigned cache ID %llu to key (%ux%u,hash=%016llx)",
               (unsigned long long)cacheId,
               key.width, key.height, (unsigned long long)key.contentHash);
  }
}

// ============================================================================
// Client-Side Decoded Pixel Storage
// ============================================================================

void ContentCache::storeDecodedPixels(const ContentKey& key,
                                     const uint8_t* pixels,
                                     const PixelFormat& pf,
                                     int width, int height, int stridePixels)
{
  if (pixels == nullptr || width <= 0 || height <= 0) {
    return;
  }

  if (!arcPixelCache_) {
    return;
  }

  // Calculate data size (stridePixels is in pixels)
  const size_t bytesPerPixel = pf.bpp / 8;
  const size_t rowBytes = (size_t)width * bytesPerPixel;
  const size_t srcStrideBytes = (size_t)stridePixels * bytesPerPixel;
  const size_t contiguousSize = (size_t)height * rowBytes;

  // Build entry
  CachedPixels cached;
  cached.key = key;
  cached.format = pf;
  cached.width = width;
  cached.height = height;
  cached.stridePixels = width; // store contiguously
  cached.bytes = contiguousSize;
  cached.lastUsedTime = getCurrentTime();
  cached.pixels.resize(contiguousSize);

  const uint8_t* src = pixels;
  uint8_t* dst = cached.pixels.data();
  for (int y = 0; y < height; y++) {
    memcpy(dst, src, rowBytes);
    src += srcStrideBytes;
    dst += rowBytes;
  }

  // Debug checks - MUST be done BEFORE std::move to avoid use-after-move
  // DEBUG: Check if cached data is all black (potential corruption)
  bool isAllBlack = true;
  for (size_t i = 0; i < contiguousSize && isAllBlack; i++) {
    if (cached.pixels[i] != 0) {
      isAllBlack = false;
    }
  }
  if (isAllBlack) {
    vlog.error("ContentCache: WARNING - Stored all-black rectangle for key (%ux%u,hash=%016llx) "
               "rect=[%dx%d] stride=%d bpp=%d - possible corruption!",
               key.width, key.height, (unsigned long long)key.contentHash,
               width, height, stridePixels, pf.bpp);
  }

  // Insert into ArcCache (will handle promotion/eviction)
  // NOTE: Do not access 'cached' after this move!
  arcPixelCache_->insert(key, std::move(cached));
  
  vlog.debug("Stored decoded pixels for key (%ux%u,hash=%016llx): %dx%d %dbpp (%zu bytes)",
             key.width, key.height, (unsigned long long)key.contentHash,
             width, height, pf.bpp, contiguousSize);
}

const ContentCache::CachedPixels* ContentCache::getDecodedPixels(const ContentKey& key)
{
  if (!arcPixelCache_) {
    return nullptr;
  }
  const CachedPixels* it = arcPixelCache_->get(key);
  if (it == nullptr) {
    stats_.cacheMisses++;
    return nullptr;
  }
  stats_.cacheHits++;

  // DEBUG: Enhanced detection for black/corrupted rectangles
  const CachedPixels& cached = *it;
  
  // Count black pixels and calculate simple checksum
  size_t blackPixelCount = 0;
  size_t totalPixels = cached.width * cached.height;
  uint32_t checksum = 0;
  int bytesPerPixel = cached.format.bpp / 8;
  
  // Sample every pixel (stride-aware)
  for (int y = 0; y < cached.height; y++) {
    const uint8_t* row = cached.pixels.data() + (y * cached.stridePixels * bytesPerPixel);
    for (int x = 0; x < cached.width; x++) {
      const uint8_t* pixel = row + (x * bytesPerPixel);
      
      // Check if pixel is black (all bytes zero)
      bool isBlackPixel = true;
      for (int b = 0; b < bytesPerPixel; b++) {
        checksum = (checksum * 31) + pixel[b];  // Simple checksum
        if (pixel[b] != 0) {
          isBlackPixel = false;
        }
      }
      if (isBlackPixel) {
        blackPixelCount++;
      }
    }
  }
  
  double blackPercent = (totalPixels > 0) ? (100.0 * blackPixelCount / totalPixels) : 0.0;
  
  // Log if rectangle is entirely or mostly black (use vlog for important warnings)
  if (blackPercent == 100.0) {
    vlog.error("ContentCache: WARNING - Retrieved 100%% black rectangle for key (%ux%u,hash=%016llx) "
               "rect=[%dx%d] stride=%d bpp=%d bytes=%zu checksum=0x%08x",
               key.width, key.height, (unsigned long long)key.contentHash,
               cached.width, cached.height,
               cached.stridePixels, cached.format.bpp, cached.pixels.size(), checksum);
  } else if (blackPercent >= 95.0) {
    vlog.error("ContentCache: WARNING - Retrieved %.1f%% black rectangle for key (%ux%u,hash=%016llx) "
               "rect=[%dx%d] stride=%d bpp=%d bytes=%zu checksum=0x%08x",
               blackPercent, key.width, key.height, (unsigned long long)key.contentHash,
               cached.width, cached.height,
               cached.stridePixels, cached.format.bpp, cached.pixels.size(), checksum);
  }
  
  // TEMPORARY: Log verbose pixel samples to debug file (every 100th retrieval)
  static int retrievalCount = 0;
  retrievalCount++;
  if (retrievalCount % 100 == 0) {
    char debugMsg[512];
    snprintf(debugMsg, sizeof(debugMsg),
             "Sample retrieval #%d - key=(%ux%u,hash=%016llx) rect=[%dx%d] checksum=0x%08x black=%.1f%% "
             "first_pixels=[%02x %02x %02x %02x %02x %02x %02x %02x]",
             retrievalCount, key.width, key.height, (unsigned long long)key.contentHash,
             cached.width, cached.height, checksum, blackPercent,
             cached.pixels.size() > 0 ? cached.pixels[0] : 0,
             cached.pixels.size() > 1 ? cached.pixels[1] : 0,
             cached.pixels.size() > 2 ? cached.pixels[2] : 0,
             cached.pixels.size() > 3 ? cached.pixels[3] : 0,
             cached.pixels.size() > 4 ? cached.pixels[4] : 0,
             cached.pixels.size() > 5 ? cached.pixels[5] : 0,
             cached.pixels.size() > 6 ? cached.pixels[6] : 0,
             cached.pixels.size() > 7 ? cached.pixels[7] : 0);
    ContentCacheDebugLogger::getInstance().log(debugMsg);
  }
  
  return it;
}
