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
// ContentCache Implementation
// ============================================================================

ContentCache::ContentCache(size_t maxSizeMB, uint32_t maxAgeSec)
  : maxCacheSize_(maxSizeMB * 1024 * 1024),
    maxAge_(maxAgeSec),
    currentCacheSize_(0)
{
  memset(&stats_, 0, sizeof(stats_));
  vlog.debug("ContentCache created: maxSize=%zuMB, maxAge=%us",
             maxSizeMB, maxAgeSec);
}

ContentCache::~ContentCache()
{
  vlog.debug("ContentCache destroyed: %zu entries, %zu bytes",
             stats_.totalEntries, stats_.totalBytes);
}

ContentCache::CacheEntry* ContentCache::findContent(uint64_t hash)
{
  auto it = cache_.find(hash);
  if (it == cache_.end()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  // Return first entry for this hash
  // In case of collision, we might have multiple entries
  if (it->second.empty()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  stats_.cacheHits++;
  it->second[0].hitCount++;
  
  return &(it->second[0]);
}

void ContentCache::insertContent(uint64_t hash,
                                const core::Rect& bounds,
                                const uint8_t* data,
                                size_t dataLen,
                                bool keepData)
{
  // Check if we need to make space
  size_t targetSize = maxCacheSize_;
  if (currentCacheSize_ + dataLen > targetSize) {
    // Prune to make room for new entry
    // Target size should allow for the new entry
    while (currentCacheSize_ + dataLen > targetSize && !lruList_.empty()) {
      uint64_t oldHash = lruList_.back();
      evictEntry(oldHash);
      stats_.evictions++;
    }
    
    // If still not enough space, don't cache this entry
    if (currentCacheSize_ + dataLen > targetSize) {
      vlog.debug("Cache full, cannot insert %zu bytes", dataLen);
      return;
    }
  }
  
  // Check if hash already exists (collision or duplicate)
  auto it = cache_.find(hash);
  if (it != cache_.end()) {
    // Update existing entry
    if (!it->second.empty()) {
      it->second[0].lastBounds = bounds;
      it->second[0].lastSeenTime = getCurrentTime();
      updateLRU(hash);
      return;
    }
  }
  
  // Create new entry
  CacheEntry entry(hash, bounds, getCurrentTime());
  entry.dataSize = dataLen;
  
  if (keepData && data != nullptr && dataLen > 0) {
    entry.data.assign(data, data + dataLen);
  }
  
  cache_[hash].push_back(entry);
  currentCacheSize_ += dataLen;
  stats_.totalEntries++;
  stats_.totalBytes += dataLen;
  
  // Add to LRU
  lruList_.push_front(hash);
  lruMap_[hash] = lruList_.begin();
  
  vlog.debug("Inserted content: hash=%016llx, size=%zu, total=%zu/%zu",
             (unsigned long long)hash, dataLen,
             currentCacheSize_, maxCacheSize_);
}

void ContentCache::touchEntry(uint64_t hash)
{
  auto it = cache_.find(hash);
  if (it != cache_.end() && !it->second.empty()) {
    it->second[0].lastSeenTime = getCurrentTime();
    updateLRU(hash);
  }
}

void ContentCache::pruneCache()
{
  uint32_t now = getCurrentTime();
  size_t evictedBytes = 0;
  size_t evictedEntries = 0;
  
  // First pass: Remove expired entries
  for (auto it = cache_.begin(); it != cache_.end(); ) {
    auto& entries = it->second;
    
    for (auto entryIt = entries.begin(); entryIt != entries.end(); ) {
      if (now - entryIt->lastSeenTime > maxAge_) {
        currentCacheSize_ -= entryIt->dataSize;
        evictedBytes += entryIt->dataSize;
        evictedEntries++;
        entryIt = entries.erase(entryIt);
      } else {
        ++entryIt;
      }
    }
    
    // Remove empty hash buckets
    if (entries.empty()) {
      uint64_t hash = it->first;
      it = cache_.erase(it);
      
      // Remove from LRU
      auto lruIt = lruMap_.find(hash);
      if (lruIt != lruMap_.end()) {
        lruList_.erase(lruIt->second);
        lruMap_.erase(lruIt);
      }
    } else {
      ++it;
    }
  }
  
  // Second pass: LRU eviction if still over limit
  while (currentCacheSize_ > maxCacheSize_ && !lruList_.empty()) {
    uint64_t hash = lruList_.back();
    evictEntry(hash);
    evictedEntries++;
  }
  
  stats_.evictions += evictedEntries;
  stats_.totalEntries -= evictedEntries;
  stats_.totalBytes -= evictedBytes;
  
  if (evictedEntries > 0) {
    vlog.debug("Pruned cache: evicted %zu entries, freed %zu bytes",
               evictedEntries, evictedBytes);
  }
}

void ContentCache::clear()
{
  cache_.clear();
  lruList_.clear();
  lruMap_.clear();
  currentCacheSize_ = 0;
  stats_.totalEntries = 0;
  stats_.totalBytes = 0;
  
  vlog.debug("Cache cleared");
}

ContentCache::Stats ContentCache::getStats() const
{
  Stats current = stats_;
  current.totalEntries = cache_.size();
  current.totalBytes = currentCacheSize_;
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
  
  // Prune if we're now over the limit
  if (currentCacheSize_ > maxCacheSize_) {
    pruneCache();
  }
}

void ContentCache::setMaxAge(uint32_t maxAgeSec)
{
  maxAge_ = maxAgeSec;
  vlog.debug("Cache max age set to %us", maxAgeSec);
}

// ============================================================================
// Private Helper Methods
// ============================================================================

void ContentCache::evictEntry(uint64_t hash)
{
  auto it = cache_.find(hash);
  if (it == cache_.end())
    return;
  
  // Remove all entries for this hash
  for (const auto& entry : it->second) {
    currentCacheSize_ -= entry.dataSize;
  }
  
  cache_.erase(it);
  
  // Remove from LRU
  auto lruIt = lruMap_.find(hash);
  if (lruIt != lruMap_.end()) {
    lruList_.erase(lruIt->second);
    lruMap_.erase(lruIt);
  }
  
  vlog.debug("Evicted entry: hash=%016llx", (unsigned long long)hash);
}

void ContentCache::updateLRU(uint64_t hash)
{
  auto it = lruMap_.find(hash);
  if (it != lruMap_.end()) {
    // Move to front
    lruList_.erase(it->second);
    lruList_.push_front(hash);
    lruMap_[hash] = lruList_.begin();
  }
}

uint32_t ContentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
