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

#include <rfb/CConnection.h>
#include <rfb/CMsgWriter.h>
#include <rfb/DecodeManager.h>
#include <rfb/Decoder.h>
#include <rfb/Exception.h>
#include <rfb/PixelBuffer.h>

#include <rdr/MemOutStream.h>

using namespace rfb;

static core::LogWriter vlog("DecodeManager");

DecodeManager::DecodeManager(CConnection *conn_) :
  conn(conn_), threadException(nullptr), contentCache(nullptr), persistentCache(nullptr)
{
  size_t cpuCount;

  memset(decoders, 0, sizeof(decoders));

  memset(stats, 0, sizeof(stats));
  memset(&cacheStats, 0, sizeof(cacheStats));
  memset(&persistentCacheStats, 0, sizeof(persistentCacheStats));
  
  // Initialize client-side content cache (2GB default, unlimited age)
  // Let ARC algorithm handle eviction without time-based constraints
  contentCache = new ContentCache(2048, 0);
  vlog.info("Client ContentCache initialized: 2048MB, unlimited age (ARC-managed)");
  
  // Initialize client-side persistent cache (2GB default)
  // TODO: Read size from parameters in Phase 4
  persistentCache = new GlobalClientPersistentCache(2048);
  vlog.info("Client PersistentCache initialized: 2048MB (ARC-managed)");
  
  // Load persistent cache from disk
  if (persistentCache->loadFromDisk()) {
    vlog.info("PersistentCache loaded from disk");
  } else {
    vlog.debug("PersistentCache starting fresh (no cache file or load failed)");
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
    
  delete contentCache;
  
  // Save persistent cache to disk before destroying
  if (persistentCache) {
    if (persistentCache->saveToDisk()) {
      vlog.info("PersistentCache saved to disk");
    } else {
      vlog.error("Failed to save PersistentCache to disk");
    }
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
  
  // Flush any pending ContentCache eviction notifications
  if (contentCache != nullptr && contentCache->hasPendingEvictions()) {
    std::vector<uint64_t> evictions = contentCache->getPendingEvictions();
    if (!evictions.empty()) {
      vlog.debug("Sending %zu cache eviction notifications to server", evictions.size());
      conn->writer()->writeCacheEviction(evictions);
    }
  }
  
  // Flush any pending PersistentCache queries
  flushPendingQueries();
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
  
  // Log client-side ContentCache ARC statistics
  if (contentCache != nullptr) {
    vlog.info(" ");
    vlog.info("Client-side ContentCache statistics:");
    vlog.info("  Protocol operations (CachedRect received):");
    vlog.info("    Lookups: %u, Hits: %u (%.1f%%)",
              cacheStats.cache_lookups,
              cacheStats.cache_hits,
              cacheStats.cache_lookups > 0 ?
                (100.0 * cacheStats.cache_hits / cacheStats.cache_lookups) : 0.0);
    vlog.info("    Misses: %u", cacheStats.cache_misses);
    
    // Report memory usage
    auto ccStats = contentCache->getStats();
    size_t totalBytes = contentCache->getTotalBytes();
    size_t hashBytes = ccStats.totalBytes;  // Hash cache (server-side structure)
    size_t pixelBytes = totalBytes - hashBytes;  // Pixel cache (client-side decoded pixels)
    size_t maxBytes = 2048ULL * 1024 * 1024;  // Default 2GB
    double pctUsed = (maxBytes > 0) ? (100.0 * totalBytes / maxBytes) : 0.0;
    
    vlog.info("  Cache memory usage:");
    vlog.info("    Hash cache: %s", core::iecPrefix(hashBytes, "B").c_str());
    vlog.info("    Pixel cache: %s", core::iecPrefix(pixelBytes, "B").c_str());
    vlog.info("    Total: %s / %s (%.1f%% used)",
              core::iecPrefix(totalBytes, "B").c_str(),
              core::iecPrefix(maxBytes, "B").c_str(),
              pctUsed);
    
    vlog.info("  ARC cache performance:");
    contentCache->logArcStats();
  }
  
  // Log client-side PersistentCache statistics
  if (persistentCache != nullptr) {
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
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect ENTER: rect=[" + std::to_string(r.tl.x) + "," + std::to_string(r.tl.y) + "-" + std::to_string(r.br.x) + "," + std::to_string(r.br.y) + "], cacheId=" + std::to_string(cacheId) + ", contentCache=" + std::to_string(reinterpret_cast<uintptr_t>(contentCache)) + ", pb=" + std::to_string(reinterpret_cast<uintptr_t>(pb)));
  
  if (contentCache == nullptr || pb == nullptr) {
    vlog.error("handleCachedRect called but cache or framebuffer is null");
    //DebugContentCache_2025-10-14
    rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect EXIT: null pointers");
    return;
  }
  
  cacheStats.cache_lookups++;
  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect: about to call getDecodedPixels for cacheId=" + std::to_string(cacheId));
  
  // Lookup cached pixels by cache ID
  const ContentCache::CachedPixels* cached = contentCache->getDecodedPixels(cacheId);
  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect: getDecodedPixels returned cached=" + std::to_string(reinterpret_cast<uintptr_t>(cached)));
  
  if (cached == nullptr) {
    // Cache miss - request data from server
    cacheStats.cache_misses++;
    vlog.debug("Cache miss for ID %llu, requesting from server",
               (unsigned long long)cacheId);
    
    // Send request to server for this cached data
    conn->writer()->writeRequestCachedData(cacheId);
    return;
  }
  
  cacheStats.cache_hits++;
  
  // Additional diagnostics for stride/bpp prior to blit
  const int dstBpp = pb->getPF().bpp;
  const int srcBpp = cached->format.bpp;
  int dstStridePx = 0;
  // Safe read-only peek at destination stride (pixels)
  (void)pb->getBuffer(r, &dstStridePx);
  size_t srcBytesPerPixel = (size_t)srcBpp / 8;
  size_t dstBytesPerPixel = (size_t)dstBpp / 8;
  size_t rowBytesSrc = (size_t)r.width() * srcBytesPerPixel;
  size_t rowBytesDst = (size_t)r.width() * dstBytesPerPixel;
  vlog.debug("CCDBG: blit cacheId=%llu rect=[%d,%d-%d,%d] srcBpp=%d dstBpp=%d srcStridePx=%d dstStridePx=%d rowBytesSrc=%zu rowBytesDst=%zu",
             (unsigned long long)cacheId,
             r.tl.x, r.tl.y, r.br.x, r.br.y,
             srcBpp, dstBpp,
             cached->stridePixels, dstStridePx,
             rowBytesSrc, rowBytesDst);
  
  vlog.debug("Cache hit for ID %llu: blitting %dx%d to [%d,%d-%d,%d]",
             (unsigned long long)cacheId, cached->width, cached->height,
             r.tl.x, r.tl.y, r.br.x, r.br.y);
  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect: about to call pb->imageRect with cached->pixels.data()=" + std::to_string(reinterpret_cast<uintptr_t>(cached->pixels.data())) + ", cached->pixels.size()=" + std::to_string(cached->pixels.size()) + ", cached->stridePixels=" + std::to_string(cached->stridePixels));
  
  // Blit cached pixels to framebuffer at target position
  pb->imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::handleCachedRect EXIT: imageRect completed successfully");
}

void DecodeManager::storeCachedRect(const core::Rect& r, uint64_t cacheId,
                                   ModifiablePixelBuffer* pb)
{
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::storeCachedRect ENTER: rect=[" + std::to_string(r.tl.x) + "," + std::to_string(r.tl.y) + "-" + std::to_string(r.br.x) + "," + std::to_string(r.br.y) + "], cacheId=" + std::to_string(cacheId));
  
  if (contentCache == nullptr || pb == nullptr) {
    vlog.error("storeCachedRect called but cache or framebuffer is null");
    //DebugContentCache_2025-10-14
    rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::storeCachedRect EXIT: null pointers");
    return;
  }
  
  vlog.debug("Storing decoded rect [%d,%d-%d,%d] with cache ID %llu",
             r.tl.x, r.tl.y, r.br.x, r.br.y,
             (unsigned long long)cacheId);
  
  // Get pixel data from framebuffer
  // CRITICAL: stride from getBuffer() is in pixels, not bytes
  int stridePixels;
  const uint8_t* pixels = pb->getBuffer(r, &stridePixels);
  
  size_t bppBytes = (size_t)pb->getPF().bpp / 8;
  size_t rowBytes = (size_t)r.width() * bppBytes;
  vlog.debug("CCDBG: store cacheId=%llu rect=[%d,%d-%d,%d] bpp=%d stridePx=%d rowBytes=%zu",
             (unsigned long long)cacheId,
             r.tl.x, r.tl.y, r.br.x, r.br.y,
             pb->getPF().bpp, stridePixels, rowBytes);
  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::storeCachedRect: about to call storeDecodedPixels with pixels=" + std::to_string(reinterpret_cast<uintptr_t>(pixels)) + ", stridePixels=" + std::to_string(stridePixels));
  
  // Store in content cache with cache ID
  contentCache->storeDecodedPixels(cacheId, pixels, pb->getPF(),
                                  r.width(), r.height(), stridePixels);
                                  
  //DebugContentCache_2025-10-14
  rfb::ContentCacheDebugLogger::getInstance().log("DecodeManager::storeCachedRect EXIT: storeDecodedPixels completed successfully");
}

void DecodeManager::handlePersistentCachedRect(const core::Rect& r,
                                              const std::vector<uint8_t>& hash,
                                              ModifiablePixelBuffer* pb)
{
  if (persistentCache == nullptr || pb == nullptr) {
    vlog.error("handlePersistentCachedRect called but cache or framebuffer is null");
    return;
  }
  
  persistentCacheStats.cache_lookups++;
  
  // Lookup cached pixels by hash
  const GlobalClientPersistentCache::CachedPixels* cached = persistentCache->get(hash);
  
  if (cached == nullptr) {
    // Cache miss - queue request for later batching
    persistentCacheStats.cache_misses++;
    
    // Format hash for logging (first 8 bytes as hex)
    char hashStr[32];
    snprintf(hashStr, sizeof(hashStr), "%02x%02x%02x%02x%02x%02x%02x%02x",
             hash.size() > 0 ? hash[0] : 0,
             hash.size() > 1 ? hash[1] : 0,
             hash.size() > 2 ? hash[2] : 0,
             hash.size() > 3 ? hash[3] : 0,
             hash.size() > 4 ? hash[4] : 0,
             hash.size() > 5 ? hash[5] : 0,
             hash.size() > 6 ? hash[6] : 0,
             hash.size() > 7 ? hash[7] : 0);
    
    vlog.debug("PersistentCache MISS: rect [%d,%d-%d,%d] hash=%s... (len=%zu), queuing query",
               r.tl.x, r.tl.y, r.br.x, r.br.y, hashStr, hash.size());
    
    // Add to pending queries for batching
    pendingQueries.push_back(hash);
    
    // Flush if we have enough queries (batch size: 10)
    if (pendingQueries.size() >= 10) {
      flushPendingQueries();
    }
    
    return;
  }
  
  persistentCacheStats.cache_hits++;
  
  // Format hash for logging (first 8 bytes as hex)
  char hashStr[32];
  snprintf(hashStr, sizeof(hashStr), "%02x%02x%02x%02x%02x%02x%02x%02x",
           hash.size() > 0 ? hash[0] : 0,
           hash.size() > 1 ? hash[1] : 0,
           hash.size() > 2 ? hash[2] : 0,
           hash.size() > 3 ? hash[3] : 0,
           hash.size() > 4 ? hash[4] : 0,
           hash.size() > 5 ? hash[5] : 0,
           hash.size() > 6 ? hash[6] : 0,
           hash.size() > 7 ? hash[7] : 0);
  
  vlog.debug("PersistentCache HIT: rect [%d,%d-%d,%d] hash=%s... cached=%dx%d stride=%d",
             r.tl.x, r.tl.y, r.br.x, r.br.y, hashStr,
             cached->width, cached->height, cached->stridePixels);
  
  // Blit cached pixels to framebuffer at target position
  pb->imageRect(cached->format, r, cached->pixels.data(), cached->stridePixels);
}

void DecodeManager::storePersistentCachedRect(const core::Rect& r,
                                             const std::vector<uint8_t>& hash,
                                             ModifiablePixelBuffer* pb)
{
  if (persistentCache == nullptr || pb == nullptr) {
    vlog.error("storePersistentCachedRect called but cache or framebuffer is null");
    return;
  }
  
  // Format hash for logging (first 8 bytes as hex)
  char hashStr[32];
  snprintf(hashStr, sizeof(hashStr), "%02x%02x%02x%02x%02x%02x%02x%02x",
           hash.size() > 0 ? hash[0] : 0,
           hash.size() > 1 ? hash[1] : 0,
           hash.size() > 2 ? hash[2] : 0,
           hash.size() > 3 ? hash[3] : 0,
           hash.size() > 4 ? hash[4] : 0,
           hash.size() > 5 ? hash[5] : 0,
           hash.size() > 6 ? hash[6] : 0,
           hash.size() > 7 ? hash[7] : 0);
  
  vlog.debug("PersistentCache STORE: rect [%d,%d-%d,%d] hash=%s... (len=%zu)",
             r.tl.x, r.tl.y, r.br.x, r.br.y, hashStr, hash.size());
  
  // Get pixel data from framebuffer
  // CRITICAL: stride from getBuffer() is in pixels, not bytes
  int stridePixels;
  const uint8_t* pixels = pb->getBuffer(r, &stridePixels);
  
  size_t bppBytes = (size_t)pb->getPF().bpp / 8;
  size_t pixelBytes = (size_t)r.height() * stridePixels * bppBytes;
  vlog.debug("PersistentCache STORE details: bpp=%d stridePx=%d pixelBytes=%zu",
             pb->getPF().bpp, stridePixels, pixelBytes);
  
  // Store in persistent cache with hash
  persistentCache->insert(hash, pixels, pb->getPF(),
                         r.width(), r.height(), stridePixels);
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
