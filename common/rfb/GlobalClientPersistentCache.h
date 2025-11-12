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

#ifndef __RFB_GLOBAL_CLIENT_PERSISTENT_CACHE_H__
#define __RFB_GLOBAL_CLIENT_PERSISTENT_CACHE_H__

#include <stdint.h>
#include <vector>
#include <unordered_map>
#include <list>
#include <memory>      // for std::unique_ptr
#include <string>
#include <functional>
#include <fstream>
#include <mutex>
#include <chrono>

#include <rfb/PixelFormat.h>
#include <rfb/cache/ArcCache.h>

namespace rfb {

  // Debug logger for PersistentCache - logs to tmpfile with timestamps
  class PersistentCacheDebugLogger {
  public:
    static PersistentCacheDebugLogger& getInstance() {
      static PersistentCacheDebugLogger instance;
      return instance;
    }
    
    void log(const std::string& message);
    
  private:
    PersistentCacheDebugLogger();
    ~PersistentCacheDebugLogger();
    PersistentCacheDebugLogger(const PersistentCacheDebugLogger&) = delete;
    PersistentCacheDebugLogger& operator=(const PersistentCacheDebugLogger&) = delete;
    
    std::ofstream logFile_;
    std::string logFilename_;
    std::mutex logMutex_;
  };

  // Hash function for std::vector<uint8_t> to use as key in unordered_map
  struct HashVectorHasher {
    size_t operator()(const std::vector<uint8_t>& v) const {
      // Use FNV-1a hash
      size_t hash = 14695981039346656037ULL;
      for (uint8_t byte : v) {
        hash ^= byte;
        hash *= 1099511628211ULL;
      }
      return hash;
    }
  };

  // Global client-side persistent cache for PersistentCache protocol
  // Uses content hashes as stable keys for cross-session/cross-server caching
  // Implements ARC (Adaptive Replacement Cache) eviction algorithm
  class GlobalClientPersistentCache {
  public:
    struct CachedPixels {
      std::vector<uint8_t> pixels;     // Decoded pixel data
      PixelFormat format;               // Pixel format
      uint16_t width;                   // Rectangle width
      uint16_t height;                  // Rectangle height
      uint16_t stridePixels;            // Stride in pixels (NOT bytes)
      uint32_t lastAccessTime;          // For LRU/ARC eviction
      
      CachedPixels() : width(0), height(0), stridePixels(0), lastAccessTime(0) {}
      
      size_t byteSize() const {
        return pixels.size();
      }
    };
    
    GlobalClientPersistentCache(size_t maxSizeMB = 2048);
    ~GlobalClientPersistentCache();
    
    // Lifecycle (disk persistence will be added in Phase 7)
    bool loadFromDisk();
    bool saveToDisk();
    
    // Protocol operations
    bool has(const std::vector<uint8_t>& hash) const;
    const CachedPixels* get(const std::vector<uint8_t>& hash);
    void insert(const std::vector<uint8_t>& hash, 
               const uint8_t* pixels,
               const PixelFormat& pf,
               uint16_t width, uint16_t height,
               uint16_t stridePixels);
    
    // Optional: Get all known hashes for HashList message
    std::vector<std::vector<uint8_t>> getAllHashes() const;
    
    // Statistics
    struct Stats {
      size_t totalEntries;
      size_t totalBytes;
      uint64_t cacheHits;
      uint64_t cacheMisses;
      uint64_t evictions;
      // ARC-specific stats
      size_t t1Size;        // Recently used once
      size_t t2Size;        // Frequently used
      size_t b1Size;        // Ghost entries from T1
      size_t b2Size;        // Ghost entries from T2
      size_t targetT1Size;  // Adaptive target for T1 (p parameter)
    };
    Stats getStats() const;
    void resetStats();
    
    // Pending evictions (to notify server)
    bool hasPendingEvictions() const { return !pendingEvictions_.empty(); }
    std::vector<std::vector<uint8_t>> getPendingEvictions() {
      auto out = pendingEvictions_;
      pendingEvictions_.clear();
      return out;
    }
    
    // Configuration
    void setMaxSize(size_t maxSizeMB);
    void clear();
    
  private:
    // Main cache storage: hash -> cached pixels
    std::unordered_map<std::vector<uint8_t>, CachedPixels, HashVectorHasher> cache_;
    
    // Queue of hashes evicted from ARC; drained by DecodeManager to notify server
    std::vector<std::vector<uint8_t>> pendingEvictions_;
    
    // Shared ARC cache (byte-capacity)
    std::unique_ptr<rfb::cache::ArcCache<std::vector<uint8_t>, CachedPixels, HashVectorHasher>> arcCache_;

    // Configuration
    size_t maxCacheSize_;      // In bytes
    
    // Statistics
    mutable Stats stats_;
    
    // Disk persistence
    std::string cacheFilePath_;

    // Helper to get current timestamp
    uint32_t getCurrentTime() const;
    
    // Prevent copying
    GlobalClientPersistentCache(const GlobalClientPersistentCache&) = delete;
    GlobalClientPersistentCache& operator=(const GlobalClientPersistentCache&) = delete;
  };

}

#endif // __RFB_GLOBAL_CLIENT_PERSISTENT_CACHE_H__
