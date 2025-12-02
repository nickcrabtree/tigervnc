/* Copyright 2015 Pierre Ossman for Cendio AB
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

#ifndef __RFB_DECODEMANAGER_H__
#define __RFB_DECODEMANAGER_H__

#include <condition_variable>
#include <exception>
#include <list>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <memory>
#include <vector>

#include <core/Region.h>

#include <rfb/GlobalClientPersistentCache.h>
#include <rfb/CacheKey.h>
#include <rfb/encodings.h>
#include <rfb/cache/BandwidthStats.h>

namespace core {
  struct Rect;
}

namespace rdr {
  class MemOutStream;
}

namespace rfb {

  class CConnection;
  class Decoder;
  class ModifiablePixelBuffer;
  class ServerParams;

  class DecodeManager {
  public:
    DecodeManager(CConnection *conn);
    ~DecodeManager();

    bool decodeRect(const core::Rect& r, int encoding,
                    ModifiablePixelBuffer* pb);

    void flush();
    
    // Cache protocol extension (legacy ContentCache entry point).
    // In the unified implementation this is backed by a session-only,
    // in-memory cache on the client. PersistentCache reuses the same
    // 64-bit ID space but adds disk-backed persistence and HashList
    // negotiation.
    void handleCachedRect(const core::Rect& r, uint64_t cacheId,
                          ModifiablePixelBuffer* pb);
    void storeCachedRect(const core::Rect& r, uint64_t cacheId,
                         ModifiablePixelBuffer* pb);
    
    // PersistentCache protocol extension (cross-session), using 64-bit
    // contentHash/cacheId identifiers on the wire (shared with ContentCache).
    void handlePersistentCachedRect(const core::Rect& r,
                                    uint64_t cacheId,
                                    ModifiablePixelBuffer* pb);
    // PersistentCache INIT: encoding is the inner payload encoding used
    // for this rect. This allows the client cache to treat lossy and
    // lossless payloads differently for on-disk persistence.
    void storePersistentCachedRect(const core::Rect& r,
                                   uint64_t cacheId,
                                   int encoding,
                                   ModifiablePixelBuffer* pb);
    // Backwards-compatible helper used by the unified ContentCache entry
    // point, which does not propagate an inner encoding. These rects are
    // treated as effectively lossless for the purposes of disk policy.
    void storePersistentCachedRect(const core::Rect& r,
                                   uint64_t cacheId,
                                   ModifiablePixelBuffer* pb);

    // Log end-of-session decode and cache statistics (client-side)
    void logStats();

    // Advertise any hashes loaded into the client-side PersistentCache to the
    // server via the HashList protocol once the CMsgWriter is available.
    void advertisePersistentCacheHashes();

    // Trigger lazy loading of PersistentCache from disk. Called by CConnection
    // when PersistentCache protocol is first negotiated (on first use).
    // This defers disk I/O until we know the server actually supports PersistentCache.
    void triggerPersistentCacheLoad();

  private:
    void setThreadException();
    void throwThreadException();

    // Session-only ContentCache helpers (no on-disk persistence). These are
    // used both for true ContentCache protocol messages and as a fallback
    // when PersistentCache protocol messages are received but the viewer's
    // PersistentCache option is disabled (PersistentCache=0).
    void handleContentCacheRect(const core::Rect& r, uint64_t cacheId,
                                ModifiablePixelBuffer* pb);
    void storeContentCacheRect(const core::Rect& r, uint64_t cacheId,
                               ModifiablePixelBuffer* pb);

  private:
    CConnection *conn;
    Decoder *decoders[encodingMax+1];

    struct DecoderStats {
      unsigned rects;
      unsigned long long bytes;
      unsigned long long pixels;
      unsigned long long equivalent;
    };

    DecoderStats stats[encodingMax+1];

    struct QueueEntry {
      bool active;
      core::Rect rect;
      int encoding;
      Decoder* decoder;
      const ServerParams* server;
      ModifiablePixelBuffer* pb;
      rdr::MemOutStream* bufferStream;
      core::Region affectedRegion;
    };

    std::list<rdr::MemOutStream*> freeBuffers;
    std::list<QueueEntry*> workQueue;

    std::mutex queueMutex;
    std::condition_variable producerCond;
    std::condition_variable consumerCond;

  private:
    class DecodeThread {
    public:
      DecodeThread(DecodeManager* manager);
      ~DecodeThread();

      void start();
      void stop();

    protected:
      void worker();
      DecodeManager::QueueEntry* findEntry();

    private:
      DecodeManager* manager;

      std::thread* thread;
      bool stopRequested;
    };

    std::list<DecodeThread*> threads;
    std::exception_ptr threadException;
    
    // Track bytes from last decoded rect so we can estimate cache INIT
    // bandwidth (now used by the unified cache engine / PersistentCache).
    size_t lastDecodedRectBytes;
    
    // Client-side persistent cache (PersistentCache - cross-session)
    GlobalClientPersistentCache* persistentCache;
    struct PersistentCacheStats {
      unsigned cache_hits;
      unsigned cache_lookups;
      unsigned cache_misses;
      unsigned stores;
      unsigned queries_sent;
    };
    PersistentCacheStats persistentCacheStats;
    
    // Session-only ContentCache statistics (no disk). These mirror the
    // semantics of the legacy ContentCache implementation but are backed
    // by a byte-bounded ARC in-memory cache keyed by CacheKey.
    struct ContentCacheStats {
      unsigned cache_hits;
      unsigned cache_lookups;
      unsigned cache_misses;
      unsigned stores;
    };
    ContentCacheStats contentCacheStats_;
    
    // Session-only ContentCache storage: ARC-managed in-memory cache keyed
    // by CacheKey, reusing the CachedPixels struct from
    // GlobalClientPersistentCache so that blitting logic is identical.
    std::unique_ptr<rfb::cache::ArcCache<CacheKey,
                                         GlobalClientPersistentCache::CachedPixels,
                                         CacheKeyHash>> contentCache_;
    
    // Pending ContentCache evictions to notify the server about at the next
    // flush(). Stored as 64-bit cache IDs matching the on-wire ContentCache
    // ID space.
    std::vector<uint64_t> contentCachePendingEvictions_;
    
    // PersistentCache bandwidth savings tracking
    rfb::cache::CacheProtocolStats persistentCacheBandwidthStats;
    
    // Batching for PersistentCache queries (vector of 64-bit content IDs)
    std::vector<uint64_t> pendingQueries;
    void flushPendingQueries();

    // One-shot guard to avoid sending the PersistentCache HashList more than
    // once per connection.
    bool persistentHashListSent;

    // Guard to ensure we only trigger PersistentCache disk load once per connection
    bool persistentCacheLoadTriggered;

#ifdef UNIT_TEST
  public:
    // Test-only helper to introspect the unified cache pointer from unit tests
    // without exposing it in production builds.
    GlobalClientPersistentCache* getPersistentCacheForTest() const { return persistentCache; }
#endif
  };

}

#endif
