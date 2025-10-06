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

#ifndef __RFB_CONTENT_CACHE_H__
#define __RFB_CONTENT_CACHE_H__

#include <stdint.h>
#include <vector>
#include <unordered_map>
#include <list>

#include <core/Rect.h>

namespace rfb {

  // Content-addressable cache for historical framebuffer chunks
  // Allows detection of repeated content even after significant time gaps
  class ContentCache {
  public:
    ContentCache(size_t maxSizeMB = 100, uint32_t maxAgeSec = 300);
    ~ContentCache();

    struct CacheEntry {
      uint64_t contentHash;           // Hash of pixel data
      core::Rect lastBounds;          // Where this was last seen
      uint32_t lastSeenTime;          // Timestamp for LRU eviction
      size_t dataSize;                // Size in bytes
      uint32_t hitCount;              // Number of times reused
      
      // Optional: Store actual data for verification on hash collision
      // Trade-off: More memory vs. zero false positives
      std::vector<uint8_t> data;      // Can be empty to save memory
      
      CacheEntry(uint64_t hash, const core::Rect& bounds, uint32_t time)
        : contentHash(hash), lastBounds(bounds), lastSeenTime(time),
          dataSize(0), hitCount(0) {}
    };

    // Find if content exists in cache
    // Returns entry if found, nullptr otherwise
    CacheEntry* findContent(uint64_t hash);
    
    // Insert new content into cache
    // If keepData=true, stores full pixel data for verification
    void insertContent(uint64_t hash, 
                      const core::Rect& bounds,
                      const uint8_t* data = nullptr,
                      size_t dataLen = 0,
                      bool keepData = false);
    
    // Mark entry as recently accessed (updates LRU)
    void touchEntry(uint64_t hash);
    
    // Remove old entries based on age and memory limits
    void pruneCache();
    
    // Flush entire cache (e.g., on resolution change)
    void clear();
    
    // Statistics
    struct Stats {
      size_t totalEntries;
      size_t totalBytes;
      uint64_t cacheHits;
      uint64_t cacheMisses;
      uint64_t evictions;
      uint64_t collisions;
    };
    
    Stats getStats() const;
    void resetStats();
    
    // Configuration
    void setMaxSize(size_t maxSizeMB);
    void setMaxAge(uint32_t maxAgeSec);
    
  private:
    // Main cache storage: hash -> list of entries
    // Multiple entries per hash possible (collision handling)
    std::unordered_map<uint64_t, std::vector<CacheEntry>> cache_;
    
    // LRU tracking: most recently used at front
    std::list<uint64_t> lruList_;
    std::unordered_map<uint64_t, std::list<uint64_t>::iterator> lruMap_;
    
    // Configuration
    size_t maxCacheSize_;      // In bytes
    uint32_t maxAge_;          // In seconds
    
    // Current state
    size_t currentCacheSize_;  // In bytes
    
    // Statistics
    mutable Stats stats_;
    
    // Helper methods
    void evictEntry(uint64_t hash);
    void updateLRU(uint64_t hash);
    uint32_t getCurrentTime() const;
    
    // Prevent copying
    ContentCache(const ContentCache&) = delete;
    ContentCache& operator=(const ContentCache&) = delete;
  };

  // Fast hash function for pixel data
  // Uses xxHash for speed and good distribution
  uint64_t computeContentHash(const uint8_t* data, size_t len);
  
  // Optional: Sampling hash for very large rectangles
  // Hashes only every Nth pixel for speed
  uint64_t computeSampledHash(const uint8_t* data, 
                              size_t width, size_t height,
                              size_t stride, size_t bytesPerPixel,
                              size_t sampleRate = 4);

}

#endif // __RFB_CONTENT_CACHE_H__
