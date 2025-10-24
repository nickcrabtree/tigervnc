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

#include <time.h>
#include <cstring>
#include <algorithm>

using namespace rfb;

static core::LogWriter vlog("PersistentCache");

// ============================================================================
// GlobalClientPersistentCache Implementation - ARC Algorithm
// ============================================================================

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxSizeMB)
  : p_(0),
    maxCacheSize_(maxSizeMB * 1024 * 1024),
    t1Size_(0),
    t2Size_(0),
    cacheFilePath_("")
{
  memset(&stats_, 0, sizeof(stats_));
  vlog.debug("PersistentCache created with ARC: maxSize=%zuMB", maxSizeMB);
}

GlobalClientPersistentCache::~GlobalClientPersistentCache()
{
  vlog.debug("PersistentCache destroyed: %zu entries, T1=%zu T2=%zu",
             cache_.size(), t1_.size(), t2_.size());
}

bool GlobalClientPersistentCache::has(const std::vector<uint8_t>& hash) const
{
  return cache_.find(hash) != cache_.end();
}

const GlobalClientPersistentCache::CachedPixels* 
GlobalClientPersistentCache::get(const std::vector<uint8_t>& hash)
{
  auto it = cache_.find(hash);
  if (it == cache_.end()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  stats_.cacheHits++;
  it->second.lastAccessTime = getCurrentTime();
  
  // ARC policy: move from T1 to T2 on second access
  auto listIt = listMap_.find(hash);
  if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
    moveToT2(hash);
  }
  
  return &(it->second);
}

void GlobalClientPersistentCache::insert(const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels)
{
  if (pixels == nullptr || width == 0 || height == 0)
    return;
    
  size_t pixelDataSize = height * stridePixels * (pf.bpp / 8);
  
  // Check if already in cache
  auto cacheIt = cache_.find(hash);
  if (cacheIt != cache_.end()) {
    // Update existing entry
    cacheIt->second.lastAccessTime = getCurrentTime();
    
    // Move to T2 if currently in T1
    auto listIt = listMap_.find(hash);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(hash);
    }
    return;
  }
  
  // Check ghost lists (recently evicted)
  auto listIt = listMap_.find(hash);
  
  if (listIt != listMap_.end() && listIt->second.list == LIST_B1) {
    // Cache hit in B1: adapt by increasing p (favor recency)
    size_t delta = (b2_.size() >= b1_.size()) ? 1 : (b2_.size() / b1_.size());
    p_ = std::min(maxCacheSize_, p_ + delta * pixelDataSize);
    
    // Make room and insert into T2
    replace(hash, pixelDataSize);
    
    // Remove from B1
    b1_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else if (listIt != listMap_.end() && listIt->second.list == LIST_B2) {
    // Cache hit in B2: adapt by decreasing p (favor frequency)
    size_t delta = (b1_.size() >= b2_.size()) ? 1 : (b1_.size() / b2_.size());
    p_ = (delta * pixelDataSize > p_) ? 0 : p_ - delta * pixelDataSize;
    
    // Make room and insert into T2
    replace(hash, pixelDataSize);
    
    // Remove from B2
    b2_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else {
    // New entry: make room and insert into T1
    replace(hash, pixelDataSize);
  }
  
  // Create the entry
  CachedPixels entry;
  entry.pixels.assign(pixels, pixels + pixelDataSize);
  entry.format = pf;
  entry.width = width;
  entry.height = height;
  entry.stridePixels = stridePixels;
  entry.lastAccessTime = getCurrentTime();
  
  // Insert into cache
  cache_[hash] = entry;
  
  // Determine which list to add to
  bool wasInGhost = (listIt != listMap_.end() && 
                     (listIt->second.list == LIST_B1 || listIt->second.list == LIST_B2));
  
  if (wasInGhost) {
    // Was in ghost list, add to T2
    t2_.push_front(hash);
    listMap_[hash].list = LIST_T2;
    listMap_[hash].iter = t2_.begin();
    t2Size_ += pixelDataSize;
  } else {
    // New entry, add to T1
    t1_.push_front(hash);
    listMap_[hash].list = LIST_T1;
    listMap_[hash].iter = t1_.begin();
    t1Size_ += pixelDataSize;
  }
  
  stats_.totalEntries++;
  stats_.totalBytes += pixelDataSize;
  
  vlog.debug("Inserted: hashLen=%zu size=%zu T1=%zu/%zu T2=%zu p=%zu",
             hash.size(), pixelDataSize, t1Size_, t1_.size(), t2_.size(), p_);
}

std::vector<std::vector<uint8_t>> 
GlobalClientPersistentCache::getAllHashes() const
{
  std::vector<std::vector<uint8_t>> hashes;
  hashes.reserve(cache_.size());
  
  for (const auto& entry : cache_) {
    hashes.push_back(entry.first);
  }
  
  return hashes;
}

void GlobalClientPersistentCache::clear()
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
  
  vlog.debug("PersistentCache cleared");
}

GlobalClientPersistentCache::Stats 
GlobalClientPersistentCache::getStats() const
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

void GlobalClientPersistentCache::resetStats()
{
  stats_.cacheHits = 0;
  stats_.cacheMisses = 0;
  stats_.evictions = 0;
}

void GlobalClientPersistentCache::setMaxSize(size_t maxSizeMB)
{
  maxCacheSize_ = maxSizeMB * 1024 * 1024;
  vlog.debug("PersistentCache max size set to %zuMB", maxSizeMB);
  
  // Evict if we're now over the limit
  while (t1Size_ + t2Size_ > maxCacheSize_) {
    if (!t1_.empty() || !t2_.empty()) {
      replace(std::vector<uint8_t>(), 0);  // Force eviction
    } else {
      break;
    }
  }
}

bool GlobalClientPersistentCache::loadFromDisk()
{
  // TODO: Phase 7 - Implement disk loading
  vlog.info("PersistentCache: loadFromDisk() not yet implemented (Phase 7)");
  return false;
}

bool GlobalClientPersistentCache::saveToDisk()
{
  // TODO: Phase 7 - Implement disk saving
  vlog.info("PersistentCache: saveToDisk() not yet implemented (Phase 7)");
  return false;
}

// ============================================================================
// ARC Algorithm Implementation
// ============================================================================

void GlobalClientPersistentCache::replace(const std::vector<uint8_t>&, size_t size)
{
  // Make room for new entry of given size
  while (t1Size_ + t2Size_ + size > maxCacheSize_) {
    
    // Case 1: T1 is larger than target p, evict from T1
    if (!t1_.empty() && 
        (t1Size_ > p_ || (t2_.empty() && t1Size_ == p_))) {
      
      std::vector<uint8_t> victim = t1_.back();
      t1_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        size_t victimSize = cacheIt->second.byteSize();
        t1Size_ -= victimSize;
        
        // Move to B1 (ghost list) - keep metadata but remove data
        b1_.push_front(victim);
        listMap_[victim].list = LIST_B1;
        listMap_[victim].iter = b1_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B1 size
        while (b1_.size() > maxCacheSize_ / (1024 * 16)) {  // ~16KB ghost entries
          std::vector<uint8_t> oldGhost = b1_.back();
          b1_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } 
    // Case 2: Evict from T2
    else if (!t2_.empty()) {
      
      std::vector<uint8_t> victim = t2_.back();
      t2_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        size_t victimSize = cacheIt->second.byteSize();
        t2Size_ -= victimSize;
        
        // Move to B2 (ghost list)
        b2_.push_front(victim);
        listMap_[victim].list = LIST_B2;
        listMap_[victim].iter = b2_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B2 size
        while (b2_.size() > maxCacheSize_ / (1024 * 16)) {
          std::vector<uint8_t> oldGhost = b2_.back();
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

void GlobalClientPersistentCache::moveToT2(const std::vector<uint8_t>& hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end() || listIt->second.list != LIST_T1)
    return;
    
  // Remove from T1
  t1_.erase(listIt->second.iter);
  size_t entrySize = getEntrySize(hash);
  t1Size_ -= entrySize;
  
  // Add to T2
  t2_.push_front(hash);
  listMap_[hash].list = LIST_T2;
  listMap_[hash].iter = t2_.begin();
  t2Size_ += entrySize;
}

void GlobalClientPersistentCache::moveToB1(const std::vector<uint8_t>& hash)
{
  removeFromList(hash);
  b1_.push_front(hash);
  listMap_[hash].list = LIST_B1;
  listMap_[hash].iter = b1_.begin();
}

void GlobalClientPersistentCache::moveToB2(const std::vector<uint8_t>& hash)
{
  removeFromList(hash);
  b2_.push_front(hash);
  listMap_[hash].list = LIST_B2;
  listMap_[hash].iter = b2_.begin();
}

void GlobalClientPersistentCache::removeFromList(const std::vector<uint8_t>& hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end())
    return;
    
  switch (listIt->second.list) {
    case LIST_T1:
      t1_.erase(listIt->second.iter);
      t1Size_ -= getEntrySize(hash);
      break;
    case LIST_T2:
      t2_.erase(listIt->second.iter);
      t2Size_ -= getEntrySize(hash);
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

size_t GlobalClientPersistentCache::getEntrySize(const std::vector<uint8_t>& hash) const
{
  auto it = cache_.find(hash);
  if (it != cache_.end()) {
    return it->second.byteSize();
  }
  return 0;
}

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
