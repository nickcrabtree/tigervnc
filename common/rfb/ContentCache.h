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
#include <atomic>
#include <vector>
#include <unordered_map>
#include <list>

#include <core/Rect.h>
#include <rfb/PixelFormat.h>

namespace rfb {

  // Content-addressable cache for historical framebuffer chunks
  // Uses ARC (Adaptive Replacement Cache) algorithm for better eviction
  // ARC combines recency (LRU) and frequency (LFU) with self-tuning
  class ContentCache {
  public:
    ContentCache(size_t maxSizeMB = 2048, uint32_t maxAgeSec = 300);
    ~ContentCache();

    struct CacheEntry {
      uint64_t contentHash;           // Hash of pixel data
      uint64_t cacheId;               // Unique cache ID for protocol
      core::Rect lastBounds;          // Where this was last seen
      uint32_t lastSeenTime;          // Timestamp for LRU eviction
      size_t dataSize;                // Size in bytes
      uint32_t hitCount;              // Number of times reused
      
      // Optional: Store actual data for verification on hash collision
      // Trade-off: More memory vs. zero false positives
      std::vector<uint8_t> data;      // Can be empty to save memory
      
      CacheEntry() : contentHash(0), cacheId(0), lastSeenTime(0), dataSize(0), hitCount(0) {}
      
      CacheEntry(uint64_t hash, const core::Rect& bounds, uint32_t time)
        : contentHash(hash), cacheId(0), lastBounds(bounds), lastSeenTime(time),
          dataSize(0), hitCount(0) {}
    };

    // Find if content exists in cache by hash
    // Returns entry if found, nullptr otherwise
    CacheEntry* findContent(uint64_t hash);
    
    // Find by cache ID (for protocol)
    CacheEntry* findByCacheId(uint64_t cacheId);
    
    // Find by hash and return cache ID if exists
    // Returns entry and sets outCacheId if found
    CacheEntry* findByHash(uint64_t hash, uint64_t* outCacheId);
    
    // Insert new content into cache
    // If keepData=true, stores full pixel data for verification
    // Returns assigned cache ID
    uint64_t insertContent(uint64_t hash, 
                          const core::Rect& bounds,
                          const uint8_t* data = nullptr,
                          size_t dataLen = 0,
                          bool keepData = false);
    
    // Insert with explicit cache ID (for client-side storage)
    void insertWithId(uint64_t cacheId,
                     uint64_t hash,
                     const core::Rect& bounds,
                     const uint8_t* data = nullptr,
                     size_t dataLen = 0,
                     bool keepData = false);
    
    // Mark entry as recently accessed (updates ARC lists)
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
      // ARC-specific stats
      size_t t1Size;        // Recently used once
      size_t t2Size;        // Frequently used
      size_t b1Size;        // Ghost entries from T1
      size_t b2Size;        // Ghost entries from T2
      size_t targetT1Size;  // Adaptive target for T1
    };
    
    Stats getStats() const;
    void resetStats();
    void logArcStats() const;  // Log concise ARC statistics
    
    // Configuration
    void setMaxSize(size_t maxSizeMB);
    void setMaxAge(uint32_t maxAgeSec);
    
    // Get next available cache ID (for server)
    uint64_t getNextCacheId();
    
    // Client-side: Store decoded pixels by cache ID
    struct CachedPixels {
      uint64_t cacheId;
      std::vector<uint8_t> pixels;
      PixelFormat format;
      int width;
      int height;
      int stride;
      uint32_t lastUsedTime;
      
      CachedPixels() : cacheId(0), width(0), height(0), stride(0), lastUsedTime(0) {}
    };
    
    void storeDecodedPixels(uint64_t cacheId, const uint8_t* pixels,
                           const PixelFormat& pf, int width, int height, int stride);
    const CachedPixels* getDecodedPixels(uint64_t cacheId);
    
  private:
    // Main cache storage: hash -> entry data
    std::unordered_map<uint64_t, CacheEntry> cache_;
    
    // Cache ID management (for protocol)
    std::atomic<uint64_t> nextCacheId_;
    std::unordered_map<uint64_t, uint64_t> hashToCacheId_;  // hash -> cache ID
    std::unordered_map<uint64_t, uint64_t> cacheIdToHash_;  // cache ID -> hash
    
    // Client-side: Decoded pixel storage
    std::unordered_map<uint64_t, CachedPixels> pixelCache_;
    
    // ARC list membership tracking
    enum ListType {
      LIST_NONE,  // Not in any list
      LIST_T1,    // Recently used once (recency)
      LIST_T2,    // Frequently used (frequency)
      LIST_B1,    // Ghost: evicted from T1
      LIST_B2     // Ghost: evicted from T2
    };
    
    struct ListInfo {
      ListType list;
      std::list<uint64_t>::iterator iter;
      
      ListInfo() : list(LIST_NONE) {}
    };
    
    // ARC lists (most recent at front)
    std::list<uint64_t> t1_;  // Recently used once
    std::list<uint64_t> t2_;  // Frequently used
    std::list<uint64_t> b1_;  // Ghost entries from T1
    std::list<uint64_t> b2_;  // Ghost entries from T2
    
    // Track which list each hash is in
    std::unordered_map<uint64_t, ListInfo> listMap_;
    
    // ARC adaptive parameter: target size for T1 in bytes
    size_t p_;
    
    // Configuration
    size_t maxCacheSize_;      // In bytes (total for T1+T2)
    uint32_t maxAge_;          // In seconds
    
    // Current state
    size_t t1Size_;            // Bytes in T1
    size_t t2Size_;            // Bytes in T2
    
    // Statistics
    mutable Stats stats_;
    
    // ARC helper methods
    void replace(uint64_t hash, size_t size);
    void moveToT2(uint64_t hash);
    void moveToB1(uint64_t hash);
    void moveToB2(uint64_t hash);
    void removeFromList(uint64_t hash);
    size_t getEntrySize(uint64_t hash) const;
    
    // Common helpers
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
