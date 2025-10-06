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
#include <algorithm>

using namespace rfb;

static core::LogWriter vlog("ContentCache");

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
                                  size_t stride, size_t bytesPerPixel,
                                  size_t sampleRate)
{
  if (data == nullptr || width == 0 || height == 0)
    return 0;
  
  const uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
  const uint64_t FNV_PRIME = 0x100000001b3ULL;
  
  uint64_t hash = FNV_OFFSET;
  
  // Sample every Nth pixel
  for (size_t y = 0; y < height; y += sampleRate) {
    const uint8_t* row = data + (y * stride * bytesPerPixel);
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
    t2Size_(0)
{
  memset(&stats_, 0, sizeof(stats_));
  vlog.debug("ContentCache created with ARC: maxSize=%zuMB, maxAge=%us",
             maxSizeMB, maxAgeSec);
}

ContentCache::~ContentCache()
{
  vlog.debug("ContentCache destroyed: %zu entries, T1=%zu T2=%zu",
             cache_.size(), t1_.size(), t2_.size());
}

ContentCache::CacheEntry* ContentCache::findContent(uint64_t hash)
{
  auto it = cache_.find(hash);
  if (it == cache_.end()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  stats_.cacheHits++;
  it->second.hitCount++;
  it->second.lastSeenTime = getCurrentTime();
  
  // ARC policy: move from T1 to T2 on second access
  auto listIt = listMap_.find(hash);
  if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
    moveToT2(hash);
  }
  
  return &(it->second);
}

uint64_t ContentCache::insertContent(uint64_t hash,
                                    const core::Rect& bounds,
                                    const uint8_t* data,
                                    size_t dataLen,
                                    bool keepData)
{
  // Check if already in cache
  auto cacheIt = cache_.find(hash);
  if (cacheIt != cache_.end()) {
    // Update existing entry
    cacheIt->second.lastBounds = bounds;
    cacheIt->second.lastSeenTime = getCurrentTime();
    
    // Move to T2 if currently in T1
    auto listIt = listMap_.find(hash);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(hash);
    }
    return cacheIt->second.cacheId;
  }
  
  // Check ghost lists (recently evicted)
  auto listIt = listMap_.find(hash);
  
  if (listIt != listMap_.end() && listIt->second.list == LIST_B1) {
    // Cache hit in B1: adapt by increasing p (favor recency)
    size_t delta = (b2_.size() >= b1_.size()) ? 1 : (b2_.size() / b1_.size());
    p_ = std::min(maxCacheSize_, p_ + delta * dataLen);
    
    // Make room and insert into T2 (it's been accessed twice now)
    replace(hash, dataLen);
    
    // Remove from B1
    b1_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else if (listIt != listMap_.end() && listIt->second.list == LIST_B2) {
    // Cache hit in B2: adapt by decreasing p (favor frequency)
    size_t delta = (b1_.size() >= b2_.size()) ? 1 : (b1_.size() / b2_.size());
    p_ = (delta * dataLen > p_) ? 0 : p_ - delta * dataLen;
    
    // Make room and insert into T2
    replace(hash, dataLen);
    
    // Remove from B2
    b2_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else {
    // New entry: make room and insert into T1
    replace(hash, dataLen);
  }
  
  // Create the entry
  CacheEntry entry(hash, bounds, getCurrentTime());
  entry.dataSize = dataLen;
  entry.cacheId = getNextCacheId();  // Assign new cache ID
  
  if (keepData && data != nullptr && dataLen > 0) {
    entry.data.assign(data, data + dataLen);
  }
  
  // Insert into cache and T2 (or T1 for new items)
  cache_[hash] = entry;
  
  // Register cache ID mappings
  hashToCacheId_[hash] = entry.cacheId;
  cacheIdToHash_[entry.cacheId] = hash;
  
  // Determine which list to add to
  bool wasInGhost = (listIt != listMap_.end() && 
                     (listIt->second.list == LIST_B1 || listIt->second.list == LIST_B2));
  
  if (wasInGhost) {
    // Was in ghost list, add to T2
    t2_.push_front(hash);
    listMap_[hash].list = LIST_T2;
    listMap_[hash].iter = t2_.begin();
    t2Size_ += dataLen;
  } else {
    // New entry, add to T1
    t1_.push_front(hash);
    listMap_[hash].list = LIST_T1;
    listMap_[hash].iter = t1_.begin();
    t1Size_ += dataLen;
  }
  
  stats_.totalEntries++;
  stats_.totalBytes += dataLen;
  
  vlog.debug("Inserted: hash=%016llx cacheId=%llu size=%zu T1=%zu/%zu T2=%zu p=%zu",
             (unsigned long long)hash, (unsigned long long)entry.cacheId, dataLen, 
             t1Size_, t1_.size(), t2_.size(), p_);
  
  return entry.cacheId;
}

void ContentCache::touchEntry(uint64_t hash)
{
  auto it = cache_.find(hash);
  if (it != cache_.end()) {
    it->second.lastSeenTime = getCurrentTime();
    
    // Move from T1 to T2 if accessed again
    auto listIt = listMap_.find(hash);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(hash);
    }
  }
}

void ContentCache::pruneCache()
{
  uint32_t now = getCurrentTime();
  size_t evictedCount = 0;
  
  // Remove aged entries from all lists
  auto pruneList = [&](std::list<uint64_t>& list, size_t& listSize, 
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
  current.totalEntries = cache_.size();
  current.totalBytes = t1Size_ + t2Size_;
  current.t1Size = t1_.size();
  current.t2Size = t2_.size();
  current.b1Size = b1_.size();
  current.b2Size = b2_.size();
  current.targetT1Size = p_;
  return current;
}

void ContentCache::resetStats()
{
  stats_.cacheHits = 0;
  stats_.cacheMisses = 0;
  stats_.evictions = 0;
  stats_.collisions = 0;
}

void ContentCache::setMaxSize(size_t maxSizeMB)
{
  maxCacheSize_ = maxSizeMB * 1024 * 1024;
  vlog.debug("Cache max size set to %zuMB", maxSizeMB);
  
  // Evict if we're now over the limit
  while (t1Size_ + t2Size_ > maxCacheSize_) {
    if (!t1_.empty() || !t2_.empty()) {
      replace(0, 0);  // Force eviction
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

void ContentCache::replace(uint64_t, size_t size)
{
  // Make room for new entry of given size
  while (t1Size_ + t2Size_ + size > maxCacheSize_) {
    
    // Case 1: T1 is larger than target p, evict from T1
    if (!t1_.empty() && 
        (t1Size_ > p_ || (t2_.empty() && t1Size_ == p_))) {
      
      uint64_t victim = t1_.back();
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
          uint64_t oldGhost = b1_.back();
          b1_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } 
    // Case 2: Evict from T2
    else if (!t2_.empty()) {
      
      uint64_t victim = t2_.back();
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
          uint64_t oldGhost = b2_.back();
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

void ContentCache::moveToT2(uint64_t hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end() || listIt->second.list != LIST_T1) {
    return;
  }
  
  size_t size = getEntrySize(hash);
  
  // Remove from T1
  t1_.erase(listIt->second.iter);
  t1Size_ -= size;
  
  // Add to T2
  t2_.push_front(hash);
  t2Size_ += size;
  
  listMap_[hash].list = LIST_T2;
  listMap_[hash].iter = t2_.begin();
}

void ContentCache::moveToB1(uint64_t hash)
{
  removeFromList(hash);
  
  b1_.push_front(hash);
  listMap_[hash].list = LIST_B1;
  listMap_[hash].iter = b1_.begin();
}

void ContentCache::moveToB2(uint64_t hash)
{
  removeFromList(hash);
  
  b2_.push_front(hash);
  listMap_[hash].list = LIST_B2;
  listMap_[hash].iter = b2_.begin();
}

void ContentCache::removeFromList(uint64_t hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end()) {
    return;
  }
  
  size_t size = getEntrySize(hash);
  
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

size_t ContentCache::getEntrySize(uint64_t hash) const
{
  auto it = cache_.find(hash);
  return (it != cache_.end()) ? it->second.dataSize : 0;
}

uint32_t ContentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
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
  // Look up hash from cache ID
  auto hashIt = cacheIdToHash_.find(cacheId);
  if (hashIt == cacheIdToHash_.end()) {
    return nullptr;
  }
  
  return findContent(hashIt->second);
}

ContentCache::CacheEntry* ContentCache::findByHash(uint64_t hash, uint64_t* outCacheId)
{
  // Check if hash has a cache ID assigned
  auto idIt = hashToCacheId_.find(hash);
  if (idIt != hashToCacheId_.end() && outCacheId != nullptr) {
    *outCacheId = idIt->second;
  }
  
  return findContent(hash);
}

void ContentCache::insertWithId(uint64_t cacheId,
                               uint64_t hash,
                               const core::Rect& bounds,
                               const uint8_t* data,
                               size_t dataLen,
                               bool keepData)
{
  // Use the standard insertion
  insertContent(hash, bounds, data, dataLen, keepData);
  
  // Associate cache ID with this hash
  auto cacheIt = cache_.find(hash);
  if (cacheIt != cache_.end()) {
    cacheIt->second.cacheId = cacheId;
    hashToCacheId_[hash] = cacheId;
    cacheIdToHash_[cacheId] = hash;
    
    vlog.debug("Assigned cache ID %llu to hash %016llx",
               (unsigned long long)cacheId,
               (unsigned long long)hash);
  }
}

// ============================================================================
// Client-Side Decoded Pixel Storage
// ============================================================================

void ContentCache::storeDecodedPixels(uint64_t cacheId,
                                     const uint8_t* pixels,
                                     const PixelFormat& pf,
                                     int width, int height, int stride)
{
  if (pixels == nullptr || width <= 0 || height <= 0) {
    return;
  }
  
  CachedPixels& cached = pixelCache_[cacheId];
  cached.cacheId = cacheId;
  cached.format = pf;
  cached.width = width;
  cached.height = height;
  cached.stride = stride;
  cached.lastUsedTime = getCurrentTime();
  
  // Copy pixel data
  size_t dataSize = height * stride * (pf.bpp / 8);
  cached.pixels.resize(dataSize);
  memcpy(cached.pixels.data(), pixels, dataSize);
  
  vlog.debug("Stored decoded pixels for cache ID %llu: %dx%d %dbpp (%zu bytes)",
             (unsigned long long)cacheId, width, height, pf.bpp, dataSize);
  
  // Prune pixel cache if it gets too large
  size_t totalPixelBytes = 0;
  for (const auto& entry : pixelCache_) {
    totalPixelBytes += entry.second.pixels.size();
  }
  
  // If over max size, evict oldest entries
  if (totalPixelBytes > maxCacheSize_) {
    vlog.debug("Pixel cache size %zu exceeds limit %zu, pruning...",
               totalPixelBytes, maxCacheSize_);
    
    // Find oldest entries
    std::vector<uint64_t> idsToRemove;
    for (const auto& entry : pixelCache_) {
      if (getCurrentTime() - entry.second.lastUsedTime > maxAge_) {
        idsToRemove.push_back(entry.first);
      }
    }
    
    // Remove old entries
    for (uint64_t id : idsToRemove) {
      pixelCache_.erase(id);
    }
  }
}

const ContentCache::CachedPixels* ContentCache::getDecodedPixels(uint64_t cacheId)
{
  auto it = pixelCache_.find(cacheId);
  if (it == pixelCache_.end()) {
    return nullptr;
  }
  
  // Update access time
  it->second.lastUsedTime = getCurrentTime();
  
  return &(it->second);
}
