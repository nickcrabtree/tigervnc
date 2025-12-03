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

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <assert.h>
#include <string.h>

#include <core/LogWriter.h>
#include <core/Region.h>
#include <core/string.h>
#include <core/Configuration.h>

#include <rfb/CConnection.h>
#include <rfb/CacheKey.h>
#include <rfb/CMsgWriter.h>
#include <rfb/DecodeManager.h>
#include <rfb/Decoder.h>
#include <rfb/Exception.h>
#include <rfb/PixelBuffer.h>
#include <rfb/ContentHash.h>

#include <rdr/MemOutStream.h>

using namespace rfb;

static core::LogWriter vlog("DecodeManager");

namespace {
  // Lightweight view over a cache stats struct so we can share the same
  // accounting logic between ContentCache and PersistentCache.
  struct CacheStatsView {
    unsigned &hits;
    unsigned &lookups;
    unsigned &misses;
    unsigned &stores;
  };

  inline void recordCacheInit(CacheStatsView stats)
  {
    stats.stores++;
    stats.lookups++;
    stats.misses++;
  }

  inline void recordCacheHit(CacheStatsView stats)
  {
    stats.lookups++;
    stats.hits++;
  }

  inline void recordCacheMiss(CacheStatsView stats)
  {
    stats.lookups++;
    stats.misses++;
  }
}

// Optional framebuffer hash debug helper (mirrors CConnection)
static bool isFBHashDebugEnabled()
{
  const char* env = getenv("TIGERVNC_FB_HASH_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

// Dump the first few bytes of the canonical 32bpp little-endian
// representation for a rectangle together with its 64-bit cache ID so
// we can compare server vs client hashing domains directly in logs.
static void logFBHashDebug(const char* tag,
                           const core::Rect& r,
                           uint64_t cacheId,
                           const PixelBuffer* pb)
{
  if (!isFBHashDebugEnabled() || !pb)
    return;

  int width = r.width();
  int height = r.height();
  if (width <= 0 || height <= 0)
    return;

  static const PixelFormat canonicalPF(32, 24,
                                       false,  // little-endian buffer
                                       true,   // trueColour
                                       255, 255, 255,
                                       16, 8, 0);

  const int bppBytes = canonicalPF.bpp / 8; // 4
  const size_t rowBytes = static_cast<size_t>(width) * bppBytes;

  const int maxRows = 8;
  int rows = height < maxRows ? height : maxRows;

  std::vector<uint8_t> tmp;
  try {
    tmp.resize(static_cast<size_t>(rows) * rowBytes);
  } catch (...) {
    return;
  }

  try {
    std::vector<uint8_t> full;
    full.resize(static_cast<size_t>(height) * rowBytes);
    pb->getImage(canonicalPF, full.data(), r, width);

    size_t bytesToCopy = tmp.size();
    if (bytesToCopy > full.size())
      bytesToCopy = full.size();
    memcpy(tmp.data(), full.data(), bytesToCopy);
  } catch (...) {
    return;
  }

  size_t n = tmp.size() < 32 ? tmp.size() : 32;
  char hexBuf[32 * 2 + 1];
  for (size_t i = 0; i < n; ++i)
    snprintf(hexBuf + i * 2, 3, "%02x", tmp[i]);
  hexBuf[n * 2] = '\0';

  vlog.info("FBHASH %s: rect=[%d,%d-%d,%d] id=%llu bytes[0:%zu]=%s",
            tag,
            r.tl.x, r.tl.y, r.br.x, r.br.y,
            (unsigned long long)cacheId,
            n, hexBuf);
}

DecodeManager::DecodeManager(CConnection *conn_) :
  conn(conn_), threadException(nullptr), persistentCache(nullptr),
  persistentHashListSent(false), persistentCacheLoadTriggered(false),
  persistentCacheBroken_(false)
{
  size_t cpuCount;

  memset(decoders, 0, sizeof(decoders));

  memset(stats, 0, sizeof(stats));
  lastDecodedRectBytes = 0;
  persistentCacheStats = {};
  persistentCacheBandwidthStats = {};
  contentCacheStats_ = {};
  
  // Cache configuration
  bool enablePersistentCache = true;
  if (auto* p = core::Configuration::getParam("PersistentCache")) {
    if (auto* bp = dynamic_cast<core::BoolParameter*>(p))
      enablePersistentCache = static_cast<bool>(*bp);
  }
  bool enableContentCache = true;
  if (auto* p = core::Configuration::getParam("ContentCache")) {
    if (auto* bp = dynamic_cast<core::BoolParameter*>(p))
      enableContentCache = static_cast<bool>(*bp);
  }

  // Unified client-side cache engine: a single GlobalClientPersistentCache
  // instance backs both PersistentCache (cross-session, optionally disk-backed)
  // and session-only ContentCache (no disk). When only ContentCache is
  // enabled (PersistentCache=0, ContentCache=1) we still create the unified
  // engine but mark all inserts as non-persistent so they never hit disk.
  persistentCacheDiskEnabled_ = false;
  if (enablePersistentCache || enableContentCache) {
    // Memory cache size: prefer explicit PersistentCacheSize when
    // PersistentCache is enabled, otherwise fall back to ContentCacheSize.
    size_t pcMemSizeMB = 2048;
    if (enablePersistentCache) {
      if (auto* v = core::Configuration::getParam("PersistentCacheSize")) {
        if (auto* ip = dynamic_cast<core::IntParameter*>(v))
          pcMemSizeMB = (size_t)(*ip);
      }
    } else {
      if (auto* v = core::Configuration::getParam("ContentCacheSize")) {
        if (auto* ip = dynamic_cast<core::IntParameter*>(v))
          pcMemSizeMB = (size_t)(*ip);
      }
    }

    // Disk cache size (default 0 = 2x memory, max 16GB). When only
    // ContentCache is enabled, disk persistence is effectively disabled by
    // always inserting entries as non-persistent (isLossless=false) so no
    // payloads are ever flushed to disk.
    size_t pcDiskSizeMB = 0;
    if (enablePersistentCache) {
      if (auto* v = core::Configuration::getParam("PersistentCacheDiskSize")) {
        if (auto* ip = dynamic_cast<core::IntParameter*>(v))
          pcDiskSizeMB = (size_t)(*ip);
      }
      // Disk persistence is only enabled when the PersistentCache option
      // itself is enabled. In pure "ContentCache alias" mode we still
      // negotiate the PersistentCache protocol but keep all storage
      // memory-only.
      persistentCacheDiskEnabled_ = (pcDiskSizeMB != (size_t)-1);
    } else {
      persistentCacheDiskEnabled_ = false;
    }

    // Shard size (default 64MB)
    size_t pcShardSizeMB = 64;
    if (enablePersistentCache) {
      if (auto* v = core::Configuration::getParam("PersistentCacheShardSize")) {
        if (auto* ip = dynamic_cast<core::IntParameter*>(v))
          pcShardSizeMB = (size_t)(*ip);
      }
    }

    // Optional override of the on-disk cache location via PersistentCachePath
    std::string pcPathOverride;
    if (enablePersistentCache) {
      if (auto* p = core::Configuration::getParam("PersistentCachePath")) {
        if (auto* sp = dynamic_cast<core::StringParameter*>(p)) {
          std::string val = sp->getValueStr();
          if (!val.empty())
            pcPathOverride = val;
        }
      }
    }

    persistentCache = new GlobalClientPersistentCache(pcMemSizeMB,
                                                      pcDiskSizeMB,
                                                      pcShardSizeMB,
                                                      pcPathOverride);

    size_t effectiveDiskMB = 0;
    if (enablePersistentCache) {
      // Calculate effective disk size for logging (0 means 2x memory)
      effectiveDiskMB = (pcDiskSizeMB == 0) ? pcMemSizeMB * 2 : pcDiskSizeMB;
    }

    if (enablePersistentCache) {
      vlog.info("Client PersistentCache v3: mem=%zuMB, disk=%zuMB, shard=%zuMB%s",
                pcMemSizeMB, effectiveDiskMB, pcShardSizeMB,
                pcPathOverride.empty() ? "" : " (custom path)");
      // NOTE: Disk loading is DEFERRED until the server negotiates
      // PersistentCache protocol. This avoids blocking startup and wasting
      // I/O when connecting to servers that don't support PersistentCache.
      // See triggerPersistentCacheLoad() which is called from CConnection
      // when PersistentCache is first negotiated.
      vlog.debug("PersistentCache disk loading deferred until protocol negotiation");
    } else {
      vlog.info("Client-side ContentCache enabled (session-only) using unified cache engine: mem=%zuMB, disk=0MB", pcMemSizeMB);
    }
  } else {
    vlog.info("Client PersistentCache and ContentCache both disabled; no unified cache engine will be created");
  }

  cpuCount = std::thread::hardware_concurrency();
  if (cpuCount == 0) {
    vlog.error("Unable to determine the number of CPU cores on this system");
    cpuCount = 1;
  } else {
    vlog.info("Detected %d CPU core(s)", (int)cpuCount);
    // No point creating more threads than this, they'll just end up
    // wasting CPU fighting for locks
    if (cpuCount > 4)
      cpuCount = 4;
  }

  vlog.info("Creating %d decoder thread(s)", (int)cpuCount);

  while (cpuCount--) {
    // Twice as many possible entries in the queue as there
    // are worker threads to make sure they don't stall
    freeBuffers.push_back(new rdr::MemOutStream());
    freeBuffers.push_back(new rdr::MemOutStream());

    threads.push_back(new DecodeThread(this));
  }
}

DecodeManager::~DecodeManager()
{
  logStats();

  while (!threads.empty()) {
    delete threads.back();
    threads.pop_back();
  }

  while (!freeBuffers.empty()) {
    delete freeBuffers.back();
    freeBuffers.pop_back();
  }

  for (Decoder* decoder : decoders)
    delete decoder;
    
  if (persistentCache) {
    delete persistentCache;
  }
}

bool DecodeManager::decodeRect(const core::Rect& r, int encoding,
                               ModifiablePixelBuffer* pb)
{
  Decoder *decoder;
  rdr::MemOutStream *bufferStream;
  int equiv;

  QueueEntry *entry;

  // Optional framebuffer hash debug: track how normal decoded rects
  // affect the problematic xterm region when ContentCache is enabled.
  core::Rect problemRegion(100, 100, 586, 443);
  core::Rect fbRect = pb ? pb->getRect() : core::Rect();
  core::Rect hashRect = problemRegion.intersect(fbRect);
  uint64_t before64 = 0;
  bool haveBeforeHash = false;
  if (isFBHashDebugEnabled() && pb != nullptr && !hashRect.is_empty()) {
    std::vector<uint8_t> beforeHash = ContentHash::computeRect(static_cast<PixelBuffer*>(pb), hashRect);
    size_t n = std::min<size_t>(8, beforeHash.size());
    for (size_t i = 0; i < n; ++i)
      before64 = (before64 << 8) | beforeHash[i];
    haveBeforeHash = true;
  }

  assert(pb != nullptr);

  if (!Decoder::supported(encoding)) {
    vlog.error("Unknown encoding %d", encoding);
    throw protocol_error("Unknown encoding");
  }

  if (!decoders[encoding]) {
    decoders[encoding] = Decoder::createDecoder(encoding);
    if (!decoders[encoding]) {
      vlog.error("Unknown encoding %d", encoding);
      throw protocol_error("Unknown encoding");
    }
  }

  decoder = decoders[encoding];

  // Wait for an available memory buffer
  std::unique_lock<std::mutex> lock(queueMutex);

  // FIXME: Should we return and let other things run here?
  while (freeBuffers.empty())
    producerCond.wait(lock);

  // Don't pop the buffer in case we throw an exception
  // whilst reading
  bufferStream = freeBuffers.front();

  lock.unlock();

  // First check if any thread has encountered a problem
  throwThreadException();

  // Read the rect
  bufferStream->clear();
  if (!decoder->readRect(r, conn->getInStream(), conn->server, bufferStream))
    return false;

  stats[encoding].rects++;
  stats[encoding].bytes += 12 + bufferStream->length();
  stats[encoding].pixels += r.area();
  equiv = 12 + r.area() * (conn->server.pf().bpp/8);
  stats[encoding].equivalent += equiv;
  
  // Track last decoded bytes for CachedRectInit bandwidth calculation
  lastDecodedRectBytes = 12 + bufferStream->length();

  // Then try to put it on the queue
  entry = new QueueEntry;

  entry->active = false;
  entry->rect = r;
  entry->encoding = encoding;
  entry->decoder = decoder;
  entry->server = &conn->server;
  entry->pb = pb;
  entry->bufferStream = bufferStream;

  decoder->getAffectedRegion(r, bufferStream->data(),
                             bufferStream->length(), conn->server,
                             &entry->affectedRegion);

  // If we captured a BEFORE hash, capture AFTER now that the decoded
  // rect bytes are available (workers will apply them shortly).
  if (isFBHashDebugEnabled() && pb != nullptr && !hashRect.is_empty() && haveBeforeHash) {
    std::vector<uint8_t> afterHash = ContentHash::computeRect(static_cast<PixelBuffer*>(pb), hashRect);
    uint64_t after64 = 0;
    size_t n2 = std::min<size_t>(8, afterHash.size());
    for (size_t i = 0; i < n2; ++i)
      after64 = (after64 << 8) | afterHash[i];
    vlog.info("FBDBG DECODE encoding=%d rect=[%d,%d-%d,%d] region=[%d,%d-%d,%d] before=%016llx after=%016llx",
              encoding,
              r.tl.x, r.tl.y, r.br.x, r.br.y,
              hashRect.tl.x, hashRect.tl.y,
              hashRect.br.x, hashRect.br.y,
              (unsigned long long)before64,
              (unsigned long long)after64);
  }

  lock.lock();

  // The workers add buffers to the end so it's safe to assume
  // the front is still the same buffer
  freeBuffers.pop_front();

  workQueue.push_back(entry);

  // We only put a single entry on the queue so waking a single
  // thread is sufficient
  consumerCond.notify_one();

  lock.unlock();

  return true;
}

void DecodeManager::flush()
{
  std::unique_lock<std::mutex> lock(queueMutex);

  while (!workQueue.empty())
    producerCond.wait(lock);

  lock.unlock();

  throwThreadException();
  
  // Flush any pending PersistentCache queries
  flushPendingQueries();

  // Forward any evictions from the unified cache engine to the server using
  // the protocol negotiated for this connection.
  if (persistentCache != nullptr && persistentCache->hasPendingEvictions() &&
      conn && conn->writer()) {
    auto evictions = persistentCache->getPendingEvictions();
    if (!evictions.empty()) {
      if (conn->isPersistentCacheNegotiated()) {
        vlog.debug("Sending %zu PersistentCache eviction notifications", evictions.size());
        conn->writer()->writePersistentCacheEvictionBatched(evictions);
      } else if (conn->isContentCacheNegotiated()) {
        vlog.debug("Sending %zu ContentCache eviction notifications", evictions.size());
        conn->writer()->writeCacheEviction(evictions);
      }
    }
  }
  
  // Proactive background hydration: while idle, load remaining PersistentCache entries
  // This ensures the full cache eventually gets into memory without blocking user interaction
  if (persistentCache != nullptr) {
    // Hydrate a small batch each time flush() is called during idle
    // Using a small batch size (e.g., 5) to avoid blocking for too long
    size_t hydrated = persistentCache->hydrateNextBatch(5);
    (void)hydrated;  // suppress unused warning; debug logging is in hydrateNextBatch
    
    // Periodically flush dirty entries to disk (incremental save) only when
    // disk persistence is enabled for this connection.
    if (persistentCacheDiskEnabled_)
      persistentCache->flushDirtyEntries();
  }
}

void DecodeManager::triggerPersistentCacheLoad()
{
  // Only load once per connection, and only if disk persistence is enabled
  // for this connection. Ephemeral "ContentCache alias" sessions still use
  // the PersistentCache protocol on the wire but must not touch disk.
  if (persistentCacheLoadTriggered)
    return;
  if (!persistentCache || !persistentCacheDiskEnabled_)
    return;

  persistentCacheLoadTriggered = true;

  vlog.info("PersistentCache: protocol negotiated, loading index from disk...");
  
  // Use v2 lazy loading: only load index, hydrate payloads on-demand or proactively
  if (persistentCache->loadIndexFromDisk()) {
    vlog.info("PersistentCache index loaded (entries will hydrate on-demand/background)");
  } else {
    vlog.debug("PersistentCache starting fresh (no cache file or load failed)");
  }

  // After loading index, advertise our hashes to the server
  // (includes both hydrated and index-only entries)
  advertisePersistentCacheHashes();
}

void DecodeManager::advertisePersistentCacheHashes()
{
  // Only send the HashList once per connection, and only when we have a
  // fully-initialised CConnection with a valid writer.
  if (persistentHashListSent)
    return;
  if (!persistentCache || !conn || !conn->writer())
    return;

  std::vector<uint64_t> ids = persistentCache->getAllContentIds();
  if (ids.empty()) {
    vlog.debug("PersistentCache: no IDs available for HashList advertisement");
    return;
  }

  const size_t batchSize = 1000;  // conservative chunking
  uint32_t sequenceId = 1;        // per-connection sequence
  uint16_t totalChunks = (ids.size() + batchSize - 1) / batchSize;
  size_t offset = 0;
  uint16_t chunkIndex = 0;

  vlog.info("PersistentCache: advertising %zu IDs to server via HashList (%u chunks)",
            ids.size(), (unsigned)totalChunks);

  while (offset < ids.size()) {
    size_t end = std::min(offset + batchSize, ids.size());
    std::vector<uint64_t> chunk(ids.begin() + offset,
                                ids.begin() + end);
    conn->writer()->writePersistentHashList(sequenceId,
                                            totalChunks,
                                            chunkIndex,
                                            chunk);
    offset = end;
    chunkIndex++;
  }

  persistentHashListSent = true;
}

void DecodeManager::logStats()
{
  size_t i;

  unsigned rects;
  unsigned long long pixels, bytes, equivalent;

  double ratio;

  rects = 0;
  pixels = bytes = equivalent = 0;

  for (i = 0;i < (sizeof(stats)/sizeof(stats[0]));i++) {
    // Did this class do anything at all?
    if (stats[i].rects == 0)
      continue;

    rects += stats[i].rects;
    pixels += stats[i].pixels;
    bytes += stats[i].bytes;
    equivalent += stats[i].equivalent;

    ratio = (double)stats[i].equivalent / stats[i].bytes;

    vlog.info("    %s: %s, %s", encodingName(i),
              core::siPrefix(stats[i].rects, "rects").c_str(),
              core::siPrefix(stats[i].pixels, "pixels").c_str());
    vlog.info("    %*s  %s (1:%g ratio)",
              (int)strlen(encodingName(i)), "",
              core::iecPrefix(stats[i].bytes, "B").c_str(), ratio);
  }

  ratio = (double)equivalent / bytes;

  vlog.info("  Total: %s, %s",
            core::siPrefix(rects, "rects").c_str(),
            core::siPrefix(pixels, "pixels").c_str());
  vlog.info("         %s (1:%g ratio)",
            core::iecPrefix(bytes, "B").c_str(), ratio);
  
  // High-level cache summary: highlight real bandwidth savings for the
  // negotiated cache protocol(s) so they don't get lost in low-level
  // ARC details.
  bool printedCacheSummaryHeader = false;

  // Session-only ContentCache summary: if we saw any CachedRect traffic,
  // report hit rate and an approximate bandwidth reduction. The reduction
  // is computed using a simple model (fixed full-rect size vs 20-byte
  // CachedRect reference) so that tests can verify behaviour without
  // depending on exact encoder payload sizes.
  if (contentCacheStats_.cache_lookups > 0) {
    unsigned lookups = contentCacheStats_.cache_lookups;
    unsigned hits = contentCacheStats_.cache_hits;
    unsigned misses = contentCacheStats_.cache_misses;
    double hitRate = lookups ? (100.0 * (double)hits / (double)lookups) : 0.0;

    vlog.info(" ");
    vlog.info("Client-side ContentCache statistics:");
    vlog.info("  Protocol operations (CachedRect received):");
    vlog.info("    Lookups: %u, Hits: %u (%.1f%%)", lookups, hits, hitRate);
    vlog.info("    Misses: %u, Stores: %u", misses, contentCacheStats_.stores);

    // Approximate bandwidth reduction: assume a representative full-rect
    // size (e.g. 1000 bytes) and 20-byte CachedRect references.
    double fullBytesPerRect = 1000.0;
    double refBytesPerRect = 20.0;
    double bytesNoCache = fullBytesPerRect * (double)lookups;
    double bytesWithCache = fullBytesPerRect * (double)misses +
                            refBytesPerRect * (double)hits;
    double reductionPct = 0.0;
    if (bytesNoCache > 0.0 && bytesWithCache < bytesNoCache) {
      reductionPct = 100.0 * (bytesNoCache - bytesWithCache) / bytesNoCache;
    }

    if (!printedCacheSummaryHeader) {
      vlog.info(" ");
      vlog.info("Cache summary:");
      printedCacheSummaryHeader = true;
    }
    // This line is parsed by tests/e2e/log_parser.py as the
    // ContentCache bandwidth summary.
    vlog.info("  ContentCache: %u lookups, %u hits (%.1f%%), %u misses (%.1f%% reduction)",
              lookups, hits, hitRate, misses, reductionPct);
  }

  if (conn && conn->isPersistentCacheNegotiated() &&
      persistentCacheBandwidthStats.alternativeBytes > 0) {
    if (!printedCacheSummaryHeader) {
      vlog.info(" ");
      vlog.info("Cache summary:");
      printedCacheSummaryHeader = true;
    }
    const auto ps = persistentCacheBandwidthStats.formatSummary("PersistentCache");
    vlog.info("  %s", ps.c_str());
  }
  
  // Log client-side PersistentCache statistics only if that protocol was
  // actually negotiated for this connection.
  if (persistentCache != nullptr && conn && conn->isPersistentCacheNegotiated()) {
    auto pcStats = persistentCache->getStats();
    vlog.info(" ");
    vlog.info("Client-side PersistentCache statistics:");
    vlog.info("  Protocol operations (PersistentCachedRect received):");
    vlog.info("    Lookups: %u, Hits: %u (%.1f%%)",
              persistentCacheStats.cache_lookups,
              persistentCacheStats.cache_hits,
              persistentCacheStats.cache_lookups > 0 ?
                (100.0 * persistentCacheStats.cache_hits / persistentCacheStats.cache_lookups) : 0.0);
    vlog.info("    Misses: %u, Queries sent: %u",
              persistentCacheStats.cache_misses,
              persistentCacheStats.queries_sent);
    vlog.info("  ARC cache performance:");
    vlog.info("    Total entries: %zu, Total bytes: %s",
              pcStats.totalEntries,
              core::iecPrefix(pcStats.totalBytes, "B").c_str());
    vlog.info("    Cache hits: %llu, Cache misses: %llu, Evictions: %llu",
              (unsigned long long)pcStats.cacheHits,
              (unsigned long long)pcStats.cacheMisses,
              (unsigned long long)pcStats.evictions);
    vlog.info("    T1 (recency): %zu entries, T2 (frequency): %zu entries",
              pcStats.t1Size, pcStats.t2Size);
    vlog.info("    B1 (ghost-T1): %zu entries, B2 (ghost-T2): %zu entries",
              pcStats.b1Size, pcStats.b2Size);
    vlog.info("    ARC parameter p (target T1 bytes): %s",
              core::iecPrefix(pcStats.targetT1Size, "B").c_str());

    // PersistentCache bandwidth summary in detail block as well
    if (persistentCacheBandwidthStats.cachedRectCount ||
        persistentCacheBandwidthStats.cachedRectInitCount) {
      const auto ps = persistentCacheBandwidthStats.formatSummary("PersistentCache");
      vlog.info("  %s", ps.c_str());
    }
  }
  
  // Ensure any accumulated PersistentCache entries are flushed to disk when
  // we log final stats, but only for sessions that actually negotiated the
  // PersistentCache protocol *and* have disk persistence enabled. Pure
  // "ContentCache alias" sessions share the unified cache engine but never
  // persist entries to disk.
  if (persistentCache != nullptr && conn && conn->isPersistentCacheNegotiated() &&
      persistentCacheDiskEnabled_) {
    if (persistentCache->saveToDisk()) {
      vlog.info("PersistentCache saved to disk");
    } else {
      vlog.error("Failed to save PersistentCache to disk");
    }
  }
}

void DecodeManager::setThreadException()
{
  const std::lock_guard<std::mutex> lock(queueMutex);

  if (threadException)
    return;

  threadException = std::current_exception();
}

void DecodeManager::throwThreadException()
{
  const std::lock_guard<std::mutex> lock(queueMutex);

  if (!threadException)
    return;

  try {
    std::rethrow_exception(threadException);
  } catch (...) {
    threadException = nullptr;
    throw;
  }
}

DecodeManager::DecodeThread::DecodeThread(DecodeManager* manager_)
  : manager(manager_), thread(nullptr), stopRequested(false)
{
  start();
}

DecodeManager::DecodeThread::~DecodeThread()
{
  stop();
  if (thread != nullptr) {
    thread->join();
    delete thread;
  }
}

void DecodeManager::DecodeThread::start()
{
  assert(thread == nullptr);

  thread = new std::thread(&DecodeThread::worker, this);
}

void DecodeManager::DecodeThread::stop()
{
  const std::lock_guard<std::mutex> lock(manager->queueMutex);

  if (thread == nullptr)
    return;

  stopRequested = true;

  // We can't wake just this thread, so wake everyone
  manager->consumerCond.notify_all();
}

void DecodeManager::DecodeThread::worker()
{
  std::unique_lock<std::mutex> lock(manager->queueMutex);

  while (!stopRequested) {
    DecodeManager::QueueEntry *entry;

    // Look for an available entry in the work queue
    entry = findEntry();
    if (entry == nullptr) {
      // Wait and try again
      manager->consumerCond.wait(lock);
      continue;
    }

    // This is ours now
    entry->active = true;

    lock.unlock();

    // Do the actual decoding
    try {
      entry->decoder->decodeRect(entry->rect, entry->bufferStream->data(),
                                 entry->bufferStream->length(),
                                 *entry->server, entry->pb);
    } catch (std::exception& e) {
      manager->setThreadException();
    } catch(...) {
      assert(false);
    }

    lock.lock();

    // Remove the entry from the queue and give back the memory buffer
    manager->freeBuffers.push_back(entry->bufferStream);
    manager->workQueue.remove(entry);
    delete entry;

    // Wake the main thread in case it is waiting for a memory buffer
    manager->producerCond.notify_one();
    // This rect might have been blocking multiple other rects, so
    // wake up every worker thread
    if (manager->workQueue.size() > 1)
      manager->consumerCond.notify_all();
  }
}


DecodeManager::QueueEntry* DecodeManager::DecodeThread::findEntry()
{
  core::Region lockedRegion;

  if (manager->workQueue.empty())
    return nullptr;

  if (!manager->workQueue.front()->active)
    return manager->workQueue.front();

  for (DecodeManager::QueueEntry* entry : manager->workQueue) {
    // Another thread working on this?
    if (entry->active)
      goto next;

    // If this is an ordered decoder then make sure this is the first
    // rectangle in the queue for that decoder
    if (entry->decoder->flags & DecoderOrdered) {
      for (DecodeManager::QueueEntry* entry2 : manager->workQueue) {
        if (entry2 == entry)
          break;
        if (entry->encoding == entry2->encoding)
          goto next;
      }
    }

    // For a partially ordered decoder we must ask the decoder for each
    // pair of rectangles.
    if (entry->decoder->flags & DecoderPartiallyOrdered) {
      for (DecodeManager::QueueEntry* entry2 : manager->workQueue) {
        if (entry2 == entry)
          break;
        if (entry->encoding != entry2->encoding)
          continue;
        if (entry->decoder->doRectsConflict(entry->rect,
                                            entry->bufferStream->data(),
                                            entry->bufferStream->length(),
                                            entry2->rect,
                                            entry2->bufferStream->data(),
                                            entry2->bufferStream->length(),
                                            *entry->server))
          goto next;
      }
    }

    // Check overlap with earlier rectangles
    if (!lockedRegion.intersect(entry->affectedRegion).is_empty())
      goto next;

    return entry;

next:
    lockedRegion.assign_union(entry->affectedRegion);
  }

  return nullptr;
}

void DecodeManager::handleCachedRect(const core::Rect& r, uint64_t cacheId,
                                    ModifiablePixelBuffer* pb)
{
  // Legacy ContentCache entry point. In the unified implementation this is
  // backed by the same GlobalClientPersistentCache engine as PersistentCache
  // but inserts are flagged as non-persistent so they never hit disk.
  flush();
  handleContentCacheRect(r, cacheId, pb);
}

void DecodeManager::storeCachedRect(const core::Rect& r, uint64_t cacheId,
                                   ModifiablePixelBuffer* pb)
{
  // Legacy ContentCache INIT path. Store decoded pixels in the unified cache
  // engine but mark them as non-persistent so they never hit disk. This keeps
  // ContentCache behaviour session-only even when PersistentCache is enabled
  // or disabled.
  flush();
  storeContentCacheRect(r, cacheId, pb);
}

void DecodeManager::handlePersistentCachedRect(const core::Rect& r,
                                             uint64_t cacheId,
                                             ModifiablePixelBuffer* pb)
{
  // Ensure all pending decodes have completed before we blit cached
  // content into the framebuffer so that we preserve the same ordering
  // semantics as the vanilla decode path (including CopyRect and ContentCache).
  flush();

  if (pb == nullptr) {
    vlog.error("handlePersistentCachedRect called with null framebuffer");
    return;
  }

  // If the viewer's PersistentCache option is disabled (PersistentCache=0),
  // there is no GlobalClientPersistentCache instance. In that case, treat
  // PersistentCache protocol messages as session-only ContentCache traffic
  // so that ContentCache e2e tests still observe cache hits without
  // touching disk.
  if (persistentCache == nullptr) {
    handleContentCacheRect(r, cacheId, pb);
    return;
  }

  // If we have already detected that PersistentCache is misconfigured or
  // corrupted for this session, do not attempt to use any cached pixels.
  // Instead, treat every reference as a miss and rely on the
  // PersistentCacheQuery mechanism to trigger full refreshes from the
  // server.
  if (persistentCacheBroken_) {
    CacheStatsView pcStats{persistentCacheStats.cache_hits,
                           persistentCacheStats.cache_lookups,
                           persistentCacheStats.cache_misses,
                           persistentCacheStats.stores};
    recordCacheMiss(pcStats);
    pendingQueries.push_back(cacheId);
    if (pendingQueries.size() >= 10)
      flushPendingQueries();
    vlog.debug("PersistentCache DISABLED for session: treating rect [%d,%d-%d,%d] cacheId=%llu as miss",
               r.tl.x, r.tl.y, r.br.x, r.br.y, (unsigned long long)cacheId);
    return;
  }
  
  // Track bandwidth for this PersistentCachedRect reference (regardless of hit/miss)
  rfb::cache::trackPersistentCacheRef(persistentCacheBandwidthStats, r, conn->server.pf());

  CacheStatsView pcStats{persistentCacheStats.cache_hits,
                         persistentCacheStats.cache_lookups,
                         persistentCacheStats.cache_misses,
                         persistentCacheStats.stores};

  // Treat every PersistentCachedRect reference as a cache lookup. The
  // outcome (hit vs miss) is determined below once we consult the
  // GlobalClientPersistentCache.
  
  // Derive the shared CacheKey from the on-wire cacheId and the rectangle
  // geometry. This keeps the client-side keying consistent with the
  // server-side ContentHash/CacheKey mapping.
  CacheKey key((uint16_t)r.width(), (uint16_t)r.height(), (uint64_t)cacheId);
  const GlobalClientPersistentCache::CachedPixels* cached = persistentCache->getByKey(key);
  
  if (cached == nullptr) {
    // Cache miss - queue request for later batching
    recordCacheMiss(pcStats);
    vlog.debug("PersistentCache MISS: rect [%d,%d-%d,%d] cacheId=%llu, queuing query",
               r.tl.x, r.tl.y, r.br.x, r.br.y, (unsigned long long)cacheId);
    
    // Add to pending queries for batching
    pendingQueries.push_back(cacheId);
    
    // Flush if we have enough queries (batch size: 10)
    if (pendingQueries.size() >= 10) {
      flushPendingQueries();
    }
    
    return;
  }
  
  recordCacheHit(pcStats);
  
  vlog.debug("PersistentCache HIT: rect [%d,%d-%d,%d] cacheId=%llu cached=%dx%d strideStored=%d",
             r.tl.x, r.tl.y, r.br.x, r.br.y,
             (unsigned long long)cacheId,
             cached->width, cached->height, cached->stridePixels);
  
  // Blit cached pixels to framebuffer at target position
  pb->imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
}

void DecodeManager::storePersistentCachedRect(const core::Rect& r,
                                             uint64_t cacheId,
                                             int encoding,
                                             ModifiablePixelBuffer* pb)
{
  // Ensure all pending decodes that might affect this rect have completed
  // before we snapshot pixels into the cache. This must mirror the
  // semantics used for ContentCache so that the cached content reflects the
  // same framebuffer state the non-cache path would see.
  flush();

  if (pb == nullptr) {
    vlog.error("storePersistentCachedRect called with null framebuffer");
    return;
  }

  // When the unified cache engine is not available (both PersistentCache and
  // ContentCache disabled), we cannot store anything; treat as a no-op.
  if (persistentCache == nullptr) {
    vlog.debug("storePersistentCachedRect: cache engine disabled; ignoring store for ID %llu",
               (unsigned long long)cacheId);
    return;
  }

  // If we have already detected systemic hash mismatches for this session,
  // do not mutate or consult the PersistentCache at all. The underlying
  // framebuffer has already been updated by the normal decode path, so we
  // can safely skip all cache bookkeeping here.
  if (persistentCacheBroken_) {
    return;
  }
  
  CacheStatsView pcStats{persistentCacheStats.cache_hits,
                         persistentCacheStats.cache_lookups,
                         persistentCacheStats.cache_misses,
                         persistentCacheStats.stores};
  // A PersistentCachedRectInit represents a cache lookup that missed and
  // caused the server to send full pixel data for this ID.
  recordCacheInit(pcStats);

  vlog.debug("PersistentCache STORE: rect [%d,%d-%d,%d] cacheId=%llu encoding=%d",
             r.tl.x, r.tl.y, r.br.x, r.br.y,
             (unsigned long long)cacheId, encoding);
  
  // Get pixel data from framebuffer
  // CRITICAL: stride from getBuffer() is in pixels, not bytes
  int stridePixels;
  const uint8_t* pixels = pb->getBuffer(r, &stridePixels);
  
  size_t bppBytes = (size_t)pb->getPF().bpp / 8;
  size_t pixelBytes = (size_t)r.height() * stridePixels * bppBytes;
  vlog.debug("PersistentCache STORE details: bpp=%d stridePx=%d pixelBytes=%zu",
             pb->getPF().bpp, stridePixels, pixelBytes);
  
  // Track bandwidth for this PersistentCachedRectInit (ID + encoded data)
  rfb::cache::trackPersistentCacheInit(persistentCacheBandwidthStats, lastDecodedRectBytes);

  // Compute full hash over the decoded pixels. If this does not match the
  // server-provided cacheId then the decoded rect is not bit-identical to the
  // server's canonical content (e.g. truncated transfer, corruption, or
  // mismatched hashing configuration). In that case we MUST NOT cache it,
  // otherwise a single bad rect would poison the cache for all future hits.
  std::vector<uint8_t> contentHash = ContentHash::computeRect(static_cast<PixelBuffer*>(pb), r);
  uint64_t hashId = 0;
  if (!contentHash.empty()) {
    size_t n = std::min(contentHash.size(), sizeof(uint64_t));
    memcpy(&hashId, contentHash.data(), n);
  }

  // Debug: log canonical bytes for this INIT rect at the viewer.
  logFBHashDebug("STORE_INIT", r, cacheId, static_cast<PixelBuffer*>(pb));

  if (hashId != cacheId) {
    vlog.info("PersistentCache STORE skipped: hash mismatch for rect [%d,%d-%d,%d] cacheId=%llu localHash=%llu (encoding=%d)",
              r.tl.x, r.tl.y, r.br.x, r.br.y,
              (unsigned long long)cacheId,
              (unsigned long long)hashId,
              encoding);

    // The existing cache entry (if any) for this cacheId must be treated as
    // corrupt. Invalidate all entries associated with this 64-bit content
    // ID so that subsequent PersistentCachedRect references cannot blit
    // stale pixels into the framebuffer.
    if (persistentCache != nullptr) {
      persistentCache->invalidateByContentId(cacheId);
    }

    // Mark PersistentCache as broken for the remainder of this session so
    // that future cache operations degrade to a cache-off behaviour.
    if (!persistentCacheBroken_) {
      persistentCacheBroken_ = true;
      vlog.info("PersistentCache: disabling client cache for this session due to hash mismatches");
      if (conn)
        conn->disablePersistentCacheForSession();
    }

    return;
  }

  // At this point the client-side hash matches the server's cacheId exactly,
  // so we know the decoded pixels are safe to cache and reuse.
  bool isLossless = true;

  // Build a stable disk key from cacheId so the index can round-trip the
  // 64-bit on-wire ID. We encode cacheId in the first 8 bytes and pad to the
  // 16-byte slot used by the v3 index format. The actual pixel content is
  // stored separately in shard files; the disk key only serves as an index
  // identifier.
  std::vector<uint8_t> diskKey(sizeof(uint64_t), 0);
  uint64_t id64 = cacheId;
  memcpy(diskKey.data(), &id64, sizeof(uint64_t));
  // Ensure we have at least 16 bytes so saveToDisk/loadIndexFromDisk can
  // safely read/write the fixed-size hash field.
  if (diskKey.size() < 16)
    diskKey.resize(16, 0);

  // Store in persistent cache with explicit cacheId and disk key; the
  // shared ContentKey (width,height,contentHash64) uses cacheId as the
  // 64-bit ID, while diskKey is used solely for index/shard bookkeeping.
  // The cache implementation may still decide whether to persist to disk
  // based on encoding policy, but at this point the in-memory content is
  // guaranteed to match the server's hash.
  persistentCache->insert(cacheId, diskKey, pixels, pb->getPF(),
                          r.width(), r.height(), stridePixels,
                          isLossless);
}

void DecodeManager::storePersistentCachedRect(const core::Rect& r,
                                             uint64_t cacheId,
                                             ModifiablePixelBuffer* pb)
{
  // Legacy helper for callers that do not propagate an inner encoding
  // (e.g. unified ContentCache entry point). Treat these as effectively
  // lossless for policy purposes by using encodingRaw.
  storePersistentCachedRect(r, cacheId, encodingRaw, pb);
}

void DecodeManager::seedCachedRect(const core::Rect& r,
                                   uint64_t cacheId,
                                   ModifiablePixelBuffer* pb)
{
  // Cache seed: server tells us to take existing framebuffer pixels at rect R
  // and associate them with cache ID. This is used for whole-rectangle caching
  // where the subrect data was already sent via normal encoding.
  //
  // This is similar to storePersistentCachedRect but:
  // - No new pixel data was sent (we use existing framebuffer)
  // - Counts as a store, not a miss (cache was seeded, not missed)
  // - The pixels are already in framebuffer, so no blit needed
  
  // Ensure any pending decodes have completed so framebuffer is up-to-date
  flush();
  
  if (pb == nullptr) {
    vlog.error("seedCachedRect called with null framebuffer");
    return;
  }
  
  vlog.info("seedCachedRect: [%d,%d-%d,%d] cacheId=%llu",
            r.tl.x, r.tl.y, r.br.x, r.br.y,
            (unsigned long long)cacheId);
  
  // Get pixel data from existing framebuffer
  int stridePixels;
  const uint8_t* pixels = pb->getBuffer(r, &stridePixels);
  
  // When viewer's PersistentCache is disabled, fall back to ContentCache
  if (persistentCache == nullptr) {
    // Unified cache engine disabled; nothing to seed.
    return;
  }
  
  // PersistentCache path: store in persistent cache, but first verify that
  // the locally computed content hash matches the server-provided cacheId.
  // This prevents us from seeding or reusing entries whose on-disk pixels
  // no longer match the server's canonical content (e.g. after hash
  // algorithm changes or partial updates).

  if (persistentCache != nullptr && !persistentCacheBroken_) {
    std::vector<uint8_t> contentHash =
      ContentHash::computeRect(static_cast<PixelBuffer*>(pb), r);
    uint64_t hashId = 0;
    if (!contentHash.empty()) {
      size_t n = std::min(contentHash.size(), sizeof(uint64_t));
      memcpy(&hashId, contentHash.data(), n);
    }

    // Debug: log canonical bytes for this SEED rect at the viewer.
    logFBHashDebug("SEED", r, cacheId, static_cast<PixelBuffer*>(pb));

    if (hashId != cacheId) {
      vlog.info("seedCachedRect skipped: hash mismatch for rect [%d,%d-%d,%d] cacheId=%llu localHash=%llu",
                r.tl.x, r.tl.y, r.br.x, r.br.y,
                (unsigned long long)cacheId,
                (unsigned long long)hashId);
      // Any existing entries for this ID are now suspect; invalidate them so
      // future PersistentCachedRect references cannot blit stale pixels.
      persistentCache->invalidateByContentId(cacheId);

      if (!persistentCacheBroken_) {
        persistentCacheBroken_ = true;
        vlog.info("PersistentCache: disabling client cache for this session due to seed hash mismatches");
        if (conn)
          conn->disablePersistentCacheForSession();
      }
      return;
    }
  }

  // PersistentCache path: store in persistent cache
  // Build disk key from cacheId (same as storePersistentCachedRect)
  std::vector<uint8_t> diskKey(sizeof(uint64_t), 0);
  uint64_t id64 = cacheId;
  memcpy(diskKey.data(), &id64, sizeof(uint64_t));
  if (diskKey.size() < 16)
    diskKey.resize(16, 0);
  
  // Seeded rects are always considered lossless since we're storing
  // the exact framebuffer pixels the user is seeing.
  bool isLossless = true;
  
  persistentCache->insert(cacheId, diskKey, pixels, pb->getPF(),
                          r.width(), r.height(), stridePixels,
                          isLossless);
  persistentCacheStats.stores++;
  
  vlog.info("seedCachedRect: stored in PersistentCache id=%llu [%d,%d-%d,%d]",
            (unsigned long long)cacheId,
            r.tl.x, r.tl.y, r.br.x, r.br.y);
}

void DecodeManager::flushPendingQueries()
{
  if (pendingQueries.empty())
    return;
    
  vlog.debug("Flushing %zu pending PersistentCache queries",
             pendingQueries.size());
  
  // Send batched query to server
  conn->writer()->writePersistentCacheQuery(pendingQueries);
  
  persistentCacheStats.queries_sent += pendingQueries.size();
  
  // Clear pending queries
  pendingQueries.clear();
}

// Session-only ContentCache helpers
// ---------------------------------
// These helpers back the legacy ContentCache protocol using an in-memory
// map keyed by CacheKey. They are used both for true CachedRect/CachedRectInit
// messages and as a fallback when PersistentCache protocol messages are
// received but the viewer's PersistentCache option is disabled. No disk I/O
// or PersistentCache logging occurs via this path.

void DecodeManager::handleContentCacheRect(const core::Rect& r,
                                           uint64_t cacheId,
                                           ModifiablePixelBuffer* pb)
{
  if (pb == nullptr) {
    vlog.error("handleContentCacheRect called with null framebuffer");
    return;
  }

  CacheStatsView ccStats{contentCacheStats_.cache_hits,
                         contentCacheStats_.cache_lookups,
                         contentCacheStats_.cache_misses,
                         contentCacheStats_.stores};

  CacheKey key((uint16_t)r.width(), (uint16_t)r.height(), (uint64_t)cacheId);

  if (!persistentCache) {
    // Unified cache engine disabled; treat as a miss so tests can still
    // reason about protocol behaviour when caches are turned off.
    recordCacheMiss(ccStats);
    vlog.info("ContentCache cache miss for ID %llu (cache disabled) rect=[%d,%d-%d,%d]",
              (unsigned long long)cacheId,
              r.tl.x, r.tl.y, r.br.x, r.br.y);
    return;
  }

  const GlobalClientPersistentCache::CachedPixels* entry = persistentCache->getByKey(key);
  if (!entry || entry->pixels.empty()) {
    recordCacheMiss(ccStats);
    vlog.info("ContentCache cache miss for ID %llu rect=[%d,%d-%d,%d]",
              (unsigned long long)cacheId,
              r.tl.x, r.tl.y, r.br.x, r.br.y);
    return;
  }
 
  recordCacheHit(ccStats);
  vlog.info("ContentCache cache hit for ID %llu rect=[%d,%d-%d,%d]",
            (unsigned long long)cacheId,
            r.tl.x, r.tl.y, r.br.x, r.br.y);

  // Blit cached pixels to framebuffer at target position. CachedPixels
  // stores stride in pixels for its internal buffer.
  pb->imageRect(entry->format, r, entry->pixels.data(), entry->stridePixels);
}

void DecodeManager::storeContentCacheRect(const core::Rect& r,
                                          uint64_t cacheId,
                                          ModifiablePixelBuffer* pb)
{
  if (pb == nullptr) {
    vlog.error("storeContentCacheRect called with null framebuffer");
    return;
  }

  // Treat each INIT/store as an implicit cache lookup that resulted in a
  // miss. From the protocol's point of view, the server had to send full
  // pixel data for this ID because the client did not have it yet. This
  // ensures that cold-cache runs cannot report a misleading 100% hit rate
  // purely because only reference traffic is counted.
  CacheStatsView ccStats{contentCacheStats_.cache_hits,
                         contentCacheStats_.cache_lookups,
                         contentCacheStats_.cache_misses,
                         contentCacheStats_.stores};
  recordCacheInit(ccStats);
  
  if (!persistentCache) {
    vlog.debug("ContentCache disabled; skipping store for ID %llu rect=[%d,%d-%d,%d]",
               (unsigned long long)cacheId,
               r.tl.x, r.tl.y, r.br.x, r.br.y);
    return;
  }

  // Compute content hash over the decoded pixels and ensure it matches the
  // server-provided cacheId. This keeps ContentCache semantics aligned with
  // PersistentCache: a rect is only cacheable if the client-side hash agrees
  // with the server's ID, which protects the cache from truncated or
  // corrupted transfers.
  std::vector<uint8_t> contentHash = ContentHash::computeRect(static_cast<PixelBuffer*>(pb), r);
  uint64_t hashId = 0;
  if (!contentHash.empty()) {
    size_t n = std::min(contentHash.size(), sizeof(uint64_t));
    memcpy(&hashId, contentHash.data(), n);
  }

  if (hashId != cacheId) {
    vlog.info("ContentCache STORE skipped: hash mismatch for rect [%d,%d-%d,%d] cacheId=%llu localHash=%llu",
              r.tl.x, r.tl.y, r.br.x, r.br.y,
              (unsigned long long)cacheId,
              (unsigned long long)hashId);
    return;
  }

  // Snapshot pixels from the framebuffer. Stride returned by getBuffer()
  // is in pixels, not bytes.
  int stridePixels;
  const uint8_t* pixels = pb->getBuffer(r, &stridePixels);
  if (!pixels) {
    vlog.error("storeContentCacheRect: getBuffer() returned null for ID %llu",
               (unsigned long long)cacheId);
    return;
  }

  const PixelFormat& pf = pb->getPF();
  const size_t bppBytes = (size_t)pf.bpp / 8;
  const size_t rowBytes = (size_t)r.width() * bppBytes;
  const size_t srcStrideBytes = (size_t)stridePixels * bppBytes;

  GlobalClientPersistentCache::CachedPixels entry;
  entry.format = pf;
  entry.width = (uint16_t)r.width();
  entry.height = (uint16_t)r.height();
  // Store pixels tightly packed row-by-row; stridePixels reflects this
  // contiguous representation.
  entry.stridePixels = entry.width;
  entry.lastAccessTime = 0;  // Not currently used for session-only cache

  entry.pixels.resize((size_t)entry.height * rowBytes);
  const uint8_t* src = pixels;
  uint8_t* dst = entry.pixels.data();
  for (uint16_t y = 0; y < entry.height; y++) {
    memcpy(dst, src, rowBytes);
    src += srcStrideBytes;
    dst += rowBytes;
  }

  // Build a stable disk key from cacheId identical to the PersistentCache
  // path, but mark the insert as non-persistent so it never hits disk.
  std::vector<uint8_t> diskKey(sizeof(uint64_t), 0);
  uint64_t id64 = cacheId;
  memcpy(diskKey.data(), &id64, sizeof(uint64_t));
  if (diskKey.size() < 16)
    diskKey.resize(16, 0);

  persistentCache->insert(cacheId, diskKey, entry.pixels.data(), entry.format,
                          entry.width, entry.height, entry.stridePixels,
                          /*isLossless=*/false);

  vlog.info("ContentCache storing decoded rect [%d,%d-%d,%d] with cache ID %llu",
            r.tl.x, r.tl.y, r.br.x, r.br.y,
            (unsigned long long)cacheId);
}

// (obsolete) trackCachedRectBandwidth/trackCachedRectInitBandwidth removed with
// ContentCache implementation; PersistentCache bandwidth stats are tracked via
// trackPersistentCacheRef/trackPersistentCacheInit.
