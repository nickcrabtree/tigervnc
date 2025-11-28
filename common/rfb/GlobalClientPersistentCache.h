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
#include <unordered_set>
#include <list>
#include <memory>      // for std::unique_ptr
#include <string>
#include <functional>
#include <fstream>
#include <mutex>
#include <chrono>

#include <rfb/PixelFormat.h>
#include <rfb/cache/ArcCache.h>
#include <rfb/ContentCache.h>  // For ContentKey / ContentKeyHash shared with ContentCache

// Forward declaration for file I/O
#include <cstdio>

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
  //
  // File Format v3 (sharded):
  //   Directory structure:
  //     index.dat      - Master index with entry metadata
  //     shard_NNNN.dat - Payload shard files (~64MB each)
  //   Disk cache can be larger than memory cache (default 2x) to keep
  //   evicted entries available for re-hydration.
  class GlobalClientPersistentCache {
  public:
    // Hydration state for lazy-load
    enum class HydrationState {
      Uninitialized,      // No disk load attempted
      IndexLoaded,        // Index loaded, payloads not yet read
      PartiallyHydrated,  // Some payloads loaded
      FullyHydrated       // All payloads in memory
    };

    struct CachedPixels {
      std::vector<uint8_t> pixels;     // Decoded pixel data (may be empty if not hydrated)
      PixelFormat format;               // Pixel format
      uint16_t width;                   // Rectangle width
      uint16_t height;                  // Rectangle height
      uint16_t stridePixels;            // Stride in pixels (NOT bytes)
      uint32_t lastAccessTime;          // For LRU/ARC eviction
      
      CachedPixels() : width(0), height(0), stridePixels(0), lastAccessTime(0) {}
      
      size_t byteSize() const {
        return pixels.size();
      }
      
      bool isHydrated() const {
        return !pixels.empty();
      }
    };
    
    GlobalClientPersistentCache(size_t maxMemorySizeMB = 2048,
                                 size_t maxDiskSizeMB = 0,  // 0 = auto (2x memory)
                                 size_t shardSizeMB = 64,
                                 const std::string& cacheDirOverride = std::string());
    ~GlobalClientPersistentCache();
    
    // Lifecycle - lazy-load from disk
    bool loadFromDisk();        // Legacy v1/v2 detection and cleanup
    bool loadIndexFromDisk();   // v3 fast index-only load
    bool saveToDisk();          // Save index (shards already written incrementally)
    
    // Incremental saves - write dirty entries to current shard
    size_t flushDirtyEntries(); // Append dirty entries to shard, returns count flushed
    size_t getDirtyEntryCount() const { return dirtyEntries_.size(); }
    
    // Garbage collection - reclaim space from cold/orphaned entries
    size_t garbageCollect();    // Compact fragmented shards, returns bytes reclaimed
    size_t getColdEntryCount() const { return coldEntries_.size(); }
    size_t getDiskUsage() const;
    
    // Lazy hydration - load pixel data on-demand
    bool hydrateEntry(const std::vector<uint8_t>& hash);  // Load single entry's pixels
    size_t hydrateNextBatch(size_t maxEntries);           // Proactive background hydration
    HydrationState getHydrationState() const { return hydrationState_; }
    size_t getHydrationQueueSize() const { return hydrationQueue_.size(); }
    
    // Protocol operations
    bool has(const std::vector<uint8_t>& hash) const;
    const CachedPixels* get(const std::vector<uint8_t>& hash);
    
    // Shared keying with ContentCache via ContentKey. These helpers allow
    // callers (e.g. DecodeManager) to look up entries directly by
    // (width,height,contentHash64) without needing the full hash vector.
    const CachedPixels* getByKey(const ContentKey& key);
    
    void insert(const std::vector<uint8_t>& hash, 
               const uint8_t* pixels,
               const PixelFormat& pf,
               uint16_t width, uint16_t height,
               uint16_t stridePixels);
    
    // Optional: Get all known hashes/IDs for HashList message
    std::vector<std::vector<uint8_t>> getAllHashes() const;
    std::vector<uint64_t> getAllContentIds() const;
    
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
    
    // Pending evictions (to notify server). Exposed as 64-bit content IDs
    // (ContentKey::contentHash) even though we internally track full
    // protocol hashes for disk/index bookkeeping.
    bool hasPendingEvictions() const { return !pendingEvictions_.empty(); }
    std::vector<uint64_t> getPendingEvictions() {
      std::vector<uint64_t> ids;
      ids.reserve(pendingEvictions_.size());
      for (const auto& hash : pendingEvictions_) {
        uint64_t id = 0;
        auto itKey = hashToKey_.find(hash);
        if (itKey != hashToKey_.end()) {
          id = itKey->second.contentHash;
        } else if (!hash.empty()) {
          // Fallback: derive ID from first up-to-8 bytes of hash
          size_t n = std::min(hash.size(), sizeof(uint64_t));
          memcpy(&id, hash.data(), n);
        }
        if (id != 0)
          ids.push_back(id);
      }
      pendingEvictions_.clear();
      return ids;
    }
    
    // Configuration
    void setMaxSize(size_t maxSizeMB);
    void clear();
    
  private:
    // Main in-memory cache storage uses the same composite keying as
    // ContentCache (width, height, 64-bit content hash).
    std::unordered_map<ContentKey, CachedPixels, ContentKeyHash> cache_;
    
    // Queue of hashes evicted from ARC; drained by DecodeManager to
    // notify server. Stored as protocol-level full hashes even though
    // the in-memory ARC key is ContentKey.
    std::vector<std::vector<uint8_t>> pendingEvictions_;
    
    // Shared ARC cache (byte-capacity), keyed by ContentKey just like
    // ContentCache. PersistentCache differs only in that it also
    // persists entries to disk.
    std::unique_ptr<rfb::cache::ArcCache<ContentKey, CachedPixels, ContentKeyHash>> arcCache_;

    // Configuration
    size_t maxMemorySize_;     // Max in-memory cache (bytes)
    size_t maxDiskSize_;       // Max on-disk cache (bytes)
    size_t shardSize_;         // Target shard file size (bytes)
    
    // Statistics
    mutable Stats stats_;
    
    // Disk persistence - directory-based sharded storage
    std::string cacheDir_;     // Cache directory path
    
    // Lazy hydration state
    HydrationState hydrationState_;
    
    // Index entry for lazy loading (v3 format with shard info)
    struct IndexEntry {
      ContentKey key;              // Shared in-memory key (width/height, hash64)
      uint16_t shardId;         // Which shard file contains the payload
      uint32_t payloadOffset;   // Offset within the shard file
      uint32_t payloadSize;     // Size of pixel data
      uint16_t width;
      uint16_t height;
      uint16_t stridePixels;
      PixelFormat format;
      bool isCold;              // True if evicted from memory but still on disk
    };
    std::unordered_map<std::vector<uint8_t>, IndexEntry, HashVectorHasher> indexMap_;
    
    // Queue of hashes waiting to be hydrated (background loading)
    std::list<std::vector<uint8_t>> hydrationQueue_;
    
    // Cold entries - evicted from ARC but still on disk
    std::unordered_set<std::vector<uint8_t>, HashVectorHasher> coldEntries_;
    
    // Dirty entry tracking for incremental saves
    std::unordered_set<std::vector<uint8_t>, HashVectorHasher> dirtyEntries_;
    
    // Shard management
    uint16_t currentShardId_;  // Current shard being written to
    FILE* currentShardHandle_; // Handle to current shard for appending
    size_t currentShardSize_;  // Current size of active shard
    std::unordered_map<uint16_t, size_t> shardSizes_;  // Size of each shard
    
    // Bidirectional mapping between protocol-level full hashes and
    // shared in-memory keys. This lets us keep ARC/cache keying
    // identical to ContentCache while still using full hashes for
    // protocol and on-disk persistence.
    std::unordered_map<ContentKey, std::vector<uint8_t>, ContentKeyHash> keyToHash_;
    std::unordered_map<std::vector<uint8_t>, ContentKey, HashVectorHasher> hashToKey_;

    // Helper methods
    std::string getShardPath(uint16_t shardId) const;
    std::string getIndexPath() const;
    bool ensureCacheDir();
    bool openCurrentShard();
    void closeCurrentShard();
    bool writeEntryToShard(const std::vector<uint8_t>& hash, const CachedPixels& entry);

    // Helper to get current timestamp
    uint32_t getCurrentTime() const;
    
    // Prevent copying
    GlobalClientPersistentCache(const GlobalClientPersistentCache&) = delete;
    GlobalClientPersistentCache& operator=(const GlobalClientPersistentCache&) = delete;
  };

}

#endif // __RFB_GLOBAL_CLIENT_PERSISTENT_CACHE_H__
