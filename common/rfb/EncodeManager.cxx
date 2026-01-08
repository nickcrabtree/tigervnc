/* Copyright (C) 2000-2003 Constantin Kaplinsky.  All Rights Reserved.
 * Copyright (C) 2011 D. R. Commander.  All Rights Reserved.
 * Copyright 2014-2022 Pierre Ossman for Cendio AB
 * Copyright 2018 Peter Astrand for Cendio AB
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

#include <stdlib.h>
#include <assert.h>
#include <ctime>
#include <sys/time.h>
#include <string>

#include <core/LogWriter.h>
#include <core/string.h>

#include <rfb/ContentHash.h>
#include <rfb/Cursor.h>
#include <rfb/EncodeManager.h>
#include <rfb/Encoder.h>
#include <rfb/Palette.h>
#include <rfb/PixelBuffer.h>
#include <rfb/SConnection.h>
#include <rfb/ServerCore.h>
#include <rfb/SMsgWriter.h>
#include <rfb/UpdateTracker.h>
#include <rfb/encodings.h>

#include <rfb/cache/TilingIntegration.h>

#include <rfb/RawEncoder.h>
#include <rfb/RREEncoder.h>
#include <rfb/HextileEncoder.h>
#include <rfb/ZRLEEncoder.h>
#include <rfb/TightEncoder.h>
#include <rfb/TightJPEGEncoder.h>

// For lossy hash computation
#include <rdr/MemInStream.h>
#include <rdr/MemOutStream.h>
#include <rfb/Decoder.h>
#include <rfb/ServerParams.h>

using namespace rfb;

static core::LogWriter vlog("EncodeManager");

// Split each rectangle into smaller ones no larger than this area,
// and no wider than this width.
static const int SubRectMaxArea = 65536;
static const int SubRectMaxWidth = 2048;

// ContentCache debug logging helpers (anonymous namespace)
namespace {
  // Format a rectangle as "x,y wxh"
  inline const char* strRect(const core::Rect& r) {
    static thread_local char buf[64];
    snprintf(buf, sizeof(buf), "%d,%d %dx%d",
             r.tl.x, r.tl.y, r.width(), r.height());
    return buf;
  }

  // Format a region summary as "bbox:(x,y wxh) rects:N"
  inline const char* strRegionSummary(const core::Region& reg) {
    static thread_local char buf[96];
    if (reg.is_empty()) {
      snprintf(buf, sizeof(buf), "bbox:(empty) rects:0");
    } else {
      core::Rect bbox = reg.get_bounding_rect();
      int numRects = reg.numRects();
      snprintf(buf, sizeof(buf), "bbox:(%s) rects:%d",
               strRect(bbox), numRects);
    }
    return buf;
  }

  // Format a 64-bit hash as hex
  inline const char* hex64(uint64_t hash) {
    static thread_local char buf[17];
    snprintf(buf, sizeof(buf), "%016llx", (unsigned long long)hash);
    return buf;
  }

  // Format yes/no
  inline const char* yesNo(bool b) {
    return b ? "yes" : "no";
  }

  // Get current time as epoch.milliseconds (matches viewer log format)
  inline double getEpochTime() {
    struct timeval tv;
    gettimeofday(&tv, nullptr);
    return tv.tv_sec + tv.tv_usec / 1000000.0;
  }

  // Format timestamp for logging
  inline const char* strTimestamp() {
    static thread_local char buf[32];
    snprintf(buf, sizeof(buf), "[%.3f]", getEpochTime());
    return buf;
  }

  // Helper: when TIGERVNC_FB_HASH_DEBUG is set, dump the first few bytes
  // of the canonical 32bpp little-endian representation for a rectangle
  // together with its 64-bit cache ID. This lets us compare server/client
  // hashing domains directly from logs.
  inline bool isFBHashDebugEnabled() {
    const char* env = getenv("TIGERVNC_FB_HASH_DEBUG");
    return env && env[0] != '\0' && env[0] != '0';
  }

  inline void logFBHashDebug(const char* tag,
                             const core::Rect& r,
                             uint64_t cacheId,
                             const PixelBuffer* pb) {
    if (!isFBHashDebugEnabled() || !pb)
      return;

    int width = r.width();
    int height = r.height();
    if (width <= 0 || height <= 0)
      return;

    // Canonical 32bpp format (must match ContentHash::computeRect and
    // client-side debug helpers).
    static const PixelFormat canonicalPF(32, 24,
                                         false,  // little-endian buffer
                                         true,   // trueColour
                                         255, 255, 255,
                                         16, 8, 0);

    const int bppBytes = canonicalPF.bpp / 8; // 4
    const size_t rowBytes = static_cast<size_t>(width) * bppBytes;

    // Limit to a reasonably small buffer; if the rectangle is huge we
    // still only need the first few rows for debugging.
    const int maxRows = 8;
    int rows = height < maxRows ? height : maxRows;

    std::vector<uint8_t> tmp;
    try {
      tmp.resize(static_cast<size_t>(rows) * rowBytes);
    } catch (...) {
      return;
    }

    try {
      // Use getImage() in canonical format with stride=width so data is
      // tightly packed; we only copy the top rows into our smaller buffer.
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

    vlog.info("FBHASH %s: rect=[%d,%d-%d,%d] id=%s bytes[0:%zu]=%s",
              tag,
              r.tl.x, r.tl.y, r.br.x, r.br.y,
              hex64(cacheId), n, hexBuf);
  }

  // (strContentKey helper removed: unified cache path now logs IDs via hex64
  // directly where needed.)
}

// The size in pixels of either side of each block tested when looking
// for solid blocks.
static const int SolidSearchBlock = 16;
// Don't bother with blocks smaller than this
static const int SolidBlockMinArea = 2048;

// How long we consider a region recently changed (in ms)
static const int RecentChangeTimeout = 50;

namespace rfb {

static const char *encoderClassName(EncoderClass klass)
{
  switch (klass) {
  case encoderRaw:
    return "Raw";
  case encoderRRE:
    return "RRE";
  case encoderHextile:
    return "Hextile";
  case encoderTight:
    return "Tight";
  case encoderTightJPEG:
    return "Tight (JPEG)";
  case encoderZRLE:
    return "ZRLE";
  case encoderClassMax:
    break;
  }

  return "Unknown Encoder Class";
}

static const char *encoderTypeName(EncoderType type)
{
  switch (type) {
  case encoderSolid:
    return "Solid";
  case encoderBitmap:
    return "Bitmap";
  case encoderBitmapRLE:
    return "Bitmap RLE";
  case encoderIndexed:
    return "Indexed";
  case encoderIndexedRLE:
    return "Indexed RLE";
  case encoderFullColour:
    return "Full Colour";
  case encoderTypeMax:
    break;
  }

  return "Unknown Encoder Type";
}

// Enable extra ContentCache/encoding debug when this env var is set
static bool isCCDebugEnabled()
{
  const char* env = getenv("TIGERVNC_CC_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

// Separate knob for enabling experimental tiling diagnostics without
// changing on-wire behaviour. When enabled, EncodeManager will run a
// log-only tiling pass on large dirty regions and report what fraction
// could have been served from cache.
static bool isCCTilingDebugEnabled()
{
  const char* env = getenv("TIGERVNC_CC_TILING_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

EncodeManager::EncodeManager(SConnection* conn_)
  : conn(conn_), recentChangeTimer(this), cacheStatsTimer(this),
    usePersistentCache(false)
{
  StatsVector::iterator iter;

  if (isCCDebugEnabled()) {
    vlog.info("TIGERVNC_CC_DEBUG enabled for connection %p", (void*)conn);
  }

  encoders.resize(encoderClassMax, nullptr);
  activeEncoders.resize(encoderTypeMax, encoderRaw);

  encoders[encoderRaw] = new RawEncoder(conn);
  encoders[encoderRRE] = new RREEncoder(conn);
  encoders[encoderHextile] = new HextileEncoder(conn);
  encoders[encoderTight] = new TightEncoder(conn);
  encoders[encoderTightJPEG] = new TightJPEGEncoder(conn);
  encoders[encoderZRLE] = new ZRLEEncoder(conn);

  updates = 0;
  memset(&copyStats, 0, sizeof(copyStats));
  memset(&persistentCacheStats, 0, sizeof(persistentCacheStats));
  stats.resize(encoderClassMax);
  for (iter = stats.begin();iter != stats.end();++iter) {
    StatsVector::value_type::iterator iter2;
    iter->resize(encoderTypeMax);
    for (iter2 = iter->begin();iter2 != iter->end();++iter2)
      memset(&*iter2, 0, sizeof(EncoderStats));
  }
  
  // No separate server-side ContentCache engine: cache protocol now uses
  // a unified PersistentCache-style 64-bit ID path only. The cacheStatsTimer
  // remains available for future periodic logging if we wish to extend
  // persistentCacheStats.
}

EncodeManager::~EncodeManager()
{
  logStats();

  for (Encoder* encoder : encoders)
    delete encoder;
}

bool EncodeManager::isLossyEncoding(int encoding) const
{
  // This function is used for optimization hints only. The actual lossy vs
  // lossless determination happens on the client side via hash comparison:
  // - Client decodes rect and computes hash of decoded pixels
  // - If hash matches server's canonical hash: lossless
  // - If hash differs: lossy (JPEG artifacts, etc)
  // - Client reports lossy hash via message 247 (PersistentCacheHashReport)
  // This approach works for all encodings without server-side encode/decode.
  
  // Tight encoding can be lossy (JPEG) or lossless depending on quality settings
  if (encoding == encodingTight)
    return true;
    
#ifdef HAVE_H264
  // H.264 is always lossy
  if (encoding == encodingH264)
    return true;
#endif
  
  // All other encodings (Raw, RRE, Hextile, ZRLE) are lossless
  return false;
}


void EncodeManager::logStats()
{
  size_t i, j;

  unsigned rects;
  unsigned long long pixels, bytes, equivalent;

  double ratio;

  rects = 0;
  pixels = bytes = equivalent = 0;

  vlog.info("Framebuffer updates: %u", updates);

  if (copyStats.rects != 0) {
    vlog.info("  %s:", "CopyRect");

    rects += copyStats.rects;
    pixels += copyStats.pixels;
    bytes += copyStats.bytes;
    equivalent += copyStats.equivalent;

    ratio = (double)copyStats.equivalent / copyStats.bytes;

    vlog.info("    %s: %s, %s", "Copies",
              core::siPrefix(copyStats.rects, "rects").c_str(),
              core::siPrefix(copyStats.pixels, "pixels").c_str());
    vlog.info("    %*s  %s (1:%g ratio)",
              (int)strlen("Copies"), "",
              core::iecPrefix(copyStats.bytes, "B").c_str(), ratio);
  }

  for (i = 0;i < stats.size();i++) {
    // Did this class do anything at all?
    for (j = 0;j < stats[i].size();j++) {
      if (stats[i][j].rects != 0)
        break;
    }
    if (j == stats[i].size())
      continue;

    vlog.info("  %s:", encoderClassName((EncoderClass)i));

    for (j = 0;j < stats[i].size();j++) {
      if (stats[i][j].rects == 0)
        continue;

      rects += stats[i][j].rects;
      pixels += stats[i][j].pixels;
      bytes += stats[i][j].bytes;
      equivalent += stats[i][j].equivalent;

      ratio = (double)stats[i][j].equivalent / stats[i][j].bytes;

      vlog.info("    %s: %s, %s", encoderTypeName((EncoderType)j),
                core::siPrefix(stats[i][j].rects, "rects").c_str(),
                core::siPrefix(stats[i][j].pixels, "pixels").c_str());
      vlog.info("    %*s  %s (1:%g ratio)",
                (int)strlen(encoderTypeName((EncoderType)j)), "",
                core::iecPrefix(stats[i][j].bytes, "B").c_str(), ratio);
    }
  }

  ratio = (double)equivalent / bytes;

  vlog.info("  Total: %s, %s",
            core::siPrefix(rects, "rects").c_str(),
            core::siPrefix(pixels, "pixels").c_str());
  vlog.info("         %s (1:%g ratio)",
            core::iecPrefix(bytes, "B").c_str(), ratio);

  // Unified cache statistics (covers both ContentCache and PersistentCache
  // protocols on the wire). Preserve the original "Lookups: N, References
  // sent: M (P%)" format consumed by e2e tests such as
  // test_cachedrect_init_propagation.py.
  unsigned cacheLookups = persistentCacheStats.cacheLookups;
  unsigned cacheHits = persistentCacheStats.cacheHits;
  double hitPct = cacheLookups ? (100.0 * (double)cacheHits / (double)cacheLookups) : 0.0;

  vlog.info("Lookups: %u, References sent: %u (%.1f%%)",
            cacheLookups,
            cacheHits,
            hitPct);
}

void EncodeManager::dumpDebugState(const char* outputDir)
{
  std::string filepath = std::string(outputDir) + "/server_cache_state.txt";
  FILE* f = fopen(filepath.c_str(), "w");
  if (!f) {
    vlog.error("Failed to create debug state file: %s", filepath.c_str());
    return;
  }
  
  fprintf(f, "=== Server EncodeManager Debug State ===\n");
  
  time_t now = time(nullptr);
  char timebuf[64];
  strftime(timebuf, sizeof(timebuf), "%Y-%m-%d %H:%M:%S", localtime(&now));
  fprintf(f, "Timestamp: %s\n", timebuf);
  
  fprintf(f, "\n=== Configuration ===\n");
  fprintf(f, "usePersistentCache: %s\n", usePersistentCache ? "true" : "false");
  fprintf(f, "currentEncoding: %d\n", currentEncoding);
  fprintf(f, "currentEncodingIsLossy: %s\n", currentEncodingIsLossy ? "true" : "false");
  
  fprintf(f, "\n=== Cache Statistics ===\n");
  fprintf(f, "Lookups: %u\n", persistentCacheStats.cacheLookups);
  fprintf(f, "Hits: %u\n", persistentCacheStats.cacheHits);
  fprintf(f, "Misses: %u\n", persistentCacheStats.cacheMisses);
  fprintf(f, "Bytes saved: %llu\n", (unsigned long long)persistentCacheStats.bytesSaved);
  double hitPct = persistentCacheStats.cacheLookups > 0 ?
    (100.0 * persistentCacheStats.cacheHits / persistentCacheStats.cacheLookups) : 0.0;
  fprintf(f, "Hit rate: %.1f%%\n", hitPct);
  
  fprintf(f, "\n=== Client Known IDs (ServerHashSet) ===\n");
  auto hashStats = clientKnownIds_.getStats();
  fprintf(f, "Current size: %zu\n", hashStats.currentSize);
  fprintf(f, "Total added: %llu\n", (unsigned long long)hashStats.totalAdded);
  fprintf(f, "Total evicted: %llu\n", (unsigned long long)hashStats.totalEvicted);
  
  fprintf(f, "\n=== Update Statistics ===\n");
  fprintf(f, "Total updates: %u\n", updates);
  
  fprintf(f, "\n=== Region State ===\n");
  fprintf(f, "Lossy region: %s\n", strRegionSummary(lossyRegion));
  fprintf(f, "Recently changed region: %s\n", strRegionSummary(recentlyChangedRegion));
  fprintf(f, "Pending refresh region: %s\n", strRegionSummary(pendingRefreshRegion));
  fprintf(f, "Last framebuffer rect: [%d,%d-%d,%d]\n",
          lastFramebufferRect.tl.x, lastFramebufferRect.tl.y,
          lastFramebufferRect.br.x, lastFramebufferRect.br.y);
  
  fclose(f);
  vlog.info("Server cache state dumped to: %s", filepath.c_str());
}

bool EncodeManager::supported(int encoding)
{
  switch (encoding) {
  case encodingRaw:
  case encodingRRE:
  case encodingHextile:
  case encodingZRLE:
  case encodingTight:
    return true;
  default:
    return false;
  }
}

bool EncodeManager::needsLosslessRefresh(const core::Region& req)
{
  return !lossyRegion.intersect(req).is_empty();
}

int EncodeManager::getNextLosslessRefresh(const core::Region& req)
{
  // Do we have something we can send right away?
  if (!pendingRefreshRegion.intersect(req).is_empty())
    return 0;

  assert(needsLosslessRefresh(req));
  assert(recentChangeTimer.isStarted());

  return recentChangeTimer.getNextTimeout();
}

void EncodeManager::pruneLosslessRefresh(const core::Region& limits)
{
  lossyRegion.assign_intersect(limits);
  pendingRefreshRegion.assign_intersect(limits);
}

void EncodeManager::forceRefresh(const core::Region& req)
{
  lossyRegion.assign_union(req);
  if (!recentChangeTimer.isStarted())
    pendingRefreshRegion.assign_union(req);
}

void EncodeManager::writeUpdate(const UpdateInfo& ui, const PixelBuffer* pb,
                                const RenderedCursor* renderedCursor)
{
  doUpdate(true, ui.changed, ui.copied, ui.copy_delta, pb, renderedCursor);

  recentlyChangedRegion.assign_union(ui.changed);
  recentlyChangedRegion.assign_union(ui.copied);
  if (!recentChangeTimer.isStarted())
    recentChangeTimer.start(RecentChangeTimeout);
}

void EncodeManager::writeLosslessRefresh(const core::Region& req,
                                         const PixelBuffer* pb,
                                         const RenderedCursor* renderedCursor,
                                         size_t maxUpdateSize)
{
  doUpdate(false, getLosslessRefresh(req, maxUpdateSize),
           {}, {}, pb, renderedCursor);
}

void EncodeManager::handleTimeout(core::Timer* t)
{
  if (t == &recentChangeTimer) {
    // Any lossy region that wasn't recently updated can
    // now be scheduled for a refresh
    pendingRefreshRegion.assign_union(lossyRegion.subtract(recentlyChangedRegion));
    recentlyChangedRegion.clear();

    // Will there be more to do? (i.e. do we need another round)
    if (!lossyRegion.subtract(pendingRefreshRegion).is_empty())
      t->repeat();
  } else if (t == &cacheStatsTimer) {
    // Periodic cache statistics/logging hook kept for future use with
    // persistentCacheStats; currently a no-op beyond timer rescheduling.
    t->repeat();  // Continue hourly logging
  }
}

void EncodeManager::doUpdate(bool allowLossy, const
                             core::Region& changed_,
                             const core::Region& copied,
                             const core::Point& copyDelta,
                             const PixelBuffer* pb,
                             const RenderedCursor* renderedCursor)
{
    int nRects;
    core::Region changed, cursorRegion;

    // CC: Log update boundaries
    vlog.info("CC doUpdate begin: changed %s, copied %s, allowLossy=%s",
              strRegionSummary(changed_), strRegionSummary(copied), yesNo(allowLossy));

    updates++;

    // Check for resolution change; lastFramebufferRect is still tracked so
    // diagnostics and future policies can react, but there is no longer a
    // server-side ContentCache to clear.
    if (pb != nullptr) {
      core::Rect fbRect = pb->getRect();
      if (fbRect != lastFramebufferRect) {
        if (!lastFramebufferRect.is_empty()) {
          vlog.info("Framebuffer size changed from [%d,%d-%d,%d] to [%d,%d-%d,%d]",
                    lastFramebufferRect.tl.x, lastFramebufferRect.tl.y,
                    lastFramebufferRect.br.x, lastFramebufferRect.br.y,
                    fbRect.tl.x, fbRect.tl.y, fbRect.br.x, fbRect.br.y);
        }
        lastFramebufferRect = fbRect;
      }
    }

    prepareEncoders(allowLossy);
    
    // Track if this update will use lossy encoding (for seeding decisions)
    // If allowLossy is true and client supports Tight, assume lossy until proven otherwise
    currentEncodingIsLossy = allowLossy &&
                             conn->client.supportsEncoding(encodingTight);
    currentEncoding = conn->getPreferredEncoding();

    changed = changed_;

    // Optional experimental tiling analysis (log-only). This does not
    // change encoding behaviour; it just logs what portion of the
    // bounding dirty region could be satisfied purely from cache based
    // on current PersistentCache state.
    if (isCCTilingDebugEnabled() && pb != nullptr && !changed.is_empty()) {
      int tileSize = 128;
      if (const char* envTile = getenv("TIGERVNC_CC_TILE_SIZE")) {
        int v = atoi(envTile);
        if (v > 0)
          tileSize = v;
      }
      int minTiles = 4; // require at least a 2x2 region by default

      // Under the unified cache engine, both pseudoEncodingContentCache and
      // pseudoEncodingPersistentCache map to the same 64-bit ID protocol on
      // the wire. Tiling diagnostics should therefore run whenever the
      // client has negotiated *any* cache encoding and the server has
      // caching enabled.
      if (usePersistentCache &&
          (conn->client.supportsEncoding(pseudoEncodingPersistentCache) ||
           conn->client.supportsEncoding(pseudoEncodingContentCache))) {
        cache::PersistentCacheQuery pq(conn);
        cache::analyzeRegionTilingLogOnly(changed, tileSize, minTiles, pb, pq);
      }
    }

    if (!conn->client.supportsEncoding(encodingCopyRect))
      changed.assign_union(copied);

    /*
     * We need to render the cursor seperately as it has its own
     * magical pixel buffer, so split it out from the changed region.
     */
    if (renderedCursor != nullptr) {
      cursorRegion = changed.intersect(renderedCursor->getEffectiveRect());
      changed.assign_subtract(renderedCursor->getEffectiveRect());
    }

    if (conn->client.supportsEncoding(pseudoEncodingLastRect))
      nRects = 0xFFFF;
    else {
      nRects = 0;
      if (conn->client.supportsEncoding(encodingCopyRect))
        nRects += copied.numRects();
      nRects += computeNumRects(changed);
      nRects += computeNumRects(cursorRegion);
    }

    conn->writer()->writeFramebufferUpdateStart(nRects);

    // Allow disabling CopyRect via environment for diagnostics
    const char* disableCopyRect = getenv("TIGERVNC_DISABLE_COPYRECT");
    if (conn->client.supportsEncoding(encodingCopyRect) && !disableCopyRect)
      writeCopyRects(copied, copyDelta);

    /*
     * We start by searching for solid rects, which are then removed
     * from the changed region.
     */
    if (conn->client.supportsEncoding(pseudoEncodingLastRect))
      writeSolidRects(&changed, pb);

    writeRects(changed, pb);
    writeRects(cursorRegion, renderedCursor);

    // Respond to any pending CachedRectInit requests for this connection
    if (conn->client.supportsEncoding(pseudoEncodingLastRect)) {
      std::vector<std::pair<uint64_t, core::Rect>> pend;
      conn->drainPendingCachedInits(pend);
      if (!pend.empty()) {
        vlog.info("Processing %d pending CachedRectInit messages", (int)pend.size());
      }
      for (const auto& item : pend) {
          const uint64_t cacheId = item.first;
          const core::Rect& r = item.second;

          // Encode this rect now and send as CachedRectInit
          // Reuse the same selection logic as normal rectangles
          PixelBuffer *ppb;
          struct RectInfo info;
          EncoderType type;

          selectEncoderForRect(r, pb, ppb, &info, type);

          Encoder* payloadEnc = encoders[activeEncoders[type]];
          // Emit CachedRectInit header (cacheId + encoding)
          conn->writer()->writeCachedRectInit(r, cacheId, payloadEnc->encoding);
          // Prepare pixel buffer respecting native-PF usage for the payload encoder
          if (payloadEnc->flags & EncoderUseNativePF)
            ppb = preparePixelBuffer(r, pb, false);
          // Write the encoded pixel payload
          payloadEnc->writeRect(ppb, info.palette);
          // Close the CachedRectInit rectangle
          conn->writer()->endRect();
          // Mark this cacheId as known to this client
          conn->markCacheIdKnown(cacheId);
        }
      }

    conn->writer()->writeFramebufferUpdateEnd();
}

void EncodeManager::prepareEncoders(bool allowLossy)
{
  enum EncoderClass solid, bitmap, bitmapRLE;
  enum EncoderClass indexed, indexedRLE, fullColour;

  bool allowJPEG;

  int32_t preferred;

  std::vector<int>::iterator iter;

  solid = bitmap = bitmapRLE = encoderRaw;
  indexed = indexedRLE = fullColour = encoderRaw;

  // Strict lossless path: used for idle "lossless refresh" updates when
  // allowLossy == false. In this mode we never use JPEG at all and prefer
  // ZRLE for all rectangle types when the client supports it. This ensures
  // that previously JPEG-encoded regions converge to a bit-perfect copy of
  // the server framebuffer when we have spare bandwidth.
  if (!allowLossy) {
    if (encoders[encoderZRLE]->isSupported()) {
      solid = bitmap = bitmapRLE = indexed = indexedRLE = fullColour = encoderZRLE;
    } else if (encoders[encoderTight]->isSupported()) {
      solid = bitmap = bitmapRLE = indexed = indexedRLE = fullColour = encoderTight;
    } else if (encoders[encoderHextile]->isSupported()) {
      solid = bitmap = bitmapRLE = indexed = indexedRLE = fullColour = encoderHextile;
    } else {
      solid = bitmap = bitmapRLE = indexed = indexedRLE = fullColour = encoderRaw;
    }
  } else {
    allowJPEG = conn->client.pf().bpp >= 16;

    // Try to respect the client's wishes
    preferred = conn->getPreferredEncoding();
    switch (preferred) {
    case encodingRRE:
      // Horrible for anything high frequency and/or lots of colours
      bitmapRLE = indexedRLE = encoderRRE;
      break;
    case encodingHextile:
      // Slightly less horrible
      bitmapRLE = indexedRLE = fullColour = encoderHextile;
      break;
    case encodingTight:
      if (encoders[encoderTightJPEG]->isSupported() && allowJPEG)
        fullColour = encoderTightJPEG;
      else
        fullColour = encoderTight;
      indexed = indexedRLE = encoderTight;
      bitmap = bitmapRLE = encoderTight;
      break;
    case encodingZRLE:
      fullColour = encoderZRLE;
      bitmapRLE = indexedRLE = encoderZRLE;
      bitmap = indexed = encoderZRLE;
      break;
    }

    // Any encoders still unassigned?

    if (fullColour == encoderRaw) {
      if (encoders[encoderTightJPEG]->isSupported() && allowJPEG)
        fullColour = encoderTightJPEG;
      else if (encoders[encoderZRLE]->isSupported())
        fullColour = encoderZRLE;
      else if (encoders[encoderTight]->isSupported())
        fullColour = encoderTight;
      else if (encoders[encoderHextile]->isSupported())
        fullColour = encoderHextile;
    }

    if (indexed == encoderRaw) {
      if (encoders[encoderZRLE]->isSupported())
        indexed = encoderZRLE;
      else if (encoders[encoderTight]->isSupported())
        indexed = encoderTight;
      else if (encoders[encoderHextile]->isSupported())
        indexed = encoderHextile;
    }

    if (indexedRLE == encoderRaw)
      indexedRLE = indexed;

    if (bitmap == encoderRaw)
      bitmap = indexed;
    if (bitmapRLE == encoderRaw)
      bitmapRLE = bitmap;

    if (solid == encoderRaw) {
      if (encoders[encoderTight]->isSupported())
        solid = encoderTight;
      else if (encoders[encoderRRE]->isSupported())
        solid = encoderRRE;
      else if (encoders[encoderZRLE]->isSupported())
        solid = encoderZRLE;
      else if (encoders[encoderHextile]->isSupported())
        solid = encoderHextile;
    }

    // JPEG is the only encoder that can reduce things to grayscale
    if ((conn->client.subsampling == subsampleGray) &&
        encoders[encoderTightJPEG]->isSupported()) {
      solid = bitmap = bitmapRLE = encoderTightJPEG;
      indexed = indexedRLE = fullColour = encoderTightJPEG;
    }
  }

  activeEncoders[encoderSolid] = solid;
  activeEncoders[encoderBitmap] = bitmap;
  activeEncoders[encoderBitmapRLE] = bitmapRLE;
  activeEncoders[encoderIndexed] = indexed;
  activeEncoders[encoderIndexedRLE] = indexedRLE;
  activeEncoders[encoderFullColour] = fullColour;

  for (iter = activeEncoders.begin(); iter != activeEncoders.end(); ++iter) {
    Encoder *encoder;

    encoder = encoders[*iter];

    encoder->setCompressLevel(conn->client.compressLevel);

    if (allowLossy) {
      encoder->setQualityLevel(conn->client.qualityLevel);
      encoder->setFineQualityLevel(conn->client.fineQualityLevel,
                                   conn->client.subsampling);
    } else {
      // Lossless refresh path: ensure any encoder that supports a
      // "losslessQuality" level uses it, and disable fine-quality
      // overrides such as subsampling.
      if (encoder->losslessQuality != -1 &&
          conn->client.qualityLevel < encoder->losslessQuality)
        encoder->setQualityLevel(encoder->losslessQuality);
      else
        encoder->setQualityLevel(conn->client.qualityLevel);
      encoder->setFineQualityLevel(-1, subsampleUndefined);
    }
  }
}

core::Region EncodeManager::getLosslessRefresh(const core::Region& req,
                                               size_t maxUpdateSize)
{
  std::vector<core::Rect> rects;
  core::Region refresh;
  size_t area;

  // We make a conservative guess at the compression ratio at 2:1
  maxUpdateSize *= 2;

  // We will measure pixels, not bytes (assume 32 bpp)
  maxUpdateSize /= 4;

  area = 0;
  pendingRefreshRegion.intersect(req).get_rects(&rects);
  while (!rects.empty()) {
    size_t idx;
    core::Rect rect;

    // Grab a random rect so we don't keep damaging and restoring the
    // same rect over and over
    idx = rand() % rects.size();

    rect = rects[idx];

    // Add rects until we exceed the threshold, then include as much as
    // possible of the final rect
    if ((area + rect.area()) > maxUpdateSize) {
      // Use the narrowest axis to avoid getting to thin rects
      if (rect.width() > rect.height()) {
        int width = (maxUpdateSize - area) / rect.height();
        if (width < 1)
          width = 1;
        rect.br.x = rect.tl.x + width;
      } else {
        int height = (maxUpdateSize - area) / rect.width();
        if (height < 1)
          height = 1;
        rect.br.y = rect.tl.y + height;
      }
      refresh.assign_union(rect);
      break;
    }

    area += rect.area();
    refresh.assign_union(rect);

    rects.erase(rects.begin() + idx);
  }

  return refresh;
}

int EncodeManager::computeNumRects(const core::Region& changed)
{
  int numRects;
  std::vector<core::Rect> rects;
  std::vector<core::Rect>::const_iterator rect;

  numRects = 0;
  changed.get_rects(&rects);
  for (rect = rects.begin(); rect != rects.end(); ++rect) {
    int w, h, sw, sh;

    w = rect->width();
    h = rect->height();

    // No split necessary?
    if (((w*h) < SubRectMaxArea) && (w < SubRectMaxWidth)) {
      numRects += 1;
      continue;
    }

    if (w <= SubRectMaxWidth)
      sw = w;
    else
      sw = SubRectMaxWidth;

    sh = SubRectMaxArea / sw;

    // ceil(w/sw) * ceil(h/sh)
    numRects += (((w - 1)/sw) + 1) * (((h - 1)/sh) + 1);
  }

  return numRects;
}

Encoder* EncodeManager::startRect(const core::Rect& rect, int type)
{
  Encoder *encoder;
  int klass, equiv;

  activeType = type;
  klass = activeEncoders[activeType];

  beforeLength = conn->getOutStream()->length();

  stats[klass][activeType].rects++;
  stats[klass][activeType].pixels += rect.area();
  equiv = 12 + rect.area() * (conn->client.pf().bpp/8);
  stats[klass][activeType].equivalent += equiv;

  encoder = encoders[klass];
  conn->writer()->startRect(rect, encoder->encoding);

  if ((encoder->flags & EncoderLossy) &&
      ((encoder->losslessQuality == -1) ||
       (encoder->getQualityLevel() < encoder->losslessQuality)))
    lossyRegion.assign_union(rect);
  else
    lossyRegion.assign_subtract(rect);

  // This was either a rect getting refreshed, or a rect that just got
  // new content. Either way we should not try to refresh it anymore.
  pendingRefreshRegion.assign_subtract(rect);

  return encoder;
}

void EncodeManager::endRect()
{
  int klass;
  int length;

  conn->writer()->endRect();

  length = conn->getOutStream()->length() - beforeLength;

  klass = activeEncoders[activeType];
  stats[klass][activeType].bytes += length;
}

void EncodeManager::writeCopyRects(const core::Region& copied,
                                   const core::Point& delta)
{
  std::vector<core::Rect> rects;
  std::vector<core::Rect>::const_iterator rect;

  core::Region lossyCopy;

  beforeLength = conn->getOutStream()->length();

  copied.get_rects(&rects, delta.x <= 0, delta.y <= 0);
  for (rect = rects.begin(); rect != rects.end(); ++rect) {
    int equiv;

    copyStats.rects++;
    copyStats.pixels += rect->area();
    equiv = 12 + rect->area() * (conn->client.pf().bpp/8);
    copyStats.equivalent += equiv;

    conn->writer()->writeCopyRect(*rect, rect->tl.x - delta.x,
                                   rect->tl.y - delta.y);
  }

  copyStats.bytes += conn->getOutStream()->length() - beforeLength;

  lossyCopy = lossyRegion;
  lossyCopy.translate(delta);
  lossyCopy.assign_intersect(copied);
  lossyRegion.assign_union(lossyCopy);

  // Stop any pending refresh as a copy is enough that we consider
  // this region to be recently changed
  pendingRefreshRegion.assign_subtract(copied);
}

void EncodeManager::writeSolidRects(core::Region* changed,
                                    const PixelBuffer* pb)
{
  std::vector<core::Rect> rects;
  std::vector<core::Rect>::const_iterator rect;

  changed->get_rects(&rects);
  for (rect = rects.begin(); rect != rects.end(); ++rect)
    findSolidRect(*rect, changed, pb);
}

void EncodeManager::findSolidRect(const core::Rect& rect,
                                  core::Region* changed,
                                  const PixelBuffer* pb)
{
  core::Rect sr;
  int dx, dy, dw, dh;

  // We start by finding a solid 16x16 block
  for (dy = rect.tl.y; dy < rect.br.y; dy += SolidSearchBlock) {

    dh = SolidSearchBlock;
    if (dy + dh > rect.br.y)
      dh = rect.br.y - dy;

    for (dx = rect.tl.x; dx < rect.br.x; dx += SolidSearchBlock) {
      // We define it like this to guarantee alignment
      uint32_t _buffer;
      uint8_t* colourValue = (uint8_t*)&_buffer;

      dw = SolidSearchBlock;
      if (dx + dw > rect.br.x)
        dw = rect.br.x - dx;

      pb->getImage(colourValue, {dx, dy, dx+1, dy+1});

      sr.setXYWH(dx, dy, dw, dh);
      if (checkSolidTile(sr, colourValue, pb)) {
        core::Rect erb, erp;

        Encoder *encoder;

        // We then try extending the area by adding more blocks
        // in both directions and pick the combination that gives
        // the largest area.
        sr.setXYWH(dx, dy, rect.br.x - dx, rect.br.y - dy);
        extendSolidAreaByBlock(sr, colourValue, pb, &erb);

        // Did we end up getting the entire rectangle?
        if (erb == rect)
          erp = erb;
        else {
          // Don't bother with sending tiny rectangles
          if (erb.area() < SolidBlockMinArea)
            continue;

          // Extend the area again, but this time one pixel
          // row/column at a time.
          extendSolidAreaByPixel(rect, erb, colourValue, pb, &erp);
        }

        // Send solid-color rectangle.
        encoder = startRect(erp, encoderSolid);
        if (encoder->flags & EncoderUseNativePF) {
          encoder->writeSolidRect(erp.width(), erp.height(),
                                  pb->getPF(), colourValue);
        } else {
          uint32_t _buffer2;
          uint8_t* converted = (uint8_t*)&_buffer2;

          conn->client.pf().bufferFromBuffer(converted, pb->getPF(),
                                         colourValue, 1);

          encoder->writeSolidRect(erp.width(), erp.height(),
                                  conn->client.pf(), converted);
        }
        endRect();

        changed->assign_subtract(erp);

        // Search remaining areas by recursion
        // FIXME: Is this the best way to divide things up?

        // Left? (Note that we've already searched a SolidSearchBlock
        //        pixels high strip here)
        if ((erp.tl.x != rect.tl.x) && (erp.height() > SolidSearchBlock)) {
          sr.setXYWH(rect.tl.x, erp.tl.y + SolidSearchBlock,
                     erp.tl.x - rect.tl.x, erp.height() - SolidSearchBlock);
          findSolidRect(sr, changed, pb);
        }

        // Right?
        if (erp.br.x != rect.br.x) {
          sr.setXYWH(erp.br.x, erp.tl.y, rect.br.x - erp.br.x, erp.height());
          findSolidRect(sr, changed, pb);
        }

        // Below?
        if (erp.br.y != rect.br.y) {
          sr.setXYWH(rect.tl.x, erp.br.y, rect.width(), rect.br.y - erp.br.y);
          findSolidRect(sr, changed, pb);
        }

        return;
      }
    }
  }
}

// Minimum rectangle area (in pixels) to attempt whole-rectangle cache lookup.
// Rectangles smaller than this will be handled by the normal subrect path.
// Set higher than the subrect split threshold to only catch "large" rects.
static const int WholeRectCacheMinArea = 10000;

void EncodeManager::writeRects(const core::Region& changed,
                               const PixelBuffer* pb)
{
  std::vector<core::Rect> rects;
  std::vector<core::Rect>::const_iterator rect;

  if (isCCDebugEnabled()) {
    vlog.info("CCDBG writeRects: region has %d rects", changed.numRects());
  }

  // Unified cache protocol: we may see either the PersistentCache pseudo-encoding
  // (-321) or the legacy ContentCache pseudo-encoding (-320). In this fork they
  // both map to the same 64-bit ID wire format; the difference is viewer policy.
  bool clientSupportsCache =
    conn->client.supportsEncoding(pseudoEncodingPersistentCache) ||
    conn->client.supportsEncoding(pseudoEncodingContentCache);

  // Work on a mutable copy of the damage so cache hits can remove regions that
  // are fully satisfied by references while still allowing other dirty areas in
  // the same update to be encoded normally.
  core::Region work = changed;

  // Extra diagnostics for the problematic top-of-screen band. When the
  // damage bounding box intersects y in ~[20,100), log a concise summary
  // of this update, including whether the client supports cache.
  core::Rect damageBbox = changed.get_bounding_rect();
  bool topBandUpdate = !damageBbox.is_empty() &&
                       damageBbox.tl.y < 100 && damageBbox.br.y > 20;
  if (topBandUpdate) {
    vlog.info("PCSRV TOPBAND_UPDATE: conn=%p supportsCache=%s bbox=[%d,%d-%d,%d]", 
              (void*)conn,
              yesNo(clientSupportsCache),
              damageBbox.tl.x, damageBbox.tl.y,
              damageBbox.br.x, damageBbox.br.y);
  }

  // TILING ENHANCEMENT: Before processing individual damage rects, detect
  // bordered content regions in the framebuffer. These are rectangular areas
  // surrounded by solid-colored borders, which typically represent:
  // - Slide content areas in presentation software
  // - Document editing areas
  // - Image/video viewing areas embedded in an application UI
  //
  // By detecting these regions, we can cache them independently of the
  // fragmented damage regions reported by X11.
  
  std::vector<ContentHash::BorderedRegion> borderedRegions;
  // Bordered region detection is expensive, so only run it when:
  // 1. Client supports cache
  // 2. The damage region is large enough to potentially be a content area change
  // (Note: We always try detection to seed the perceptual hash index on first pass)
  bool shouldDetectBorderedRegions = clientSupportsCache && 
                                      !changed.is_empty() &&
                                      changed.get_bounding_rect().area() > 10000;
  
  if (shouldDetectBorderedRegions) {
    vlog.info("BORDERED: Attempting detection on %dx%d framebuffer (damage bbox area=%d)",
              pb->width(), pb->height(), damageBbox.area());
    
    // Detect bordered regions in the full framebuffer
    borderedRegions = ContentHash::detectBorderedRegions(pb, 5, 50000);
    
    if (!borderedRegions.empty()) {
      vlog.info("BORDERED: Detected %d bordered regions", (int)borderedRegions.size());
      for (const auto& r : borderedRegions) {
        vlog.info("BORDERED:   Region [%d,%d-%d,%d] border=%d/%d/%d/%d",
                  r.contentRect.tl.x, r.contentRect.tl.y,
                  r.contentRect.br.x, r.contentRect.br.y,
                  r.borderTop, r.borderBottom, r.borderLeft, r.borderRight);
      }
    } else {
      vlog.info("BORDERED: No regions detected (minBorder=5, minArea=50000)");
    }
  }
  
  if (!borderedRegions.empty()) {
    // Check if any detected regions match cached content
    for (const auto& region : borderedRegions) {
      const core::Rect& contentRect = region.contentRect;
      
      // Only process if the remaining damage intersects this content area
      core::Region damageInContent = work.intersect(contentRect);
      if (damageInContent.is_empty()) {
        continue;
      }

      // Compute content hash for this bordered region first, so we can check
      // if we've already seeded it before applying coverage heuristics.
      std::vector<uint8_t> contentHash = ContentHash::computeRect(pb, contentRect);
      uint64_t contentId = 0;
      if (!contentHash.empty()) {
        size_t n = std::min(contentHash.size(), sizeof(uint64_t));
        memcpy(&contentId, contentHash.data(), n);
      }
      
      // DEBUG: Log pixel sample to help diagnose hash collisions
      // Sample a few pixels from corners and center to verify framebuffer content
      {
        int stride = 0;
        const uint8_t* buf = pb->getBuffer(contentRect, &stride);
        int bpp = pb->getPF().bpp / 8;
        if (buf && stride > 0 && bpp > 0) {
          // Sample top-left, top-right, center, bottom-left pixels
          int w = contentRect.width();
          int h = contentRect.height();
          uint32_t tlPixel = 0, trPixel = 0, cPixel = 0, blPixel = 0;
          if (bpp == 4) {
            tlPixel = *(uint32_t*)(buf);
            trPixel = *(uint32_t*)(buf + (w-1)*bpp);
            cPixel = *(uint32_t*)(buf + (h/2)*stride*bpp + (w/2)*bpp);
            blPixel = *(uint32_t*)(buf + (h-1)*stride*bpp);
          }
          vlog.info("%s BORDERED: Hash debug [%d,%d-%d,%d] id=%s pixels: TL=%08x TR=%08x C=%08x BL=%08x",
                    strTimestamp(),
                    contentRect.tl.x, contentRect.tl.y,
                    contentRect.br.x, contentRect.br.y,
                    hex64(contentId), tlPixel, trPixel, cPixel, blPixel);
        }
      }

      // For large bordered regions (e.g. slide canvases or document panes),
      // avoid taking an optimistic cache hit when only a tiny fraction of the
      // region has changed. In highly dynamic UIs this can otherwise cause the
      // viewer to display a cached full-screen rect even though a non-trivial
      // sub-rect has new content.
      //
      // SAFETY: We now ALWAYS apply the coverage check for large bordered regions.
      // Even if the hash is "known", a low-coverage HIT can cause corruption if
      // there's a hash collision or framebuffer race condition. The small
      // performance cost of re-encoding is worth the visual correctness.
      const int contentArea = contentRect.area();
      bool alreadyKnown = conn->knowsPersistentId(contentId);
      
      const core::Rect damageBboxInContent = damageInContent.get_bounding_rect();
      const int damageAreaInContent = damageBboxInContent.area();
      const double coverage = static_cast<double>(damageAreaInContent) /
                              static_cast<double>(contentArea);
      
      // For very large bordered regions with low coverage, skip cache lookup
      // to avoid potential visual corruption from hash collisions or races
      if (contentArea > WholeRectCacheMinArea && coverage < 0.5) {
        vlog.info("%s BORDERED: Skipping cache lookup for [%d,%d-%d,%d] due to low damage coverage (%.3f)%s",
                  strTimestamp(),
                  contentRect.tl.x, contentRect.tl.y,
                  contentRect.br.x, contentRect.br.y, coverage,
                  alreadyKnown ? " - hash known but coverage too low" : "");
        continue;
      }

      // Debug: record the canonical bytes used for this content region.
      logFBHashDebug("bordered", contentRect, contentId, pb);
      
      // Decide if we can safely send a reference. We only do so when the
      // connection believes the viewer can satisfy this canonical ID.
      bool clientRequested = conn->clientRequestedPersistent(contentId);
      bool hasMatch = alreadyKnown && !clientRequested;
      uint64_t matchedId = contentId;  // Always canonical

      // Count this bordered-region attempt as a cache lookup.
      persistentCacheStats.cacheLookups++;

      if (hasMatch) {
        // CACHE HIT on bordered content region!
        persistentCacheStats.cacheHits++;
        int equiv = 12 + contentRect.area() * (conn->client.pf().bpp / 8);
        persistentCacheStats.bytesSaved += equiv - 20;
        copyStats.rects++;
        copyStats.pixels += contentRect.area();
        copyStats.equivalent += equiv;
        beforeLength = conn->getOutStream()->length();
        conn->writer()->writePersistentCachedRect(contentRect, matchedId);
        copyStats.bytes += conn->getOutStream()->length() - beforeLength;

        vlog.info("%s BORDERED: Cache HIT for content region [%d,%d-%d,%d] id=%s cov=%.3f",
                  strTimestamp(),
                  contentRect.tl.x, contentRect.tl.y,
                  contentRect.br.x, contentRect.br.y, hex64(matchedId), coverage);

        conn->onCachedRectRef(matchedId, contentRect);
        
        // Only clear lossy tracking if client has lossless content.
        // If there's a lossy hash mapping for this canonical ID, the client
        // might only have lossy pixels, so keep in lossyRegion for refresh.
        uint64_t lossyIdUnused = 0;
        bool hasLossyMapping = conn->hasLossyHash(contentId, lossyIdUnused);
        if (!hasLossyMapping) {
          lossyRegion.assign_subtract(contentRect);
        }
        pendingRefreshRegion.assign_subtract(contentRect);

        // This reference satisfies the entire bordered region, but we might still
        // have other dirty areas outside it in this same update.
        work.assign_subtract(core::Region(contentRect));
        continue;
      }

      // Miss - will seed after encoding
      vlog.info("%s BORDERED: Content region [%d,%d-%d,%d] id=%s - will seed (cov=%.3f)",
                strTimestamp(),
                contentRect.tl.x, contentRect.tl.y,
                contentRect.br.x, contentRect.br.y,
                hex64(contentId), coverage);
      persistentCacheStats.cacheMisses++;
    }
  }
  
  // TILING ENHANCEMENT: Also check if the BOUNDING BOX of the changed region
  // matches a known cached rectangle.
  
  if (clientSupportsCache && rfb::Server::enableBBoxCache && !work.is_empty()) {
    core::Rect bbox = work.get_bounding_rect();
    int bboxArea = bbox.area();

    bool bboxTopBand = (bbox.tl.y < 100 && bbox.br.y > 20);

    if (bboxArea >= WholeRectCacheMinArea) {
      // For very large bounding boxes, avoid taking a cache hit when only a
      // small fraction of the bbox is damaged. This is particularly important
      // in mixed-content UIs where unrelated widgets (clocks, thumbnails, etc.)
      // can extend the damage region beyond the "true" content area.
      //
      // We still rely on the content hash comparison for correctness, but this
      // heuristic prevents us from constantly attempting full-bbox hits in
      // cases where the majority of the content is evolving frame-to-frame.
      // Estimate how much of the bbox is actually involved in this update
      // using the area of the damage bounding box.
      bool attemptBboxHit = true;
      const core::Rect damageBboxForCoverage = changed.get_bounding_rect();
      const int damageArea = damageBboxForCoverage.area();
      if (damageArea > 0) {
        const double bboxCoverage = static_cast<double>(damageArea) /
                                    static_cast<double>(bboxArea);
        if (bboxCoverage < 0.5) {
          attemptBboxHit = false;
          if (isCCDebugEnabled()) {
            vlog.info("TILING: Skipping bbox cache lookup for [%d,%d-%d,%d] due to low damage coverage (%.3f)",
                      bbox.tl.x, bbox.tl.y, bbox.br.x, bbox.br.y, bboxCoverage);
          }
        }
      }

      if (attemptBboxHit) {
        // Compute content hash for the entire bounding box
        std::vector<uint8_t> bboxHash = ContentHash::computeRect(pb, bbox);
        uint64_t bboxId = 0;
        if (!bboxHash.empty()) {
          size_t n = std::min(bboxHash.size(), sizeof(uint64_t));
          memcpy(&bboxId, bboxHash.data(), n);
        }

        // Debug: log canonical bytes for the bounding box domain.
        logFBHashDebug("bbox", bbox, bboxId, pb);

        // Decide if we can safely send a reference for this bounding box.
        bool clientRequested = conn->clientRequestedPersistent(bboxId);
        bool hasHit = conn->knowsPersistentId(bboxId) && !clientRequested;
        uint64_t matchedBboxId = bboxId;  // Always canonical

        // Count this bbox attempt as a cache lookup.
        persistentCacheStats.cacheLookups++;

        if (hasHit) {
          // CACHE HIT on bounding box!
          persistentCacheStats.cacheHits++;

          if (bboxTopBand) {
            vlog.info("PCSRV TOPBAND_BBOX_HIT: conn=%p bbox=[%d,%d-%d,%d] id=%s",
                      (void*)conn,
                      bbox.tl.x, bbox.tl.y, bbox.br.x, bbox.br.y,
                      hex64(matchedBboxId));
          }

          int equiv = 12 + bboxArea * (conn->client.pf().bpp / 8);
          persistentCacheStats.bytesSaved += equiv - 20;
          copyStats.rects++;
          copyStats.pixels += bboxArea;
          copyStats.equivalent += equiv;
          beforeLength = conn->getOutStream()->length();
          conn->writer()->writePersistentCachedRect(bbox, matchedBboxId);
          copyStats.bytes += conn->getOutStream()->length() - beforeLength;

          vlog.info("TILING: Bounding-box cache HIT [%d,%d-%d,%d] id=%s (saved %d bytes, %d damage rects coalesced)",
                    bbox.tl.x, bbox.tl.y, bbox.br.x, bbox.br.y,
                    hex64(matchedBboxId), equiv - 20, changed.numRects());

          conn->onCachedRectRef(matchedBboxId, bbox);
          
          // Only clear lossy tracking if client has lossless content.
          // If there's a lossy hash mapping for this canonical ID, the client
          // might only have lossy pixels, so keep in lossyRegion for refresh.
          uint64_t lossyBboxIdUnused = 0;
          bool hasLossyBboxMapping = conn->hasLossyHash(bboxId, lossyBboxIdUnused);
          if (!hasLossyBboxMapping) {
            lossyRegion.assign_subtract(bbox);
          }
          pendingRefreshRegion.assign_subtract(bbox);
          return;  // Entire region handled by one cache hit!
        }

        // Bounding box not in cache - we'll seed it after encoding
        vlog.info("TILING: Bounding-box cache MISS [%d,%d-%d,%d] id=%s - will seed after encoding %d rects",
                  bbox.tl.x, bbox.tl.y, bbox.br.x, bbox.br.y,
                  hex64(bboxId), changed.numRects());

        persistentCacheStats.cacheMisses++;
      }
    }
  }
  
  // Track bounding box for seeding after encoding individual rects
  core::Rect bboxForSeeding;
  uint64_t bboxIdForSeeding = 0;
  bool shouldSeedBbox = false;
  
  if (clientSupportsCache && !changed.is_empty()) {
    core::Rect bbox = changed.get_bounding_rect();
    if (bbox.area() >= WholeRectCacheMinArea) {
      std::vector<uint8_t> bboxHash = ContentHash::computeRect(pb, bbox);
      if (!bboxHash.empty()) {
        size_t n = std::min(bboxHash.size(), sizeof(uint64_t));
        memcpy(&bboxIdForSeeding, bboxHash.data(), n);
        bboxForSeeding = bbox;
        shouldSeedBbox = !conn->knowsPersistentId(bboxIdForSeeding);

        // Debug: log canonical bytes for the seeding domain.
        logFBHashDebug("bboxSeed", bboxForSeeding, bboxIdForSeeding, pb);

        if (!bboxForSeeding.is_empty() &&
            bboxForSeeding.tl.y < 100 && bboxForSeeding.br.y > 20) {
          vlog.info("PCSRV TOPBAND_BBOX_SEED: conn=%p bbox=[%d,%d-%d,%d] id=%s",
                    (void*)conn,
                    bboxForSeeding.tl.x, bboxForSeeding.tl.y,
                    bboxForSeeding.br.x, bboxForSeeding.br.y,
                    hex64(bboxIdForSeeding));
        }
      }
    }
  }

  work.get_rects(&rects);
  for (rect = rects.begin(); rect != rects.end(); ++rect) {
    int w, h, sw, sh;
    core::Rect sr;

    w = rect->width();
    h = rect->height();

    // No split necessary?
    if (((w*h) < SubRectMaxArea) && (w < SubRectMaxWidth)) {
      vlog.debug("CC rect no-split: (%s) area=%d", strRect(*rect), w*h);
      writeSubRect(*rect, pb);
      continue;
    }

    if (w <= SubRectMaxWidth)
      sw = w;
    else
      sw = SubRectMaxWidth;

    sh = SubRectMaxArea / sw;

    vlog.debug("CC rect split: parent (%s) tileSize=%dx%d", strRect(*rect), sw, sh);

    for (sr.tl.y = rect->tl.y; sr.tl.y < rect->br.y; sr.tl.y += sh) {
      sr.br.y = sr.tl.y + sh;
      if (sr.br.y > rect->br.y)
        sr.br.y = rect->br.y;

      for (sr.tl.x = rect->tl.x; sr.tl.x < rect->br.x; sr.tl.x += sw) {
        sr.br.x = sr.tl.x + sw;
        if (sr.br.x > rect->br.x)
          sr.br.x = rect->br.x;

        vlog.debug("CC subrect: (%s) from parent (%s)", strRect(sr), strRect(*rect));
        writeSubRect(sr, pb);
      }
    }
  }
  
  // TILING ENHANCEMENT: Seed the bounding box hash after encoding all damage rects.
  // Always seed with canonical hash. For lossy encodings, the client will compute
  // the actual lossy hash after decode, detect the mismatch, and report back via
  // message 247 (PersistentCacheHashReport). This enables first-occurrence cache
  // hits for large lossy rectangles without requiring server-side encode/decode.
  if (shouldSeedBbox) {
    // Seed with canonical hash regardless of encoding
    conn->writer()->writeCachedRectSeed(bboxForSeeding, bboxIdForSeeding);
    conn->markPersistentIdKnown(bboxIdForSeeding);
    
    vlog.info("TILING: Seeded bounding-box hash [%d,%d-%d,%d] id=%s (client will report lossy hash if needed)",
              bboxForSeeding.tl.x, bboxForSeeding.tl.y,
              bboxForSeeding.br.x, bboxForSeeding.br.y,
              hex64(bboxIdForSeeding));
  }
  
  // BORDERED REGION SEEDING: Seed any detected bordered content regions
  // so that future identical content can be served from cache.
  // Always seed with canonical hash. For lossy encodings, the client will compute
  // the actual lossy hash after decode and report back via message 247.
  for (const auto& region : borderedRegions) {
    const core::Rect& contentRect = region.contentRect;
    
    // Compute content hash
    std::vector<uint8_t> contentHash = ContentHash::computeRect(pb, contentRect);
    uint64_t contentId = 0;
    if (!contentHash.empty()) {
      size_t n = std::min(contentHash.size(), sizeof(uint64_t));
      memcpy(&contentId, contentHash.data(), n);
    }

    // Debug: log canonical bytes for bordered region seeds.
    logFBHashDebug("borderedSeed", contentRect, contentId, pb);
    
    // Only seed if not already known
    if (!conn->knowsPersistentId(contentId)) {
      conn->writer()->writeCachedRectSeed(contentRect, contentId);
      conn->markPersistentIdKnown(contentId);
      
      vlog.info("BORDERED: Seeded content region [%d,%d-%d,%d] id=%s (client will report lossy hash if needed)",
                contentRect.tl.x, contentRect.tl.y,
                contentRect.br.x, contentRect.br.y,
                hex64(contentId));
    }
  }
}

void EncodeManager::writeSubRect(const core::Rect& rect,
                                 const PixelBuffer* pb)
{
  // Cache protocol selection: Use at most one cache per connection.
  // PersistentCache (64-bit ID protocol) is the only remaining cache
  // engine on the server side; the legacy ContentCache pseudo-encoding
  // now aliases the same engine via viewer/server policy.
  
  bool clientSupportsUnifiedCache =
    conn->client.supportsEncoding(pseudoEncodingPersistentCache);
  
  if (isCCDebugEnabled()) {
    vlog.info("CCDBG writeSubRect: rect=%s area=%d clientCache=%s",
              strRect(rect), rect.area(),
              yesNo(clientSupportsUnifiedCache));
  }
  
  if (usePersistentCache && clientSupportsUnifiedCache) {
    // Use unified cache protocol whenever the client has negotiated the
    // PersistentCache encoding.
    vlog.debug("CC attempt unified cache lookup for rect (%s)", strRect(rect));

    bool topBandRect = (rect.tl.y < 100 && rect.br.y > 20);
    if (topBandRect) {
      vlog.info("PCSRV TOPBAND_SUBRECT_LOOKUP: conn=%p rect=[%d,%d-%d,%d]",
                (void*)conn,
                rect.tl.x, rect.tl.y, rect.br.x, rect.br.y);
    }

    if (tryPersistentCacheLookup(rect, pb))
      return;
  }

  PixelBuffer *ppb;
  Encoder *encoder;
  struct RectInfo info;
  EncoderType type;

  // Shared encoder selection for normal and cache INIT paths
  selectEncoderForRect(rect, pb, ppb, &info, type);

  // Normal rectangle path
  encoder = startRect(rect, type);

  if (isCCDebugEnabled()) {
    vlog.info("CCDBG ENCODER: rect=%s enc=%d", strRect(rect), encoder->encoding);
  }

  if (encoder->flags & EncoderUseNativePF)
    ppb = preparePixelBuffer(rect, pb, false);

  encoder->writeRect(ppb, info.palette);
 
  endRect();
 
  // EXTRA DEBUG: For subrects inside the known problematic region, log a
  // content hash of the raw pixels we just encoded so we can compare
  // across viewers.
  core::Rect problemRegion(100, 100, 586, 443); // [100,100-586,443]
  core::Rect r = rect;
  bool inProblemRegion = !problemRegion.intersect(r).is_empty();
  uint64_t payloadHash64 = 0;
  if (isCCDebugEnabled() && inProblemRegion) {
    std::vector<uint8_t> payloadHash = ContentHash::computeRect(ppb, rect);
    if (payloadHash.size() >= 8) {
      memcpy(&payloadHash64, payloadHash.data(), 8);
    }
    vlog.info("CCDBG PAYLOAD: conn=%p rect=%s enc=%d hash64=%s",
              (void*)conn,
              strRect(rect),
              encoder->encoding,
              hex64(payloadHash64));
  }
 
  // Unified cache protocol: no server-side insertion path remains here; the
  // PersistentCache engine tracks client-known IDs via INIT messages on both
  // server and client.

  if (isCCDebugEnabled() && inProblemRegion) {
    vlog.info("CCDBG SERVER PATH: conn=%p rect=%s enc=%d path=%s hash64=%s",
              (void*)conn,
              strRect(rect),
              encoder->encoding,
              "NORMAL",
              hex64(payloadHash64));
  }
}

bool EncodeManager::checkSolidTile(const core::Rect& r,
                                   const uint8_t* colourValue,
                                   const PixelBuffer *pb)
{
  const uint8_t* buffer;
  int stride;

  buffer = pb->getBuffer(r, &stride);

  switch (pb->getPF().bpp) {
  case 32:
    return checkSolidTile(r.width(), r.height(),
                          (const uint32_t*)buffer, stride,
                          *(const uint32_t*)colourValue);
  case 16:
    return checkSolidTile(r.width(), r.height(),
                          (const uint16_t*)buffer, stride,
                          *(const uint16_t*)colourValue);
  default:
    return checkSolidTile(r.width(), r.height(),
                          (const uint8_t*)buffer, stride,
                          *(const uint8_t*)colourValue);
  }
}

void EncodeManager::extendSolidAreaByBlock(const core::Rect& r,
                                           const uint8_t* colourValue,
                                           const PixelBuffer* pb,
                                           core::Rect* er)
{
  int dx, dy, dw, dh;
  int w_prev;
  core::Rect sr;
  int w_best = 0, h_best = 0;

  w_prev = r.width();

  // We search width first, back off when we hit a different colour,
  // and restart with a larger height. We keep track of the
  // width/height combination that gives us the largest area.
  for (dy = r.tl.y; dy < r.br.y; dy += SolidSearchBlock) {

    dh = SolidSearchBlock;
    if (dy + dh > r.br.y)
      dh = r.br.y - dy;

    // We test one block here outside the x loop in order to break
    // the y loop right away.
    dw = SolidSearchBlock;
    if (dw > w_prev)
      dw = w_prev;

    sr.setXYWH(r.tl.x, dy, dw, dh);
    if (!checkSolidTile(sr, colourValue, pb))
      break;

    for (dx = r.tl.x + dw; dx < r.tl.x + w_prev;) {

      dw = SolidSearchBlock;
      if (dx + dw > r.tl.x + w_prev)
        dw = r.tl.x + w_prev - dx;

      sr.setXYWH(dx, dy, dw, dh);
      if (!checkSolidTile(sr, colourValue, pb))
        break;

      dx += dw;
    }

    w_prev = dx - r.tl.x;
    if (w_prev * (dy + dh - r.tl.y) > w_best * h_best) {
      w_best = w_prev;
      h_best = dy + dh - r.tl.y;
    }
  }

  er->tl.x = r.tl.x;
  er->tl.y = r.tl.y;
  er->br.x = er->tl.x + w_best;
  er->br.y = er->tl.y + h_best;
}

void EncodeManager::extendSolidAreaByPixel(const core::Rect& r,
                                           const core::Rect& sr,
                                           const uint8_t* colourValue,
                                           const PixelBuffer* pb,
                                           core::Rect* er)
{
  int cx, cy;
  core::Rect tr;

  // Try to extend the area upwards.
  for (cy = sr.tl.y - 1; cy >= r.tl.y; cy--) {
    tr.setXYWH(sr.tl.x, cy, sr.width(), 1);
    if (!checkSolidTile(tr, colourValue, pb))
      break;
  }
  er->tl.y = cy + 1;

  // ... downwards.
  for (cy = sr.br.y; cy < r.br.y; cy++) {
    tr.setXYWH(sr.tl.x, cy, sr.width(), 1);
    if (!checkSolidTile(tr, colourValue, pb))
      break;
  }
  er->br.y = cy;

  // ... to the left.
  for (cx = sr.tl.x - 1; cx >= r.tl.x; cx--) {
    tr.setXYWH(cx, er->tl.y, 1, er->height());
    if (!checkSolidTile(tr, colourValue, pb))
      break;
  }
  er->tl.x = cx + 1;

  // ... to the right.
  for (cx = sr.br.x; cx < r.br.x; cx++) {
    tr.setXYWH(cx, er->tl.y, 1, er->height());
    if (!checkSolidTile(tr, colourValue, pb))
      break;
  }
  er->br.x = cx;
}

PixelBuffer* EncodeManager::preparePixelBuffer(const core::Rect& rect,
                                               const PixelBuffer *pb,
                                               bool convert)
{
  const uint8_t* buffer;
  int stride;

  // Do wo need to convert the data?
  if (convert && conn->client.pf() != pb->getPF()) {
    convertedPixelBuffer.setPF(conn->client.pf());
    convertedPixelBuffer.setSize(rect.width(), rect.height());

    buffer = pb->getBuffer(rect, &stride);
    convertedPixelBuffer.imageRect(pb->getPF(),
                                   convertedPixelBuffer.getRect(),
                                   buffer, stride);

    return &convertedPixelBuffer;
  }

  // Otherwise we still need to shift the coordinates. We have our own
  // abusive subclass of FullFramePixelBuffer for this.

  buffer = pb->getBuffer(rect, &stride);

  offsetPixelBuffer.update(pb->getPF(), rect.width(), rect.height(),
                           buffer, stride);

  return &offsetPixelBuffer;
}

bool EncodeManager::analyseRect(const PixelBuffer *pb,
                                struct RectInfo *info, int maxColours)
{
  const uint8_t* buffer;
  int stride;

  buffer = pb->getBuffer(pb->getRect(), &stride);

  switch (pb->getPF().bpp) {
  case 32:
    return analyseRect(pb->width(), pb->height(),
                       (const uint32_t*)buffer, stride,
                       info, maxColours);
  case 16:
    return analyseRect(pb->width(), pb->height(),
                       (const uint16_t*)buffer, stride,
                       info, maxColours);
  default:
    return analyseRect(pb->width(), pb->height(),
                       (const uint8_t*)buffer, stride,
                       info, maxColours);
  }
}

void EncodeManager::selectEncoderForRect(const core::Rect& rect,
                                         const PixelBuffer* pb,
                                         PixelBuffer*& ppb,
                                         struct RectInfo* info,
                                         EncoderType& type)
{
  unsigned int divisor, maxColours;
  bool useRLE;

  // FIXME: This is roughly the algorithm previously used by the Tight
  //        encoder. It seems a bit backwards though, that higher
  //        compression setting means spending less effort in building
  //        a palette. It might be that they figured the increase in
  //        zlib setting compensated for the loss.
  if (conn->client.compressLevel == -1)
    divisor = 2 * 8;
  else
    divisor = conn->client.compressLevel * 8;
  if (divisor < 4)
    divisor = 4;

  maxColours = rect.area() / divisor;

  // Special exception inherited from the Tight encoder
  if (activeEncoders[encoderFullColour] == encoderTightJPEG) {
    if ((conn->client.compressLevel != -1) && (conn->client.compressLevel < 2))
      maxColours = 24;
    else
      maxColours = 96;
  }

  if (maxColours < 2)
    maxColours = 2;

  Encoder* encoder = encoders[activeEncoders[encoderIndexedRLE]];
  if (maxColours > encoder->maxPaletteSize)
    maxColours = encoder->maxPaletteSize;
  encoder = encoders[activeEncoders[encoderIndexed]];
  if (maxColours > encoder->maxPaletteSize)
    maxColours = encoder->maxPaletteSize;

  ppb = preparePixelBuffer(rect, pb, true);

  if (!analyseRect(ppb, info, maxColours))
    info->palette.clear();

  // Different encoders might have different RLE overhead, but
  // here we do a guess at RLE being the better choice if reduces
  // the pixel count by 50%.
  useRLE = info->rleRuns <= (rect.area() * 2);

  switch (info->palette.size()) {
  case 0:
    type = encoderFullColour;
    break;
  case 1:
    type = encoderSolid;
    break;
  case 2:
    type = useRLE ? encoderBitmapRLE : encoderBitmap;
    break;
  default:
    type = useRLE ? encoderIndexedRLE : encoderIndexed;
    break;
  }
}

void EncodeManager::OffsetPixelBuffer::update(const PixelFormat& pf,
                                              int width_, int height_,
                                              const uint8_t* data_,
                                              int stride_)
{
  format = pf;
  // Forced cast. We never write anything though, so it should be safe.
  setBuffer(width_, height_, (uint8_t*)data_, stride_);
}

uint8_t* EncodeManager::OffsetPixelBuffer::getBufferRW(const core::Rect& /*r*/, int* /*stride*/)
{
  throw std::logic_error("Invalid write attempt to OffsetPixelBuffer");
}

bool EncodeManager::tryPersistentCacheLookup(const core::Rect& rect,
                                             const PixelBuffer* pb)
{
  // NOTE: The unified cache engine is now driven purely by protocol
  // negotiation (client SetEncodings) rather than a separate
  // server-side on/off switch. The VNCSConnectionST::setEncodings
  // implementation enables caching whenever the client advertises
  // either ContentCache or PersistentCache. We therefore do not gate
  // lookups on a separate usePersistentCache flag here; tests like
  // test_cachedrect_init_propagation.py and test_cpp_cache_back_to_back.py
  // expect cache statistics even when EnablePersistentCache=0 on the
  // server (session-only ContentCache policy).

  // Skip if below minimum size threshold
  if (rect.area() < Server::persistentCacheMinRectSize)
    return false;

  // Require client support for *some* cache protocol. Under the unified
  // engine, pseudoEncodingContentCache and pseudoEncodingPersistentCache
  // are handled by the same 64-bit ID path; the difference is purely
  // policy (ephemeral vs persistent) on the viewer side.
  bool clientSupportsPersistent =
    conn->client.supportsEncoding(pseudoEncodingPersistentCache);
  bool clientSupportsContentCache =
    conn->client.supportsEncoding(pseudoEncodingContentCache);
  if (!clientSupportsPersistent && !clientSupportsContentCache)
    return false;

  persistentCacheStats.cacheLookups++;
  
  // Compute content hash using ContentHash utility (same as ContentCache)
  std::vector<uint8_t> fullHash = ContentHash::computeRect(pb, rect);

  uint64_t cacheId = 0;
  if (!fullHash.empty()) {
    size_t n = std::min(fullHash.size(), sizeof(uint64_t));
    memcpy(&cacheId, fullHash.data(), n);
  }

  // Optional framebuffer hash debug: log the canonical bytes used for
  // hashing at the exact point where the server computes cacheId.
  logFBHashDebug("tryLookup", rect, cacheId, pb);

  
  // Check if the client knows this content via either the canonical ID or a
  // lossy ID that it reported back via PersistentCacheHashReport.
  //
  // PREFERENCE ORDERING:
  // 1. Canonical ID (lossless, best quality) - check first
  // 2. Lossy ID (degraded quality) - fallback only
  //
  // This ensures we always prefer lossless content when available. The lossy
  // mapping is only used when the canonical content is NOT available.
  
  // If the client explicitly requested this ID (e.g. after a cache miss),
  // force a "miss" here so we send the full INIT again.
  bool clientRequested = conn->clientRequestedPersistent(cacheId);
  
  bool hasCanonicalMatch = conn->knowsPersistentId(cacheId) && !clientRequested;
  uint64_t lossyId = 0;
  bool hasLossyMatch = false;
  uint64_t matchedId = cacheId;

  if (hasCanonicalMatch) {
    // Prefer canonical (lossless) content when available
    matchedId = cacheId;
    vlog.debug("tryLookup: Using canonical (lossless) ID %s", hex64(cacheId));
  } else if (conn->hasLossyHash(cacheId, lossyId) &&
             conn->knowsPersistentId(lossyId) &&
             !clientRequested) {
    // Fall back to lossy content only when lossless not available
    hasLossyMatch = true;
    matchedId = lossyId;
    vlog.debug("tryLookup: Using lossy hash match id=%s (canonical=%s not available)",
               hex64(lossyId), hex64(cacheId));
  }

  if (hasCanonicalMatch || hasLossyMatch) {
    // Cache hit! Client has this content, send reference
    persistentCacheStats.cacheHits++;

    bool topBandRect = (rect.tl.y < 100 && rect.br.y > 20);
    if (topBandRect) {
      vlog.info("PCSRV TOPBAND_CACHE_HIT: conn=%p rect=[%d,%d-%d,%d] id=%s%s",
                (void*)conn,
                rect.tl.x, rect.tl.y, rect.br.x, rect.br.y,
                hex64(matchedId),
                hasLossyMatch ? " (lossy)" : "");
    }
    int equiv = 12 + rect.area() * (conn->client.pf().bpp/8);
    // PersistentCachedRect overhead now matches CachedRect: 20 bytes
    persistentCacheStats.bytesSaved += equiv - 20;
    copyStats.rects++;
    copyStats.pixels += rect.area();
    copyStats.equivalent += equiv;
    beforeLength = conn->getOutStream()->length();
    conn->writer()->writePersistentCachedRect(rect, matchedId);
    copyStats.bytes += conn->getOutStream()->length() - beforeLength;
    
    vlog.debug("PersistentCache protocol HIT: rect [%d,%d-%d,%d] id=%s saved %d bytes%s",
               rect.tl.x, rect.tl.y, rect.br.x, rect.br.y,
               hex64(matchedId),
               equiv - 20,
               hasLossyMatch ? " (lossy)" : "");
    
    // Remember that this client just referenced this matchedId for this
    // rectangle so that a subsequent RequestCachedData can trigger a
    // targeted refresh of the same region.
    conn->onCachedRectRef(matchedId, rect);
    
    // Only clear lossy tracking if the client has lossless content.
    // If the hit is for a lossy entry, keep the region in lossyRegion
    // so that lossless refresh will eventually upgrade quality.
    if (!hasLossyMatch) {
      lossyRegion.assign_subtract(rect);
    }
    pendingRefreshRegion.assign_subtract(rect);
    return true;
  }
  
  // Client doesn't know this ID - send PersistentCachedRectInit.
  // This allows future identical rects to be references.
  persistentCacheStats.cacheMisses++;

  bool topBandRect = (rect.tl.y < 100 && rect.br.y > 20);
  if (topBandRect) {
    vlog.info("PCSRV TOPBAND_CACHE_INIT: conn=%p rect=[%d,%d-%d,%d] id=%s",
              (void*)conn,
              rect.tl.x, rect.tl.y, rect.br.x, rect.br.y,
              hex64(cacheId));
  }
  
  // Choose payload encoder using the shared selection logic
  PixelBuffer *ppb;
  Encoder *payloadEnc;
  struct RectInfo info;
  EncoderType type;

  selectEncoderForRect(rect, pb, ppb, &info, type);

  payloadEnc = encoders[activeEncoders[type]];

  // LOSSY CACHING ENABLED: We now allow lossy rectangles (JPEG, etc.) to be
  // cached. The client-side DecodeManager will validate the decoded pixels
  // against the server's cacheId using a quality-aware approach:
  //   - Lossless encodings: exact hash match required
  //   - Lossy encodings: hash mismatch tolerated, entry stored as session-only
  //
  // This dramatically improves cache hit rates for real-world content where
  // JPEG compression is common. Lossy cache entries are flagged as non-persistent
  // (isLossless=false) so they remain memory-only and don't persist cross-session,
  // avoiding any long-term visual drift from compression artifacts.
  //
  // The old blocker is commented out below:
  // if (payloadEnc->flags & EncoderLossy) {
  //   vlog.debug("PersistentCache: skipping INIT due to lossy encoder");
  //   return false;
  // }

  // Emit PersistentCachedRectInit header (ID + encoding)
  conn->writer()->writePersistentCachedRectInit(rect, cacheId, payloadEnc->encoding);
  // Prepare pixel buffer respecting native-PF usage for the payload encoder
  if (payloadEnc->flags & EncoderUseNativePF)
    ppb = preparePixelBuffer(rect, pb, false);
  // Write the encoded pixel payload
  payloadEnc->writeRect(ppb, info.palette);
  // Close the PersistentCachedRectInit rectangle
  conn->writer()->endRect();

  // Mark ID as known in session tracking (for future hits)
  conn->markPersistentIdKnown(cacheId);
  // Also track in EncodeManager (legacy, may be redundant with session tracking)
  clientKnownIds_.add(cacheId);
  // Clear any explicit request for this ID
  if (conn->clientRequestedPersistent(cacheId)) {
    conn->clearClientPersistentRequest(cacheId);
  }

  // Only clear lossy tracking if the payload encoder is lossless.
  // If this INIT used a lossy encoder (e.g., Tight/JPEG), keep the
  // region in lossyRegion so that lossless refresh will upgrade quality.
  bool payloadIsLossy = (payloadEnc->flags & EncoderLossy) &&
                        ((payloadEnc->losslessQuality == -1) ||
                         (payloadEnc->getQualityLevel() < payloadEnc->losslessQuality));
  if (!payloadIsLossy) {
    lossyRegion.assign_subtract(rect);
  }
  pendingRefreshRegion.assign_subtract(rect);
  
  vlog.debug("PersistentCache INIT: rect [%d,%d-%d,%d] id=%s (now known for session)",
             rect.tl.x, rect.tl.y, rect.br.x, rect.br.y, hex64(cacheId));
  return true;
}

void EncodeManager::addClientKnownHash(uint64_t cacheId)
{
  clientKnownIds_.add(cacheId);
}

void EncodeManager::removeClientKnownHash(uint64_t cacheId)
{
  clientKnownIds_.remove(cacheId);
}

bool EncodeManager::clientKnowsHash(uint64_t cacheId) const
{
  return clientKnownIds_.has(cacheId);
}

template<class T>
inline bool EncodeManager::checkSolidTile(int width, int height,
                                          const T* buffer, int stride,
                                          const T colourValue)
{
  int pad;

  pad = stride - width;

  while (height--) {
    int width_ = width;
    while (width_--) {
      if (*buffer != colourValue)
        return false;
      buffer++;
    }
    buffer += pad;
  }

  return true;
}

template<class T>
inline bool EncodeManager::analyseRect(int width, int height,
                                       const T* buffer, int stride,
                                       struct RectInfo *info, int maxColours)
{
  int pad;

  T colour;
  int count;

  info->rleRuns = 0;
  info->palette.clear();

  pad = stride - width;

  // For efficiency, we only update the palette on changes in colour
  colour = buffer[0];
  count = 0;
  while (height--) {
    int w_ = width;
    while (w_--) {
      if (*buffer != colour) {
        if (!info->palette.insert(colour, count))
          return false;
        if (info->palette.size() > maxColours)
          return false;

        // FIXME: This doesn't account for switching lines
        info->rleRuns++;

        colour = *buffer;
        count = 0;
      }
      buffer++;
      count++;
    }
    buffer += pad;
  }

  // Make sure the final pixels also get counted
  if (!info->palette.insert(colour, count))
    return false;
  if (info->palette.size() > maxColours)
    return false;

  return true;
}

} // namespace rfb
