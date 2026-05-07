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
#include <rfb/DecodeManager.h>

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <assert.h>
#include <inttypes.h>
#include <stdint.h>
#include <string.h>
#include <sys/stat.h>
#if !defined(WIN32)
#include <dirent.h>
#include <sys/statvfs.h>
#endif

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <string>
#include <vector>

#include <core/Configuration.h>
#include <core/LogWriter.h>
#include <core/Region.h>
#include <core/string.h>
#include <rdr/MemOutStream.h>
#include <rfb/CConnection.h>
#include <rfb/CMsgWriter.h>
#include <rfb/CacheKey.h>
#include <rfb/ContentHash.h>
#include <rfb/Decoder.h>
#include <rfb/Exception.h>
#include <rfb/PixelBuffer.h>
#include <rfb/encodings.h>

namespace rfb {

static core::LogWriter vlog("DecodeManager");

static inline CacheKey makeCacheKeyFromU64(uint64_t id) {
  CacheKey k;
  memcpy(k.bytes.data(), &id, sizeof(id));
  return k;
}

static inline uint64_t cacheKeyFirstU64(const CacheKey &key) {
  uint64_t v = 0;
  memcpy(&v, key.bytes.data(), sizeof(v));
  return v;
}

namespace {
// Lightweight view over a cache stats struct so we can share the same
// accounting logic between CachedRect and PersistentCache.
struct CacheStatsView {
  unsigned *hits;
  unsigned *lookups;
  unsigned *misses;
  unsigned *stores;
};

inline void recordCacheInit(const CacheStatsView &stats) {
  (*stats.stores)++;
  (*stats.lookups)++;
  (*stats.misses)++;
}

inline void recordCacheHit(const CacheStatsView &stats) {
  (*stats.lookups)++;
  (*stats.hits)++;
}

inline void recordCacheMiss(const CacheStatsView &stats) {
  (*stats.lookups)++;
  (*stats.misses)++;
}

static uint64_t bytesFromMBClamped(uint64_t mb) {
  const uint64_t mul = 1024ULL * 1024ULL;
  if (mb > (UINT64_MAX / mul))
    return UINT64_MAX;
  return mb * mul;
}

#if !defined(WIN32)
static bool getFilesystemFreeBytes(const std::string &path,
                                   uint64_t *freeBytes) {
  if (!freeBytes)
    return false;

  struct statvfs vfs;
  if (statvfs(path.c_str(), &vfs) != 0)
    return false;

  *freeBytes =
      static_cast<uint64_t>(vfs.f_bavail) * static_cast<uint64_t>(vfs.f_frsize);
  return true;
}

static uint64_t getDirectorySizeBytes(const std::string &path) {
  struct stat st;
  if (stat(path.c_str(), &st) != 0)
    return 0;
  if (!S_ISDIR(st.st_mode))
    return 0;

  DIR *dir = opendir(path.c_str());
  if (!dir)
    return 0;

  uint64_t total = 0;
  const struct dirent *de;
  while ((de = readdir(dir)) != nullptr) {
    if (strcmp(de->d_name, ".") == 0 || strcmp(de->d_name, "..") == 0)
      continue;

    std::string child = path;
    child += "/";
    child += de->d_name;

    struct stat stChild;
    if (stat(child.c_str(), &stChild) != 0)
      continue;
    if (S_ISREG(stChild.st_mode)) {
      total += static_cast<uint64_t>(stChild.st_size);
    }
  }

  closedir(dir);
  return total;
}
#endif
} // namespace

// Optional framebuffer hash debug helper (mirrors CConnection)
static bool isFBHashDebugEnabled() {
  const char *env = getenv("TIGERVNC_FB_HASH_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

// Dump the first few bytes of the canonical 32bpp little-endian
// representation for a rectangle together with its 64-bit cache ID so
// we can compare server vs client hashing domains directly in logs.
static void logFBHashDebug(const char *tag, const core::Rect &r,
                           uint64_t cacheId, const PixelBuffer *pb) {
  if (!isFBHashDebugEnabled() || !pb)
    return;

  int width = r.width();
  int height = r.height();
  if (width <= 0 || height <= 0)
    return;

  static const PixelFormat canonicalPF(32, 24,
                                       false, // little-endian buffer
                                       true,  // trueColour
                                       255, 255, 255, 16, 8, 0);

  const int bppBytes = canonicalPF.bpp / 8; // 4
  const size_t rowBytes = static_cast<size_t>(width) * bppBytes;

  const int maxRows = 8;
  const int rows = (height < maxRows) ? height : maxRows;

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
  for (size_t i = 0; i < n; ++i) {
    snprintf(hexBuf + i * 2, sizeof(hexBuf) - (i * 2), "%02x", tmp[i]);
  }
  hexBuf[n * 2] = '\0';

  vlog.debug("FBHASH %s: rect=[%d,%d-%d,%d] id=%" PRIu64 " bytes[0:%zu]=%s",
             tag, r.tl.x, r.tl.y, r.br.x, r.br.y, cacheId, n, hexBuf);
}

DecodeManager::DecodeManager(CConnection *conn_)
    : conn(conn_), threadException(nullptr), persistentCache(nullptr),
      persistentCacheEnabled_(true), persistentHashListSent(false),
      persistentCacheLoadTriggered(false), arcEvictionLogInitialized_(false),
      lastArcEvictions_(0) {
  size_t cpuCount;

  memset(decoders, 0, sizeof(decoders));

  memset(stats, 0, sizeof(stats));
  lastDecodedRectBytes = 0;
  persistentCacheStats = {};
  persistentCacheBandwidthStats = {};

  // Cache configuration
  bool enablePersistentCache = true;
  if (auto *p = core::Configuration::getParam("PersistentCache")) {
    if (const auto *bp = dynamic_cast<const core::BoolParameter *>(p))
      enablePersistentCache = static_cast<bool>(*bp);
  }
  vlog.info("Cache config: enablePersistentCache=%s",
            enablePersistentCache ? "true" : "false");
  persistentCacheEnabled_ = enablePersistentCache;

  // Construct the unified cache engine regardless of the PersistentCache toggle
  // so unit tests and internal callers can rely on its presence. Protocol usage
  // and disk persistence are gated by
  // persistentCacheEnabled_/persistentCacheDiskEnabled_.
  size_t pcMemSizeMB = 2048;
  if (auto *v = core::Configuration::getParam("PersistentCacheSize")) {
    if (const auto *ip = dynamic_cast<const core::IntParameter *>(v))
      pcMemSizeMB = static_cast<size_t>(*ip);
  }
  size_t ccMemSizeMB = 256;
  if (auto *v = core::Configuration::getParam("ContentCacheSize")) {
    if (const auto *ip = dynamic_cast<const core::IntParameter *>(v))
      ccMemSizeMB = static_cast<size_t>(*ip);
  }
  size_t pcDiskSizeMB = 0;
  if (auto *v = core::Configuration::getParam("PersistentCacheDiskSize")) {
    if (const auto *ip = dynamic_cast<const core::IntParameter *>(v))
      pcDiskSizeMB = static_cast<size_t>(*ip);
  }
  size_t pcShardSizeMB = 8;
  if (auto *v = core::Configuration::getParam("PersistentCacheShardSize")) {
    if (const auto *ip = dynamic_cast<const core::IntParameter *>(v))
      pcShardSizeMB = static_cast<size_t>(*ip);
  }
  std::string pcPathOverride;
  if (auto *p2 = core::Configuration::getParam("PersistentCachePath")) {
    if (const auto *sp = dynamic_cast<const core::StringParameter *>(p2)) {
      std::string val = sp->getValueStr();
      if (!val.empty())
        pcPathOverride = val;
    }
  }
  if (!enablePersistentCache) {
    // Session-only ContentCache compatibility mode:
    // keep the unified engine memory-only, but honour the legacy
    // ContentCacheSize knob instead of collapsing to a 1MB stub cache.
    pcMemSizeMB = ccMemSizeMB;
    pcDiskSizeMB = std::numeric_limits<size_t>::max();
    pcPathOverride.clear();
  }
  persistentCacheDiskEnabled_ =
      enablePersistentCache &&
      (pcDiskSizeMB != std::numeric_limits<size_t>::max());
  const size_t pcAutoDiskSizeMB = 4096;
  const size_t effectiveDiskMB =
      (pcDiskSizeMB == 0) ? pcAutoDiskSizeMB : pcDiskSizeMB;
  persistentCache = new GlobalClientPersistentCache(
      pcMemSizeMB, effectiveDiskMB, pcShardSizeMB, pcPathOverride);
  if (enablePersistentCache) {
    vlog.info(
        "Client PersistentCache v3: mem=%zuMB, disk=%zuMB%s, shard=%zuMB%s",
        pcMemSizeMB, effectiveDiskMB, (pcDiskSizeMB == 0) ? " (auto)" : "",
        pcShardSizeMB, pcPathOverride.empty() ? "" : " (custom path)");
#if !defined(WIN32)
    const std::string &cacheDir = persistentCache->getCacheDirectory();
    uint64_t freeBytes = 0;
    uint64_t existingCacheBytes = getDirectorySizeBytes(cacheDir);
    if (getFilesystemFreeBytes(cacheDir, &freeBytes)) {
      uint64_t requestedBytes =
          bytesFromMBClamped(static_cast<uint64_t>(effectiveDiskMB));
      uint64_t maxReasonableBytes = freeBytes + existingCacheBytes;
      if (requestedBytes != UINT64_MAX && requestedBytes > maxReasonableBytes) {
        vlog.info(
            "PersistentCacheDiskSize warning: requested=%zuMB but filesystem "
            "has only %s free + %s current cache (cache dir %s)",
            effectiveDiskMB, core::iecPrefix(freeBytes, "B").c_str(),
            core::iecPrefix(existingCacheBytes, "B").c_str(), cacheDir.c_str());
      }
    }
#endif
    vlog.debug(
        "PersistentCache disk loading deferred until protocol negotiation");
  } else {
    vlog.info("PersistentCache disabled; cache engine constructed in "
              "memory-only mode");
  }

  cpuCount = std::thread::hardware_concurrency();
  if (cpuCount == 0) {
    vlog.error("Unable to determine the number of CPU cores on this system");
    cpuCount = 1;
  } else {
    vlog.info("Detected %d CPU core(s)",
              static_cast<int>(cpuCount)); // No point creating more threads
                                           // than this, they'll just end up
    // wasting CPU fighting for locks
    if (cpuCount > 4)
      cpuCount = 4;
  }

  vlog.info("Creating %d decoder thread(s)", static_cast<int>(cpuCount));

  while (cpuCount--) {
    // Twice as many possible entries in the queue as there
    // are worker threads to make sure they don't stall
    freeBuffers.push_back(new rdr::MemOutStream());
    freeBuffers.push_back(new rdr::MemOutStream());

    threads.push_back(new DecodeThread(this));
  }
}

DecodeManager::~DecodeManager() {
  logStats();

  while (!threads.empty()) {
    delete threads.back();
    threads.pop_back();
  }

  while (!freeBuffers.empty()) {
    delete freeBuffers.back();
    freeBuffers.pop_back();
  }

  for (Decoder *decoder : decoders)
    delete decoder;

  if (persistentCache) {
    delete persistentCache;
  }
}

bool DecodeManager::decodeRect(const core::Rect &r, int encoding,
                               ModifiablePixelBuffer *pb,
                               const ServerParams *serverOverride) {
  Decoder *decoder;
  rdr::MemOutStream *bufferStream;
  int equiv;

  QueueEntry
      *entry; // Optional framebuffer hash debug: track how normal decoded rects
  // affect the problematic xterm region when CachedRect is enabled.
  core::Rect problemRegion(100, 100, 586, 443);
  core::Rect fbRect = pb ? pb->getRect() : core::Rect();
  core::Rect hashRect = problemRegion.intersect(fbRect);
  uint64_t before64 = 0;
  bool haveBeforeHash = false;
  if (isFBHashDebugEnabled() && pb != nullptr && !hashRect.is_empty()) {
    std::vector<uint8_t> beforeHash =
        ContentHash::computeRect(static_cast<PixelBuffer *>(pb), hashRect);
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

  decoder = decoders[encoding]; // Wait for an available memory buffer
  std::unique_lock<std::mutex> lock(
      queueMutex); // FIXME: Should we return and let other things run here?
  while (freeBuffers.empty())
    producerCond.wait(
        lock); // Don't pop the buffer in case we throw an exception
  // whilst reading
  bufferStream = freeBuffers.front();

  lock.unlock();          // First check if any thread has encountered a problem
  throwThreadException(); // Read the rect
  bufferStream->clear();
  const ServerParams &serverParams =
      serverOverride ? *serverOverride : conn->server;

  if (!decoder->readRect(r, conn->getInStream(), serverParams, bufferStream))
    return false;

  stats[encoding].rects++;
  stats[encoding].bytes += 12 + bufferStream->length();
  stats[encoding].pixels += r.area();
  equiv = 12 + r.area() * (serverParams.pf().bpp / 8);
  stats[encoding].equivalent += equiv; // Track last decoded bytes for
                                       // CachedRectInit bandwidth calculation
  lastDecodedRectBytes =
      12 + bufferStream->length(); // Then try to put it on the queue
  entry = new QueueEntry;

  entry->active = false;
  entry->rect = r;
  entry->encoding = encoding;
  entry->decoder = decoder;
  if (serverOverride) {
    entry->serverParams = *serverOverride;
    entry->server = &entry->serverParams;
  } else {
    entry->server = &conn->server;
  }
  entry->pb = pb;
  entry->bufferStream = bufferStream;
  decoder->getAffectedRegion(
      r, bufferStream->data(), bufferStream->length(), *entry->server,
      &entry->affectedRegion); // If we captured a BEFORE hash, capture AFTER
                               // now that the decoded
  // rect bytes are available (workers will apply them shortly).
  if (isFBHashDebugEnabled() && pb != nullptr && !hashRect.is_empty() &&
      haveBeforeHash) {
    std::vector<uint8_t> afterHash =
        ContentHash::computeRect(static_cast<PixelBuffer *>(pb), hashRect);
    uint64_t after64 = 0;
    size_t n2 = std::min<size_t>(8, afterHash.size());
    for (size_t i = 0; i < n2; ++i)
      after64 = (after64 << 8) | afterHash[i];
    vlog.debug("FBDBG DECODE encoding=%d rect=[%d,%d-%d,%d] "
               "region=[%d,%d-%d,%d] before=%016" PRIx64 " after=%016" PRIx64
               "",
               encoding, r.tl.x, r.tl.y, r.br.x, r.br.y, hashRect.tl.x,
               hashRect.tl.y, hashRect.br.x, hashRect.br.y, before64, after64);
  }

  lock.lock(); // The workers add buffers to the end so it's safe to assume
  // the front is still the same buffer
  freeBuffers.pop_front();

  workQueue.push_back(
      entry); // We only put a single entry on the queue so waking a single
  // thread is sufficient
  consumerCond.notify_one();

  lock.unlock();

  return true;
}

void DecodeManager::flush() {
  std::unique_lock<std::mutex> lock(queueMutex);

  while (!workQueue.empty())
    producerCond.wait(lock);

  lock.unlock();

  throwThreadException(); // Flush any pending PersistentCache queries
  flushPendingQueries();  // Forward any evictions from the unified cache engine
                          // to the server using
  // the protocol negotiated for this connection.
  flushPendingEvictions(); // Proactive background hydration: while idle, load
                           // remaining PersistentCache
  // entries This ensures the full cache eventually gets into memory without
  // blocking user interaction
  if (persistentCache != nullptr) {
    // Hydrate a small batch each time flush() is called during idle
    // Using a small batch size (e.g., 5) to avoid blocking for too long
    size_t hydrated = persistentCache->hydrateNextBatch(5);
    (void)hydrated; // suppress unused warning; debug logging is in
                    // hydrateNextBatch

    // Periodically flush dirty entries to disk (incremental save) only when
    // disk persistence is enabled for this connection.
    if (persistentCacheDiskEnabled_)
      persistentCache->flushDirtyEntries();
  }
}

void DecodeManager::triggerPersistentCacheLoad() {
  vlog.info(
      "triggerPersistentCacheLoad called: triggered=%s cache=%p diskEnabled=%s",
      persistentCacheLoadTriggered ? "true" : "false",
      reinterpret_cast<void *>(persistentCache),
      persistentCacheDiskEnabled_
          ? "true"
          : "false"); // Only load once per connection, and only if disk
                      // persistence is enabled
  // for this connection. Ephemeral "CachedRect alias" sessions still use
  // the PersistentCache protocol on the wire but must not touch disk.
  if (persistentCacheLoadTriggered) {
    vlog.debug("triggerPersistentCacheLoad: already triggered, returning");
    return;
  }
  if (!persistentCache || !persistentCacheDiskEnabled_) {
    vlog.info("triggerPersistentCacheLoad: skipping - cache=%p diskEnabled=%s",
              reinterpret_cast<void *>(persistentCache),
              persistentCacheDiskEnabled_ ? "true" : "false");
    if (!persistentCacheEnabled_)
      return;
    return;
  }

  // Log the concrete cache directory and index file path so users and
  // tests can see exactly where PersistentCache state will be read from.
  const std::string &cacheDir = persistentCache->getCacheDirectory();
  std::string indexPath = persistentCache->getIndexFilePath();

  // Avoid misleading logs: only claim we are loading index.dat when it exists.
  struct stat stIndex;
  bool haveIndex =
      (stat(indexPath.c_str(), &stIndex) == 0) && S_ISREG(stIndex.st_mode);
  if (!haveIndex) {
    vlog.info("PersistentCache: protocol negotiated, no index.dat at %s (fresh "
              "start)",
              indexPath.c_str());
  } else {
    // Emit a short, unambiguous proof line for tests and humans.
    vlog.info("PersistentCache: loading index from index.dat");
    vlog.info("PersistentCache: protocol negotiated, loading index from %s "
              "(directory %s)",
              indexPath.c_str(), cacheDir.c_str());
  }

  // Use v3 lazy loading: only load index, hydrate payloads on-demand/background
  bool loaded = false;
  if (haveIndex) {
    loaded = persistentCache->loadIndexFromDisk();
  }
  if (loaded) {
    // Emit a short proof line that cannot be wrapped away from index.dat.
    vlog.info("PersistentCache: loaded index.dat");
    vlog.info("PersistentCache index loaded from %s (entries will hydrate "
              "on-demand/background)",
              indexPath.c_str());
  } else {
    if (haveIndex) {
      vlog.error(
          "PersistentCache: failed to load index.dat at %s; starting fresh",
          indexPath.c_str());
    } else {
      vlog.debug("PersistentCache starting fresh (no index at %s)",
                 indexPath.c_str());
    }
  }

  // After loading index, advertise our hashes to the server
  // (includes both hydrated and index-only entries)
  advertisePersistentCacheHashes();
}

void DecodeManager::advertisePersistentCacheHashes() {
  // Only send the HashList once per connection, and only when we have a
  // fully-initialised CConnection with a valid writer.
  if (persistentHashListSent)
    return;
  if (!persistentCache || !conn || !conn->writer())
    return;

  std::vector<CacheKey> keys = persistentCache->getAllKeys();
  if (keys.empty()) {
    vlog.debug("PersistentCache: no keys available for HashList advertisement");
    return;
  }

  const size_t batchSize = 1000; // conservative chunking
  uint32_t sequenceId = 1;       // per-connection sequence
  uint16_t totalChunks = (keys.size() + batchSize - 1) / batchSize;

  size_t offset = 0;
  uint16_t chunkIndex = 0;

  vlog.info("PersistentCache: advertising %zu keys to server via HashList (%u "
            "chunks)",
            keys.size(), (unsigned)totalChunks);

  while (offset < keys.size()) {
    size_t end = std::min(offset + batchSize, keys.size());
    std::vector<CacheKey> chunk(keys.begin() + offset, keys.begin() + end);
    conn->writer()->writePersistentHashList(sequenceId, totalChunks, chunkIndex,
                                            chunk);
    offset = end;
    chunkIndex++;
  }

  persistentHashListSent = true;
}

void DecodeManager::logStats() {
  size_t i;

  unsigned rects;
  uint64_t pixels, bytes, equivalent;

  double ratio;

  rects = 0;
  pixels = bytes = equivalent = 0;

  for (i = 0; i < (sizeof(stats) / sizeof(stats[0])); i++) {
    // Did this class do anything at all?
    if (stats[i].rects == 0)
      continue;

    rects += stats[i].rects;
    pixels += stats[i].pixels;
    bytes += stats[i].bytes;
    equivalent += stats[i].equivalent;

    ratio = static_cast<double>(stats[i].equivalent) / stats[i].bytes;

    vlog.info("    %s: %s, %s", encodingName(i),
              core::siPrefix(stats[i].rects, "rects").c_str(),
              core::siPrefix(stats[i].pixels, "pixels").c_str());
    vlog.info("    %*s  %s (1:%g ratio)",
              static_cast<int>(strlen(encodingName(i))), "",
              core::iecPrefix(stats[i].bytes, "B").c_str(), ratio);
  }

  ratio = static_cast<double>(equivalent) / bytes;

  vlog.info("  Total: %s, %s", core::siPrefix(rects, "rects").c_str(),
            core::siPrefix(pixels, "pixels").c_str());
  vlog.info("         %s (1:%g ratio)", core::iecPrefix(bytes, "B").c_str(),
            ratio); // High-level cache summary: highlight real bandwidth
                    // savings for the
  // negotiated cache protocol(s) so they don't get lost in low-level
  // ARC details.
  bool printedCacheSummaryHeader = false;
  auto ensureCacheSummaryHeader = [&]() {
    if (printedCacheSummaryHeader)
      return;
    vlog.info(" ");
    vlog.info("Cache summary:");
    printedCacheSummaryHeader = true;
  }; // Session-only CachedRect summary: if we saw any CachedRect traffic,  //
     // report hit rate and an approximate bandwidth reduction. The reduction
  // is computed using a simple model (fixed full-rect size vs 20-byte
  // CachedRect reference) so that tests can verify behaviour without
  // depending on exact encoder payload sizes.

  if (conn && conn->isPersistentCacheNegotiated() &&
      persistentCacheBandwidthStats.alternativeBytes > 0) {
    ensureCacheSummaryHeader();
    const auto ps =
        persistentCacheBandwidthStats.formatSummary("PersistentCache");
    vlog.info("  %s", ps.c_str());
  }

  // Log client-side PersistentCache statistics only if that protocol was
  // actually negotiated for this connection.
  if (persistentCache != nullptr && conn &&
      conn->isPersistentCacheNegotiated()) {
    auto pcStats = persistentCache->getStats();
    vlog.info(" ");
    vlog.info("Client-side PersistentCache statistics:");
    vlog.info("  Protocol operations (PersistentCachedRect received):");
    vlog.info("    Lookups: %u, Hits: %u (%.1f%%)",
              persistentCacheStats.cache_lookups,
              persistentCacheStats.cache_hits,
              persistentCacheStats.cache_lookups > 0
                  ? (100.0 * persistentCacheStats.cache_hits /
                     persistentCacheStats.cache_lookups)
                  : 0.0);
    vlog.info("    Misses: %u, Queries sent: %u",
              persistentCacheStats.cache_misses,
              persistentCacheStats.queries_sent);
    vlog.info("  ARC cache performance:");
    vlog.info("    Total entries: %zu, Total bytes: %s", pcStats.totalEntries,
              core::iecPrefix(pcStats.totalBytes, "B").c_str());
    vlog.info("    Cache hits: %" PRIu64 ", Cache misses: %" PRIu64
              ", Evictions: %" PRIu64 "",
              pcStats.cacheHits, pcStats.cacheMisses, pcStats.evictions);
    vlog.info("    T1 (recency): %zu entries, T2 (frequency): %zu entries",
              pcStats.t1Size, pcStats.t2Size);
    vlog.info("    B1 (ghost-T1): %zu entries, B2 (ghost-T2): %zu entries",
              pcStats.b1Size, pcStats.b2Size);
    vlog.info("    ARC parameter p (target T1 bytes): %s",
              core::iecPrefix(pcStats.targetT1Size, "B")
                  .c_str()); // PersistentCache bandwidth summary in detail
                             // block as well
    if (persistentCacheBandwidthStats.cachedRectCount ||
        persistentCacheBandwidthStats.cachedRectInitCount) {
      const auto ps =
          persistentCacheBandwidthStats.formatSummary("PersistentCache");
      vlog.info("  %s", ps.c_str());
    }

    // Log final stats to debug file
    PersistentCacheDebugLogger::getInstance().logStats(
        persistentCacheStats.cache_hits, persistentCacheStats.cache_misses,
        persistentCacheStats.stores, pcStats.totalEntries, pcStats.totalBytes);
  }

  // Ensure any accumulated PersistentCache entries are flushed to disk when
  // we log final stats, but only for sessions that actually negotiated the
  // PersistentCache protocol *and* have disk persistence enabled. Pure
  // "CachedRect alias" sessions share the unified cache engine but never
  // persist entries to disk.
  if (persistentCache != nullptr && conn &&
      conn->isPersistentCacheNegotiated() && persistentCacheDiskEnabled_) {
    const std::string &cacheDir = persistentCache->getCacheDirectory();
    std::string indexPath = persistentCache->getIndexFilePath();
    if (persistentCache->saveToDisk()) {
      vlog.info("PersistentCache saved index to %s (directory %s)",
                indexPath.c_str(), cacheDir.c_str());
    } else {
      vlog.error("Failed to save PersistentCache to disk at %s (directory %s)",
                 indexPath.c_str(), cacheDir.c_str());
    }
  }
}

void DecodeManager::setThreadException() {
  const std::lock_guard<std::mutex> lock(queueMutex);

  if (threadException)
    return;

  threadException = std::current_exception();
}

void DecodeManager::throwThreadException() {
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

DecodeManager::DecodeThread::DecodeThread(DecodeManager *manager_)
    : manager(manager_), thread(nullptr), stopRequested(false) {
  start();
}

DecodeManager::DecodeThread::~DecodeThread() {
  stop();
  if (thread != nullptr) {
    thread->join();
    delete thread;
  }
}

void DecodeManager::DecodeThread::start() {
  assert(thread == nullptr);

  thread = new std::thread(&DecodeThread::worker, this);
}

void DecodeManager::DecodeThread::stop() {
  const std::lock_guard<std::mutex> lock(manager->queueMutex);

  if (thread == nullptr)
    return;

  stopRequested = true; // We can't wake just this thread, so wake everyone
  manager->consumerCond.notify_all();
}

void DecodeManager::DecodeThread::worker() {
  std::unique_lock<std::mutex> lock(manager->queueMutex);

  while (!stopRequested) {
    DecodeManager::QueueEntry
        *entry; // Look for an available entry in the work queue
    entry = findEntry();
    if (entry == nullptr) {
      // Wait and try again
      manager->consumerCond.wait(lock);
      continue;
    }

    // This is ours now
    entry->active = true;

    lock.unlock(); // Do the actual decoding
    try {
      entry->decoder->decodeRect(entry->rect, entry->bufferStream->data(),
                                 entry->bufferStream->length(), *entry->server,
                                 entry->pb);
    } catch (std::exception &e) {
      manager->setThreadException();
    } catch (...) {
      assert(false);
    }

    lock.lock(); // Remove the entry from the queue and give back the memory
                 // buffer
    manager->freeBuffers.push_back(entry->bufferStream);
    manager->workQueue.remove(entry);
    delete entry; // Wake the main thread in case it is waiting for a memory
                  // buffer
    manager->producerCond.notify_one(); // This rect might have been blocking
                                        // multiple other rects, so
    // wake up every worker thread
    if (manager->workQueue.size() > 1)
      manager->consumerCond.notify_all();
  }
}

DecodeManager::QueueEntry *DecodeManager::DecodeThread::findEntry() {
  core::Region lockedRegion;

  if (manager->workQueue.empty())
    return nullptr;

  if (!manager->workQueue.front()->active)
    return manager->workQueue.front();

  for (DecodeManager::QueueEntry *entry : manager->workQueue) {
    // Another thread working on this?
    if (entry->active)
      goto next; // If this is an ordered decoder then make sure this is the
                 // first
    // rectangle in the queue for that decoder
    if (entry->decoder->flags & DecoderOrdered) {
      for (const DecodeManager::QueueEntry *entry2 : manager->workQueue) {
        if (entry2 == entry)
          break;
        if (entry->encoding == entry2->encoding)
          goto next;
      }
    }

    // For a partially ordered decoder we must ask the decoder for each
    // pair of rectangles.
    if (entry->decoder->flags & DecoderPartiallyOrdered) {
      for (const DecodeManager::QueueEntry *entry2 : manager->workQueue) {
        if (entry2 == entry)
          break;
        if (entry->encoding != entry2->encoding)
          continue;
        if (entry->decoder->doRectsConflict(
                entry->rect, entry->bufferStream->data(),
                entry->bufferStream->length(), entry2->rect,
                entry2->bufferStream->data(), entry2->bufferStream->length(),
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

void DecodeManager::handleCachedRect(const core::Rect & /*r*/,
                                     const CacheKey & /*key*/,
                                     ModifiablePixelBuffer * /*pb*/) {
  flush();
  throw protocol_error("CachedRect is not supported; use PersistentCachedRect");
}

void DecodeManager::storeCachedRect(const core::Rect & /*r*/,
                                    const CacheKey & /*key*/,
                                    ModifiablePixelBuffer * /*pb*/) {
  flush();
  throw protocol_error(
      "CachedRectInit is not supported; use PersistentCachedRectInit");
}

void DecodeManager::handlePersistentCachedRect(const core::Rect &r,
                                               const CacheKey &key,
                                               ModifiablePixelBuffer *pb) {
  // Ensure all pending decodes have completed before we blit cached
  // content into the framebuffer so that we preserve the same ordering
  // semantics as the vanilla decode path (including CopyRect and CachedRect).
  flush();

  uint64_t cacheId = cacheKeyFirstU64(key);
  if (pb == nullptr) {
    vlog.error("handlePersistentCachedRect called with null framebuffer");
    return;
  }
  if (persistentCache == nullptr) {
    // Unified cache engine unavailable; ignore cache references.
    return;
  }

  // Track bandwidth for this PersistentCachedRect reference (regardless of
  // hit/miss)
  rfb::cache::trackPersistentCacheRef(persistentCacheBandwidthStats, r,
                                      conn->server.pf());

  CacheStatsView pcStats{
      &persistentCacheStats.cache_hits, &persistentCacheStats.cache_lookups,
      &persistentCacheStats.cache_misses,
      &persistentCacheStats.stores}; // Treat every PersistentCachedRect
                                     // reference as a cache lookup. The
  // outcome (hit vs miss) is determined below once we consult the
  // GlobalClientPersistentCache.

  // NEW DESIGN: cacheId is the canonical hash from the server.
  // Look up by canonical hash to find entries with matching canonical,  //
  // regardless of whether we have lossless or lossy version.

  // Cast dimensions to match CacheKey type (uint16_t)
  uint16_t w = static_cast<uint16_t>(r.width());
  uint16_t h = static_cast<uint16_t>(
      r.height()); // Pass viewer's current bpp as minBpp to prevent quality
                   // loss from upscaling.
  // If cache only has lower-bpp entries than the viewer needs, we prefer to
  // miss and request fresh high-quality data from the server rather than
  // upscale low-quality cached data (which causes visible artifacts).
  uint8_t minBpp = pb->getPF().bpp;
  vlog.debug("Cache lookup: cacheId=%" PRIx64 " rect=[%d,%d-%d,%d] minBpp=%d",
             cacheId, r.tl.x, r.tl.y, r.br.x, r.br.y, minBpp);
  const GlobalClientPersistentCache::CachedPixels *cached =
      persistentCache->getByCanonicalHash(cacheId, w, h, minBpp);

  if (cached == nullptr) {
    // Cache miss - queue request for later batching
    recordCacheMiss(pcStats);

    char pbFmtStr[256];
    pb->getPF().print(pbFmtStr, sizeof(pbFmtStr));
    vlog.debug("Cache MISS: cacheId=%" PRIx64
               " rect=[%d,%d-%d,%d] fb_format=[%s] "
               "minBpp=%d (no matching entry or filtered)",
               cacheId, r.tl.x, r.tl.y, r.br.x, r.br.y, pbFmtStr,
               minBpp); // Log to debug file (less verbose than console)
    PersistentCacheDebugLogger::getInstance().logCacheMiss(
        "PersistentCache", r.tl.x, r.tl.y, r.width(), r.height(),
        cacheId); // Add to pending queries for batching
    pendingQueries.push_back(
        cacheId); // Flush if we have enough queries (batch size: 10)
    if (pendingQueries.size() >= 10) {
      flushPendingQueries();
    }

    return;
  }

  // Hit found: We have an entry with matching canonical hash.
  // It could be lossless (actual == canonical) or lossy (actual != canonical).
  // Both are valid - just use the pixels we have.

  bool isLossless = cached->isLossless();

  recordCacheHit(pcStats); // Log to debug file
  PersistentCacheDebugLogger::getInstance().logCacheHit(
      "PersistentCache", r.tl.x, r.tl.y, r.width(), r.height(), cacheId,
      isLossless); // IMPORTANT: We do NOT send
                   // PersistentCacheHashReport on every cache hit.
  // Hash reports are only needed when the viewer detects a canonical!=actual
  // mismatch while storing or seeding a rect (i.e. lossy decode). Emitting a
  // report for every hit adds protocol noise and breaks tests that expect
  // lossless runs to have zero hash-mismatch reports.

  // Blit cached pixels to framebuffer
  // Debug: log format comparison on cache hit
  char cachedFmtStr[256], pbFmtStr[256];
  cached->format.print(cachedFmtStr, sizeof(cachedFmtStr));
  pb->getPF().print(pbFmtStr, sizeof(pbFmtStr));
  if (cached->format != pb->getPF()) {
    vlog.debug(
        "Cache HIT format MISMATCH! cached=[%s] fb=[%s] rect=[%d,%d-%d,%d]",
        cachedFmtStr, pbFmtStr, r.tl.x, r.tl.y, r.br.x, r.br.y);
  } else {
    vlog.debug("Cache HIT format OK: bpp=%d rect=[%d,%d-%d,%d]",
               cached->format.bpp, r.tl.x, r.tl.y, r.br.x, r.br.y);
  }
  pb->imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
}
void DecodeManager::handlePersistentCachedRectWithOffset(
    const core::Rect &r, const CacheKey &key, uint16_t ox, uint16_t oy,
    uint16_t cachedW, uint16_t cachedH, ModifiablePixelBuffer *pb) {
  flush();
  uint64_t cacheId = cacheKeyFirstU64(key);
  if (pb == nullptr) {
    vlog.error(
        "handlePersistentCachedRectWithOffset called with null framebuffer");
    return;
  }
  if (persistentCache == nullptr) {
    return;
  }
  // Track bandwidth for this reference (same accounting as
  // PersistentCachedRect)
  rfb::cache::trackPersistentCacheRef(persistentCacheBandwidthStats, r,
                                      conn->server.pf());
  CacheStatsView pcStats{
      &persistentCacheStats.cache_hits, &persistentCacheStats.cache_lookups,
      &persistentCacheStats.cache_misses, &persistentCacheStats.stores};
  uint8_t minBpp = pb->getPF().bpp;
  const GlobalClientPersistentCache::CachedPixels *cached =
      persistentCache->getByCanonicalHash(cacheId, cachedW, cachedH, minBpp);
  if (cached == nullptr) {
    recordCacheMiss(pcStats);
    pendingQueries.push_back(cacheId);
    if (pendingQueries.size() >= 10) {
      flushPendingQueries();
    }
    return;
  }
  // Bounds check to prevent any out-of-range reads.
  if ((uint32_t)ox + (uint32_t)r.width() > (uint32_t)cachedW ||
      (uint32_t)oy + (uint32_t)r.height() > (uint32_t)cachedH) {
    vlog.error("PersistentCachedRectWithOffset out of bounds: rect=%dx%d "
               "off=%u,%u cached=%ux%u id=%" PRIu64,
               r.width(), r.height(), (unsigned)ox, (unsigned)oy,
               (unsigned)cachedW, (unsigned)cachedH, cacheId);
    throw protocol_error("PersistentCachedRectWithOffset out of bounds");
  }
  bool isLossless = cached->isLossless();
  recordCacheHit(pcStats);
  PersistentCacheDebugLogger::getInstance().logCacheHit(
      "PersistentCache", r.tl.x, r.tl.y, r.width(), r.height(), cacheId,
      isLossless);
  size_t bppBytes = static_cast<size_t>(cached->format.bpp) / 8;
  size_t srcIndex =
      (static_cast<size_t>(oy) * static_cast<size_t>(cached->stridePixels) +
       static_cast<size_t>(ox)) *
      bppBytes;
  if (srcIndex >= cached->pixels.size()) {
    vlog.error("PersistentCachedRectWithOffset source pointer outside cached "
               "buffer: id=%" PRIu64,
               cacheId);
    throw protocol_error(
        "PersistentCachedRectWithOffset invalid source offset");
  }
  const uint8_t *src = cached->pixels.data() + srcIndex;
  pb->imageRect(cached->format, r, src, cached->stridePixels);
}

void DecodeManager::storePersistentCachedRect(const core::Rect &r,
                                              const CacheKey &key, int encoding,
                                              ModifiablePixelBuffer *pb) {
  // Ensure all pending decodes that might affect this rect have completed
  // before we snapshot pixels into the cache. This must mirror the
  // semantics used for CachedRect so that the cached content reflects the
  // same framebuffer state the non-cache path would see.
  flush();

  uint64_t cacheId = cacheKeyFirstU64(key);
  if (pb == nullptr) {
    vlog.error("storePersistentCachedRect called with null framebuffer");
    return;
  }

  // When the unified cache engine is not available (both PersistentCache and
  // CachedRect disabled), we cannot store anything; treat as a no-op.
  if (persistentCache == nullptr) {
    return;
  }

  CacheStatsView pcStats{
      &persistentCacheStats.cache_hits, &persistentCacheStats.cache_lookups,
      &persistentCacheStats.cache_misses,
      &persistentCacheStats.stores}; // A PersistentCachedRectInit represents a
                                     // cache lookup that missed and
  // caused the server to send full pixel data for this ID.
  recordCacheInit(pcStats); // Get pixel data from framebuffer
  // CRITICAL: stride from getBuffer() is in pixels, not bytes
  int stridePixels;
  const uint8_t *pixels = pb->getBuffer(r, &stridePixels);

  size_t bppBytes = static_cast<size_t>(pb->getPF().bpp) / 8;
  size_t pixelBytes = static_cast<size_t>(r.height()) *
                      static_cast<size_t>(stridePixels) *
                      bppBytes; // Debug: log format being stored
  {
    char fmtStr[256];
    pb->getPF().print(fmtStr, sizeof(fmtStr));
    vlog.debug("STORE: rect=[%d,%d-%d,%d] cacheId=%" PRIx64 " encoding=%d "
               "fb_format=[%s] bpp=%d",
               r.tl.x, r.tl.y, r.br.x, r.br.y, cacheId, encoding, fmtStr,
               pb->getPF().bpp);
  }

  // Track bandwidth for this PersistentCachedRectInit (ID + encoded data)
  rfb::cache::trackPersistentCacheInit(
      persistentCacheBandwidthStats,
      lastDecodedRectBytes); // Compute full hash over the decoded pixels. If
                             // this does not match the
  // server-provided cacheId then the decoded rect is not bit-identical to the
  // server's canonical content (e.g. truncated transfer, corruption, or
  // mismatched hashing configuration). In that case we MUST NOT cache it,  //
  // otherwise a single bad rect would poison the cache for all future hits.

  // IMPORTANT: Create a temporary buffer with just these pixels to ensure
  // hash validation later works on the same data layout. This prevents
  // hash mismatches caused by stride differences between storage and retrieval.
  ManagedPixelBuffer tempPB(pb->getPF(), r.width(), r.height());
  tempPB.imageRect(pb->getPF(), tempPB.getRect(), pixels, stridePixels);

  std::vector<uint8_t> contentHash = ContentHash::computeRect(
      static_cast<PixelBuffer *>(&tempPB), tempPB.getRect());
  uint64_t hashId = 0;
  if (!contentHash.empty()) {
    size_t n = std::min(contentHash.size(), sizeof(uint64_t));
    memcpy(&hashId, contentHash.data(), n);
  }

  // Debug: log canonical bytes for this INIT rect at the viewer.
  logFBHashDebug("STORE_INIT", r, cacheId,
                 static_cast<PixelBuffer *>(
                     pb)); // Hash comparison determines if data is lossless:
  // - Hash match: bit-identical (lossless)
  // - Hash mismatch: compression artifacts (lossy)
  bool hashMatch = (hashId == cacheId);

  if (!hashMatch) {
    // For strictly lossless encodings (ZRLE, Hextile, RRE, Raw), a hash
    // mismatch indicates corruption (decoding error, stride issue, etc). We
    // must NOT cache this result, otherwise we will persist and replay the
    // corruption. Tight encoding is the only standard one that can be lossy
    // (JPEG).
    bool encodingCanBeLossy = (encoding == encodingTight);
#ifdef HAVE_H264
    encodingCanBeLossy |= (encoding == encodingH264);
#endif

    if (!encodingCanBeLossy) {
      vlog.debug("PersistentCache STORE: hash mismatch for LOSSLESS encoding "
                 "%d! Dropping corrupt entry for cacheId=%" PRIu64 "",
                 encoding,
                 cacheId); // Do not insert into cache. This forces a miss on
                           // next reference,  // triggering self-healing.
      return;
    }

    // Hash mismatch indicates lossy compression (e.g. JPEG artifacts).
    // Report the lossy hash back to the server so it can learn the
    // canonical->lossy mapping. This enables cache hits on first occurrence
    // instead of second occurrence for lossy content.
    if (conn->writer() != nullptr) {
      if (contentHash.size() >= 16) {
        CacheKey actualKey(contentHash.data());
        conn->writer()->writePersistentCacheHashReport(key, actualKey);
      }
    }
  }

  // NEW DESIGN: Store with BOTH canonical and actual hash.
  // canonicalHash = cacheId (server's lossless hash)
  // actualHash = hashId (client's computed hash, may differ if lossy)
  // Always persist (isPersistable=true), even for lossy entries.
  uint64_t canonicalHash = cacheId;
  uint64_t actualHash =
      hashId; // Build a stable disk key from actualHash (used for indexing)
  std::vector<uint8_t> diskKey(sizeof(uint64_t), 0);
  uint64_t id64 = actualHash;
  memcpy(diskKey.data(), &id64, sizeof(uint64_t));
  if (diskKey.size() < 16)
    diskKey.resize(16, 0); // Store in persistent cache with both hashes.
  // Use the pixel data from our temporary buffer to ensure the same layout
  // that we computed the hash on, preventing validation failures.
  const uint8_t *storedPixels =
      tempPB.getBuffer(tempPB.getRect(), &stridePixels);

  persistentCache->insert(canonicalHash, actualHash, diskKey, storedPixels,
                          tempPB.getPF(), r.width(), r.height(), stridePixels,
                          /*isPersistable=*/true); // NEW: Always persist

  // Log to debug file
  PersistentCacheDebugLogger::getInstance().logCacheStore(
      "PersistentCache", r.tl.x, r.tl.y, r.width(), r.height(), cacheId,
      encoding,
      pixelBytes); // Proactively send any eviction notifications
                   // triggered by this insert
  flushPendingEvictions();
}

void DecodeManager::storePersistentCachedRect(const core::Rect &r,
                                              const CacheKey &key,
                                              ModifiablePixelBuffer *pb) {
  // Legacy helper for callers that do not propagate an inner encoding
  // (e.g. unified CachedRect entry point). Treat these as effectively
  // lossless for policy purposes by using encodingRaw.
  storePersistentCachedRect(r, key, encodingRaw, pb);
}

void DecodeManager::seedCachedRect(const core::Rect &r, const CacheKey &key,
                                   ModifiablePixelBuffer *pb) {
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

  uint64_t cacheId = cacheKeyFirstU64(key);
  if (pb == nullptr) {
    vlog.error("seedCachedRect called with null framebuffer");
    return;
  }

  // Logged to debug file below

  // Get pixel data from existing framebuffer
  int stridePixels;
  const uint8_t *pixels =
      pb->getBuffer(r, &stridePixels); // When viewer's PersistentCache is
                                       // disabled, fall back to CachedRect
  if (persistentCache == nullptr) {
    // Unified cache engine unavailable; nothing to seed.
    return;
  }

  // PersistentCache path: Compute hash of framebuffer pixels and compare
  // to server's canonical hash. If they match (lossless), store under
  // canonical ID. If they don't match (lossy encoding), store under the
  // computed lossy ID and report the mapping to the server.

  if (persistentCache != nullptr) {
    std::vector<uint8_t> contentHash =
        ContentHash::computeRect(static_cast<PixelBuffer *>(pb), r);
    if (contentHash.empty()) {
      vlog.error("seedCachedRect: failed to compute hash for rect "
                 "[%d,%d-%d,%d] canonical=%" PRIu64 "",
                 r.tl.x, r.tl.y, r.br.x, r.br.y, cacheId);
      return;
    }

    uint64_t hashId = 0;
    size_t n = std::min(contentHash.size(), sizeof(uint64_t));
    memcpy(&hashId, contentHash.data(),
           n); // Debug: log canonical bytes for this SEED rect at the viewer.
    logFBHashDebug("SEED", r, cacheId, static_cast<PixelBuffer *>(pb));

    bool hashMatch = (hashId == cacheId);

    if (!hashMatch) {
      // NEW DESIGN: Seed messages are always sent with the server's canonical
      // hash, but the framebuffer pixels we read here may be the result of a
      // lossy decode (e.g. Tight/JPEG). In that case a mismatch is expected.
      // Store using the *actual* hash and report the mapping to the server so
      // future references can still use the canonical ID.
      if (conn && conn->writer() != nullptr) {
        if (contentHash.size() >= 16) {
          CacheKey actualKey(contentHash.data());
          conn->writer()->writePersistentCacheHashReport(key, actualKey);
        }
      }
    }

    // NEW DESIGN: Store with BOTH canonical and actual hash
    uint64_t canonicalHash = cacheId;
    uint64_t actualHash =
        hashId; // Build disk key from actualHash (used for indexing)
    std::vector<uint8_t> diskKey(sizeof(uint64_t), 0);
    uint64_t id64 = actualHash;
    memcpy(diskKey.data(), &id64, sizeof(uint64_t));
    if (diskKey.size() < 16)
      diskKey.resize(16, 0); // Debug: log format being stored
    {
      char fmtStr[256];
      pb->getPF().print(fmtStr, sizeof(fmtStr));
      vlog.debug("SEED: rect=[%d,%d-%d,%d] cacheId=%" PRIx64
                 " actualHash=%" PRIx64 " "
                 "format=[%s] bpp=%d hashMatch=%s",
                 r.tl.x, r.tl.y, r.br.x, r.br.y, canonicalHash, actualHash,
                 fmtStr, pb->getPF().bpp, hashMatch ? "yes" : "no");
    }
    persistentCache->insert(canonicalHash, actualHash, diskKey, pixels,
                            pb->getPF(), r.width(), r.height(), stridePixels,
                            /*isPersistable=*/true); // NEW: Always persist
    persistentCacheStats.stores++;                   // Log to debug file
    PersistentCacheDebugLogger::getInstance().logCacheSeed(
        "PersistentCache", r.tl.x, r.tl.y, r.width(), r.height(), cacheId,
        hashMatch); // Proactively send any eviction notifications
                    // triggered by this insert
    flushPendingEvictions();
    return;
  }
}

void DecodeManager::flushPendingQueries() {
  if (pendingQueries.empty())
    return; // Send batched query to server
  std::vector<CacheKey> queryKeys;
  queryKeys.reserve(pendingQueries.size());
  for (uint64_t id : pendingQueries)
    queryKeys.push_back(makeCacheKeyFromU64(id));
  conn->writer()->writePersistentCacheQuery(queryKeys);

  persistentCacheStats.queries_sent +=
      pendingQueries.size(); // Clear pending queries
  pendingQueries.clear();
}

void DecodeManager::logArcEvictionsThrottled() {
  if (!persistentCache)
    return;

  const auto arcStats = persistentCache->getStats();
  const uint64_t ev = arcStats.evictions; // Initialise on first observation to
                                          // avoid a noisy startup log if
  // the cache was already warm before the first flush/store call.
  if (!arcEvictionLogInitialized_) {
    arcEvictionLogInitialized_ = true;
    lastArcEvictions_ = ev;
    lastArcEvictionLogTime_ = std::chrono::steady_clock::now();
    return;
  }

  if (ev <= lastArcEvictions_)
    return;

  const uint64_t delta = ev - lastArcEvictions_;
  const auto now = std::chrono::steady_clock::now();
  constexpr auto kMinInterval = std::chrono::seconds(1);
  constexpr uint64_t kBurst =
      16; // Throttle by time, but still log large bursts promptly.
  if ((now - lastArcEvictionLogTime_) < kMinInterval && delta < kBurst)
    return;

  vlog.info("ARC evicted %" PRIu64 " entries (total=%" PRIu64
            ", cache=%zu entries, bytes=%s)",
            delta, ev, arcStats.totalEntries,
            core::iecPrefix(arcStats.totalBytes, "B").c_str());

  lastArcEvictionLogTime_ = now;
  lastArcEvictions_ = ev;
}

void DecodeManager::flushPendingEvictions() {
  // Emit throttled ARC eviction logs for production visibility.
  logArcEvictionsThrottled();
  // Forward any evictions from the unified cache engine to the server.
  if (persistentCache != nullptr && persistentCache->hasPendingEvictions() &&
      conn && conn->writer()) {
    auto evictions = persistentCache->getPendingEvictions();
    if (!evictions.empty()) {
      if (conn->isPersistentCacheNegotiated()) {
        vlog.info(
            "Sending %zu PersistentCache eviction notifications to server",
            evictions.size());
        conn->writer()->writePersistentCacheEvictionBatched(evictions);
      } else {
        vlog.debug("Pending evictions (%zu) but no negotiated cache protocol",
                   evictions.size());
      }
    }
  }
}

// (obsolete) trackCachedRectBandwidth/trackCachedRectInitBandwidth removed with
// CachedRect implementation; PersistentCache bandwidth stats are tracked via
// trackPersistentCacheRef/trackPersistentCacheInit.

std::string
DecodeManager::dumpCacheDebugState(const std::string &outputDir) const {
  if (persistentCache) {
    return persistentCache->dumpDebugState(outputDir);
  }
  vlog.info("No PersistentCache engine available for debug dump");
  return "";
}

} // namespace rfb
