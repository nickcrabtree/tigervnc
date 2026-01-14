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
#include <string.h>    // memcpy
#include <algorithm>   // std::min

// Forward declaration for file I/O
#include <cstdio>

#include <rfb/PixelFormat.h>
#include <rfb/CacheKey.h>
#include <rfb/cache/ArcCache.h>
#include <rfb/cache/CacheCoordinator.h>

namespace rfb {

  // Debug logger for PersistentCache - logs to tmpfile with timestamps
  class PersistentCacheDebugLogger {
  public:
    static PersistentCacheDebugLogger& getInstance() {
      static PersistentCacheDebugLogger instance;
      return instance;
    }
    
    void log(const std::string& message);
    
    // Convenience methods for cache events
    void logCacheHit(const char* cacheType, int x, int y, int w, int h,
                     uint64_t cacheId, bool isLossless);
    void logCacheMiss(const char* cacheType, int x, int y, int w, int h,
                      uint64_t cacheId);
    void logCacheStore(const char* cacheType, int x, int y, int w, int h,
                       uint64_t cacheId, int encoding, size_t bytes);
    void logCacheSeed(const char* cacheType, int x, int y, int w, int h,
                      uint64_t cacheId, bool hashMatch);
    void logStats(unsigned hits, unsigned misses, unsigned stores,
                  size_t totalEntries, size_t totalBytes);
    
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
      
      // NEW: Dual-hash design for viewer-managed lossy mapping
      uint64_t canonicalHash;           // Server's canonical hash (lossless)
      uint64_t actualHash;              // Client's computed hash (may differ if lossy)
      
      CachedPixels() : width(0), height(0), stridePixels(0), lastAccessTime(0),
                       canonicalHash(0), actualHash(0) {}
      
      size_t byteSize() const {
        return pixels.size();
      }
      
      bool isHydrated() const {
        return !pixels.empty();
      }
      
      bool isLossless() const {
        return canonicalHash == actualHash;
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
    // Convenience lookup by CacheKey used by the unified cache engine
    const CachedPixels* getByKey(const CacheKey& key);
    
    // NEW: Lookup by canonical hash (for viewer-managed lossy mapping)
    // Must match dimensions to avoid hash collisions between different shapes.
    //
    // minBpp (optional): Minimum bits-per-pixel required. If the only matching
    // entries have lower bpp than minBpp, returns nullptr so the caller can
    // request fresh high-quality data from the server. This prevents quality
    // loss when upscaling from a low-quality cached entry (e.g., 8bpp) to a
    // high-quality viewer format (e.g., 32bpp).
    //
    // When minBpp=0 (default), no filtering is applied and the best available
    // entry is returned (preferring higher bpp and lossless over lossy).
    const CachedPixels* getByCanonicalHash(uint64_t canonicalHash, uint16_t width,
                                           uint16_t height, uint8_t minBpp = 0);
    
    // Insert/update a cache entry with dual-hash design.
    // 
    // NEW DESIGN (2025-12-13): Both lossy and lossless entries are persisted.
    // Each entry stores BOTH the canonical hash (server's lossless) and the
    // actual hash (client's computed, may differ if lossy).
    //
    // Parameters:
    //   canonicalHash - Server's canonical hash (from PersistentCachedRectInit)
    //   actualHash - Client's computed hash after decoding (may differ if lossy)
    //   hash - Full protocol hash vector (for disk/index bookkeeping)
    //   pixels - Decoded pixel data
    //   pf - Pixel format
    //   width, height - Rectangle dimensions
    //   stridePixels - Stride in pixels (not bytes)
    //   isPersistable - Always true now (both lossy and lossless persist)
    void insert(uint64_t canonicalHash,
               uint64_t actualHash,
               const std::vector<uint8_t>& hash,
               const uint8_t* pixels,
               const PixelFormat& pf,
               uint16_t width, uint16_t height,
               uint16_t stridePixels,
               bool isPersistable = true);
    
    // Optional: Get all known hashes/IDs for HashList message
    std::vector<std::vector<uint8_t>> getAllHashes() const;
    // Deprecated: 64-bit content IDs are no longer part of the unified cache protocol.
// Callers should use getAllHashes() (protocol hashes).
// std::vector<uint64_t> getAllContentIds() const;
std::vector<CacheKey> getAllKeys() const; // All known keys (16-byte)
    
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

    // Invalidate a cache entry by its unified CacheKey.
// Used when the viewer detects a hash mismatch or corruption.
void invalidateByKey(const CacheKey& key);
    
    // Pending evictions (to notify server). Drained by DecodeManager.
// Returned as unified CacheKey values (16-byte content hashes).
bool hasPendingEvictions() const { return !pendingEvictions_.empty(); }
std::vector<CacheKey> getPendingEvictions() {
  auto keys = pendingEvictions_;
  pendingEvictions_.clear();
  return keys;
}
// Configuration
    void setMaxSize(size_t maxSizeMB);
    void clear();
    
    // Multi-viewer coordination
    // Start the cache coordinator (should be called after loadIndexFromDisk)
    bool startCoordinator();
    // Stop the coordinator (called automatically in destructor)
    void stopCoordinator();
    // Get coordinator role (for diagnostics)
    cache::CacheCoordinator::Role getCoordinatorRole() const;
    // Get coordinator stats
    cache::CacheCoordinator::Stats getCoordinatorStats() const;

    // Expose cache location for logging and diagnostics. These helpers are
    // intentionally narrow so callers do not need to know about the on-disk
    // layout details.
    const std::string& getCacheDirectory() const { return cacheDir_; }
    std::string getIndexFilePath() const { return getIndexPath(); }
    
    // Debug dump: Write comprehensive cache state to a file for post-mortem
    // analysis of corruption issues. Returns the path to the dump file.
    std::string dumpDebugState(const std::string& outputDir = "/tmp") const;
    
  private:
    
    // Queue of hashes evicted from ARC; drained by DecodeManager to
    // notify server. Stored as protocol-level full hashes even though
    // the in-memory ARC key is CacheKey.
    std::vector<CacheKey> pendingEvictions_;
    
    // Shared ARC cache (byte-capacity), keyed by CacheKey just like the
    // original ContentCache. PersistentCache differs only in that it also
    // persists entries to disk.
    std::unique_ptr<rfb::cache::ArcCache<CacheKey, CachedPixels, CacheKeyHash>> arcCache_;

    // In-memory view keyed by CacheKey and bidirectional mapping between
    // CacheKey and full protocol hashes.
    std::unordered_map<CacheKey, CachedPixels, CacheKeyHash> cache_;
    std::unordered_map<CacheKey, std::vector<uint8_t>, CacheKeyHash> keyToHash_;
    std::unordered_map<std::vector<uint8_t>, CacheKey, HashVectorHasher> hashToKey_;

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
    
    // Index entry for lazy loading (v3 format with shard info), keyed by
    // CacheKey so we can reconstitute the in-memory view.
    struct IndexEntry {
      uint16_t shardId;         // Which shard file contains the payload
      uint32_t payloadOffset;   // Offset within the shard file
      uint32_t payloadSize;     // Size of pixel data
      uint16_t width;
      uint16_t height;
      uint16_t stridePixels;
      PixelFormat format;
      bool isCold;              // True if evicted from memory but still on disk
      uint64_t canonicalHash;   // Server's canonical hash (persisted since v4)
      CacheKey key;             // Corresponding in-memory key
      
      // Quality code (v7): 3-bit field encoding color depth and lossy/lossless
      //   Bit 0: Lossy flag (0=lossless, 1=lossy)
      //   Bits 1-2: Color depth code (00=8bpp, 01=16bpp, 10=24/32bpp, 11=reserved)
      // Values: 0=8bpp lossless, 1=8bpp lossy, 2=16bpp lossless, 3=16bpp lossy,
      //         4=24/32bpp lossless, 5=24/32bpp lossy, 6-7=reserved
      uint8_t qualityCode;
      
      IndexEntry() : shardId(0), payloadOffset(0), payloadSize(0), width(0),
                     height(0), stridePixels(0), isCold(false), canonicalHash(0),
                     qualityCode(0) {}
    };
    
    // Helper to compute quality code from pixel format and lossy flag
    static uint8_t computeQualityCode(const PixelFormat& pf, bool isLossy);
    std::unordered_map<std::vector<uint8_t>, IndexEntry, HashVectorHasher> indexMap_;
    
    // Queue of hashes waiting to be hydrated (background loading)
    std::list<std::vector<uint8_t>> hydrationQueue_;
    
    // Cold entries - evicted from ARC but still on disk
    std::unordered_set<std::vector<uint8_t>, HashVectorHasher> coldEntries_;
    
    // Dirty entry tracking for incremental saves (payloads needing to be
    // appended to shard files).
    std::unordered_set<std::vector<uint8_t>, HashVectorHasher> dirtyEntries_;

    // True when indexMap_ has changes that must be persisted to index.dat.
    // We keep this separate from dirtyEntries_ so that if the disk becomes
    // full after writing shard payloads, we can retry saving the index later
    // without re-appending duplicate payloads.
    bool indexDirty_;
    
    // Shard management
    uint16_t currentShardId_;  // Current shard being written to
    FILE* currentShardHandle_; // Handle to current shard for appending
    size_t currentShardSize_;  // Current size of active shard
    std::unordered_map<uint16_t, size_t> shardSizes_;  // Size of each shard
    
    // Multi-viewer coordination
    std::unique_ptr<cache::CacheCoordinator> coordinator_;
    mutable std::mutex coordinatorMutex_;  // Protects coordinator_ access
    
    // Coordinator callbacks
    void onIndexUpdate(const std::vector<cache::WireIndexEntry>& entries);
    bool onWriteRequest(const cache::WireIndexEntry& entry,
                        const std::vector<uint8_t>& payload,
                        cache::WireIndexEntry& resultEntry);

    // Helper methods
    std::string getShardPath(uint16_t shardId) const;
    std::string getIndexPath() const;
    bool ensureCacheDir();
    bool openCurrentShard();
    void closeCurrentShard();
    bool writeEntryToShard(const std::vector<uint8_t>& hash, const CachedPixels& entry);

    // Remove shard_*.dat files that are no longer referenced by indexMap_. This
    // is critical for enforcing maxDiskSize_ across restarts because shardSizes_
    // is reconstructed from the index (and would otherwise ignore orphaned
    // shard files left behind by earlier GC/index rewrites).
    size_t cleanupOrphanShardsOnDisk();

    // Helper to get current timestamp
    uint32_t getCurrentTime() const;
    
    // Prevent copying
    GlobalClientPersistentCache(const GlobalClientPersistentCache&) = delete;
    GlobalClientPersistentCache& operator=(const GlobalClientPersistentCache&) = delete;
  };

}

#endif // __RFB_GLOBAL_CLIENT_PERSISTENT_CACHE_H__
