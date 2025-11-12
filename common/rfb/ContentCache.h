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
#include <functional>  // for std::hash
#include <vector>
#include <unordered_map>
#include <list>
#include <fstream>     //DebugContentCache_2025-10-14
#include <string>      //DebugContentCache_2025-10-14
#include <chrono>      //DebugContentCache_2025-10-14
#include <mutex>       //DebugContentCache_2025-10-14
#include <iostream>    //DebugContentCache_2025-10-14

#include <core/Rect.h>
#include <rfb/PixelFormat.h>
#include <rfb/cache/ArcCache.h>

namespace rfb {

  //DebugContentCache_2025-10-14 - Start debug logging class
  class ContentCacheDebugLogger {
  public:
    static ContentCacheDebugLogger& getInstance() {
      static ContentCacheDebugLogger instance;
      return instance;
    }
    
    void log(const std::string& message);
    std::string getLogFilename() const { return logFilename_; }
    
  private:
    ContentCacheDebugLogger();
    ~ContentCacheDebugLogger();
    std::string logFilename_;
    std::ofstream logFile_;
    std::mutex logMutex_;
    
    ContentCacheDebugLogger(const ContentCacheDebugLogger&) = delete;
    ContentCacheDebugLogger& operator=(const ContentCacheDebugLogger&) = delete;
  };
  //DebugContentCache_2025-10-14 - End debug logging class

  // Composite key for content cache entries
  // Combines rectangle dimensions with content hash to guarantee uniqueness
  // across different-sized rectangles with similar content.
  // Total size: 12 bytes (uint16_t + uint16_t + uint64_t)
  struct ContentKey {
    uint16_t width;       // Rectangle width (max 65535)
    uint16_t height;      // Rectangle height (max 65535)
    uint64_t contentHash; // 64-bit content hash
    
    ContentKey() : width(0), height(0), contentHash(0) {}
    
    ContentKey(uint16_t w, uint16_t h, uint64_t hash)
      : width(w), height(h), contentHash(hash) {}
    
    bool operator==(const ContentKey& other) const {
      return width == other.width && 
             height == other.height && 
             contentHash == other.contentHash;
    }
  };
  
  // Hash function for ContentKey to use in unordered_map
  // Combines dimensions and content hash using simple bit operations
  // (no magic primes - just bit packing based on field widths)
  struct ContentKeyHash {
    size_t operator()(const ContentKey& k) const {
      // Pack dimensions into high bits, combine with content hash
      uint64_t dims = ((uint64_t)(k.width & 0xFFFF) << 48) | 
                      ((uint64_t)(k.height & 0xFFFF) << 32);
      // Mix with content hash for good distribution
      uint64_t h = k.contentHash ^ dims ^ (k.contentHash >> 32);
      return std::hash<uint64_t>{}(h);
    }
  };

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

    // Find if content exists in cache by composite key (dimensions + hash)
    // Returns entry if found, nullptr otherwise
    CacheEntry* findContent(const ContentKey& key);
    
    // Find by cache ID (for protocol)
    CacheEntry* findByCacheId(uint64_t cacheId);
    
    // Find by composite key and return cache ID if exists
    // Returns entry and sets outCacheId if found
    CacheEntry* findByKey(const ContentKey& key, uint64_t* outCacheId);
    
    // Insert new content into cache
    // If keepData=true, stores full pixel data for verification
    // Returns assigned cache ID
    uint64_t insertContent(const ContentKey& key, 
                          const core::Rect& bounds,
                          const uint8_t* data = nullptr,
                          size_t dataLen = 0,
                          bool keepData = false);
    
    // Insert with explicit cache ID (for client-side storage)
    void insertWithId(uint64_t cacheId,
                     const ContentKey& key,
                     const core::Rect& bounds,
                     const uint8_t* data = nullptr,
                     size_t dataLen = 0,
                     bool keepData = false);
    
    // Mark entry as recently accessed (updates ARC lists)
    void touchEntry(const ContentKey& key);
    
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
    
    // Get total memory usage in bytes
    // Returns sum of hash cache (server) and pixel cache (client)
    size_t getTotalBytes() const;
    
    // Configuration
    void setMaxSize(size_t maxSizeMB);
    void setMaxAge(uint32_t maxAgeSec);
    
    // Get next available cache ID (for server)
    uint64_t getNextCacheId();
    
    // Client-side: Store decoded pixels by ContentKey
    struct CachedPixels {
      ContentKey key;           // Composite key (width, height, hash)
      std::vector<uint8_t> pixels;
      PixelFormat format;
      int width;
      int height;
      int stridePixels;  // Stride in pixels, NOT bytes
      size_t bytes;      // Total byte size (for ARC tracking)
      uint32_t lastUsedTime;
      
      CachedPixels() : width(0), height(0), stridePixels(0), bytes(0), lastUsedTime(0) {}
    };
    
    void storeDecodedPixels(const ContentKey& key, const uint8_t* pixels,
                           const PixelFormat& pf, int width, int height, int stridePixels);
    const CachedPixels* getDecodedPixels(const ContentKey& key);
    
    // Eviction notification management
    // Returns vector of evicted cache IDs that need to be sent to server
    // Clears the pending list after returning
    std::vector<uint64_t> getPendingEvictions();
    bool hasPendingEvictions() const { return !pendingEvictions_.empty(); }
    
  private:
    // Main cache storage: ContentKey -> entry data
    // Key includes dimensions to prevent reuse across different-sized rectangles
    std::unordered_map<ContentKey, CacheEntry, ContentKeyHash> cache_;
    
    // Cache ID management (for protocol)
    std::atomic<uint64_t> nextCacheId_;
    std::unordered_map<ContentKey, uint64_t, ContentKeyHash> keyToCacheId_;  // key -> cache ID
    std::unordered_map<uint64_t, ContentKey> cacheIdToKey_;  // cache ID -> key
    
    // Client-side: Decoded pixel storage (shared ArcCache)
    std::unique_ptr<rfb::cache::ArcCache<ContentKey, CachedPixels, ContentKeyHash>> arcPixelCache_;

    // ARC lists (most recent at front) - server-side hash cache
    std::list<ContentKey> t1_;  // Recently used once
    std::list<ContentKey> t2_;  // Frequently used
    std::list<ContentKey> b1_;  // Ghost entries from T1
    std::list<ContentKey> b2_;  // Ghost entries from T2
    
    // Track which list each key is in (server-side hash cache)
    enum ListType { LIST_T1, LIST_T2, LIST_B1, LIST_B2, LIST_NONE };
    struct ListInfo {
      ListType list;
      std::list<ContentKey>::iterator iter;
      ListInfo() : list(LIST_NONE) {}
    };
    std::unordered_map<ContentKey, ListInfo, ContentKeyHash> listMap_;
    
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

    // Pixel cache capacity (ArcCache), kept configurable
    size_t maxPixelCacheSize_;  // Maximum pixel cache size in bytes
    
    // Eviction notification tracking
    std::vector<uint64_t> pendingEvictions_;  // Cache IDs evicted, pending notification to server
    
    // ARC helper methods (server-side hash cache)
    void replace(const ContentKey& key, size_t size);
    void moveToT2(const ContentKey& key);
    void moveToB1(const ContentKey& key);
    void moveToB2(const ContentKey& key);
    void removeFromList(const ContentKey& key);
    size_t getEntrySize(const ContentKey& key) const;
    
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
                              size_t stridePixels, size_t bytesPerPixel,
                              size_t sampleRate = 4);

}

#endif // __RFB_CONTENT_CACHE_H__
