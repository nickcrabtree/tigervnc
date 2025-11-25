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

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <rfb/GlobalClientPersistentCache.h>
#include <core/LogWriter.h>

#include <time.h>
#include <cstring>
#include <algorithm>
#include <fstream>
#include <sys/stat.h>
#include <errno.h>
#include <iostream>
#include <iomanip>

#ifdef HAVE_GNUTLS
#include <gnutls/gnutls.h>
#include <gnutls/crypto.h>
#else
// Fallback to a simple checksum if GnuTLS not available
#include <zlib.h>
#endif

using namespace rfb;

static core::LogWriter vlog("PersistentCache");

// ============================================================================
// PersistentCache Debug Logger Implementation
// ============================================================================

PersistentCacheDebugLogger::PersistentCacheDebugLogger() {
  auto now = std::chrono::system_clock::now();
  auto time_val = std::chrono::system_clock::to_time_t(now);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
    now.time_since_epoch()) % 1000;
  
  std::string timestamp = std::to_string(time_val) + "_" + std::to_string(ms.count());
  logFilename_ = "/tmp/persistentcache_debug_" + timestamp + ".log";
  
  logFile_.open(logFilename_, std::ios::out | std::ios::app);
  if (logFile_.is_open()) {
    std::cout << "PersistentCache debug log: " << logFilename_ << std::endl;
    log("=== PersistentCache Debug Log Started ===");
  } else {
    std::cerr << "Failed to open PersistentCache debug log: " << logFilename_ << std::endl;
  }
}

PersistentCacheDebugLogger::~PersistentCacheDebugLogger() {
  if (logFile_.is_open()) {
    log("=== PersistentCache Debug Log Ended ===");
    logFile_.close();
  }
}

void PersistentCacheDebugLogger::log(const std::string& message) {
  std::lock_guard<std::mutex> lock(logMutex_);
  if (logFile_.is_open()) {
    auto now = std::chrono::system_clock::now();
    auto time_val = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
      now.time_since_epoch()) % 1000;
    
    logFile_ << "[" << time_val << "." << std::setfill('0') << std::setw(3) << ms.count()
             << "] " << message << std::endl;
    logFile_.flush();
  }
}

// ============================================================================
// GlobalClientPersistentCache Implementation - ARC Algorithm
// ============================================================================

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxSizeMB,
                                                           const std::string& cacheFilePathOverride)
  : maxCacheSize_(maxSizeMB * 1024 * 1024),
    hydrationState_(HydrationState::Uninitialized),
    cacheFileHandle_(nullptr)
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor ENTER: maxSizeMB=" + std::to_string(maxSizeMB));
  
  memset(&stats_, 0, sizeof(stats_));
  
  // Determine cache file path: allow viewer parameter override via
  // PersistentCachePath so tests can control cold vs warm behaviour.
  if (!cacheFilePathOverride.empty()) {
    cacheFilePath_ = cacheFilePathOverride;
  } else {
    const char* home = getenv("HOME");
    if (home) {
      cacheFilePath_ = std::string(home) + "/.cache/tigervnc/persistentcache.dat";
    } else {
      cacheFilePath_ = "/tmp/tigervnc_persistentcache.dat";
    }
  }
  
  vlog.debug("PersistentCache created with ARC: maxSize=%zuMB, path=%s", 
             maxSizeMB, cacheFilePath_.c_str());

  // Initialize shared ArcCache with eviction callback
  arcCache_.reset(new rfb::cache::ArcCache<std::vector<uint8_t>, CachedPixels, HashVectorHasher>(
      maxCacheSize_,
      [](const CachedPixels& e) { return e.byteSize(); },
      [this](const std::vector<uint8_t>& h) { pendingEvictions_.push_back(h); }
  ));
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor EXIT: cacheFilePath=" + cacheFilePath_);
}

GlobalClientPersistentCache::~GlobalClientPersistentCache()
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor ENTER: entries=" + std::to_string(cache_.size()));
  
  // Close file handle if still open from lazy loading
  if (cacheFileHandle_) {
    fclose(cacheFileHandle_);
    cacheFileHandle_ = nullptr;
  }
  
  vlog.debug("PersistentCache destroyed: %zu entries", cache_.size());
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor EXIT");
}

bool GlobalClientPersistentCache::has(const std::vector<uint8_t>& hash) const
{
  // Check both fully-hydrated ARC cache and index-only entries (lazy load)
  if (arcCache_ && arcCache_->has(hash))
    return true;
  // Also check indexMap_ for entries loaded but not yet hydrated
  return indexMap_.find(hash) != indexMap_.end();
}

const GlobalClientPersistentCache::CachedPixels* 
GlobalClientPersistentCache::get(const std::vector<uint8_t>& hash)
{
  if (!arcCache_) return nullptr;
  
  // First check if already in ARC cache (hydrated)
  const CachedPixels* e = arcCache_->get(hash);
  if (e != nullptr) {
    stats_.cacheHits++;
    return e;
  }
  
  // Check if in index but not yet hydrated (lazy load)
  auto indexIt = indexMap_.find(hash);
  if (indexIt != indexMap_.end()) {
    // Entry exists on disk but not loaded - hydrate it now (on-demand)
    if (hydrateEntry(hash)) {
      // Re-fetch from ARC cache after hydration
      e = arcCache_->get(hash);
      if (e != nullptr) {
        stats_.cacheHits++;
        return e;
      }
    }
    // Hydration failed - treat as miss
  }
  
  // Not found anywhere
  stats_.cacheMisses++;
  return nullptr;
}

void GlobalClientPersistentCache::insert(const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels)
{
  if (!arcCache_ || pixels == nullptr || width == 0 || height == 0)
    return;

  // Update ARC statistics: treat new inserts as misses and
  // re-initialisations of existing entries as hits. This mirrors
  // the ContentCache fix in commit 8902e213 ("correct ARC cache
  // hit/miss statistics in ContentCache") so that persistent
  // cache stats don't report an impossible 100% hit rate when
  // many new entries are created.
  if (arcCache_->has(hash)) {
    stats_.cacheHits++;
  } else {
    stats_.cacheMisses++;
  }

  // Build CachedPixels entry and copy rows respecting stride (pixels)
  CachedPixels entry;
  entry.format = pf;
  entry.width = width;
  entry.height = height;
  // Store pixels contiguously in our cache to simplify later blits
  // NOTE: stridePixels in this struct is in PIXELS for the stored buffer.
  // Since we copy rows tightly, the stored stride is exactly the width.
  entry.stridePixels = width;
  entry.lastAccessTime = getCurrentTime();
 
  const size_t bppBytes = pf.bpp / 8;
  const size_t rowBytes = (size_t)width * bppBytes;
  const size_t srcStrideBytes = (size_t)stridePixels * bppBytes;
  entry.pixels.resize((size_t)height * rowBytes);
  const uint8_t* src = pixels;
  uint8_t* dst = entry.pixels.data();
  for (uint16_t y = 0; y < height; y++) {
    memcpy(dst, src, rowBytes);
    src += srcStrideBytes;
    dst += rowBytes;
  }

  // Keep persistence map in sync (used for save/load); note this duplicates memory temporarily
  cache_[hash] = entry;
  arcCache_->insert(hash, entry);
}

std::vector<std::vector<uint8_t>> 
GlobalClientPersistentCache::getAllHashes() const
{
  std::vector<std::vector<uint8_t>> hashes;
  // Include both hydrated entries (cache_) and index-only entries (indexMap_)
  hashes.reserve(cache_.size() + indexMap_.size());
  for (const auto& entry : cache_) {
    hashes.push_back(entry.first);
  }
  // Add index-only entries that haven't been hydrated yet
  for (const auto& entry : indexMap_) {
    // Skip if already in cache_ (would be duplicate)
    if (cache_.find(entry.first) == cache_.end()) {
      hashes.push_back(entry.first);
    }
  }
  return hashes;
}

void GlobalClientPersistentCache::clear()
{
  if (arcCache_) arcCache_->clear();
  cache_.clear();
  pendingEvictions_.clear();
  stats_.totalEntries = 0;
  stats_.totalBytes = 0;
  vlog.debug("PersistentCache cleared");
}

GlobalClientPersistentCache::Stats 
GlobalClientPersistentCache::getStats() const
{
  Stats current = stats_;
  size_t totalEntries = cache_.size();
  size_t totalBytes = 0;
  size_t t1Count = 0, t2Count = 0, b1Count = 0, b2Count = 0, target = 0;
  if (arcCache_) {
    auto s = arcCache_->getStats();
    totalEntries = s.totalEntries;
    totalBytes = s.totalBytes;
    t1Count = s.t1Size;
    t2Count = s.t2Size;
    b1Count = s.b1Size;
    b2Count = s.b2Size;
    target = s.targetT1Size;
  }
  current.totalEntries = totalEntries;
  current.totalBytes = totalBytes;
  current.t1Size = t1Count;
  current.t2Size = t2Count;
  current.b1Size = b1Count;
  current.b2Size = b2Count;
  current.targetT1Size = target;
  return current;
}

void GlobalClientPersistentCache::resetStats()
{
  stats_.cacheHits = 0;
  stats_.cacheMisses = 0;
  stats_.evictions = 0;
}

void GlobalClientPersistentCache::setMaxSize(size_t maxSizeMB)
{
  maxCacheSize_ = maxSizeMB * 1024 * 1024;
  vlog.debug("PersistentCache max size set to %zuMB", maxSizeMB);
  // Recreate arc cache to apply new capacity
  arcCache_.reset(new rfb::cache::ArcCache<std::vector<uint8_t>, CachedPixels, HashVectorHasher>(
      maxCacheSize_,
      [](const CachedPixels& e) { return e.byteSize(); },
      [this](const std::vector<uint8_t>& h) { pendingEvictions_.push_back(h); }
  ));
}

bool GlobalClientPersistentCache::loadFromDisk()
{
  std::ifstream file(cacheFilePath_, std::ios::binary);
  if (!file.is_open()) {
    vlog.info("PersistentCache: no cache file found at %s (fresh start)", 
              cacheFilePath_.c_str());
    return false;
  }
  
  vlog.info("PersistentCache: loading from %s", cacheFilePath_.c_str());
  
  // Read header
  struct Header {
    uint32_t magic;
    uint32_t version;
    uint64_t totalEntries;
    uint64_t totalBytes;
    uint64_t created;
    uint64_t lastAccess;
    uint8_t reserved[24];
  } header;
  
  file.read(reinterpret_cast<char*>(&header), sizeof(header));
  if (!file.good()) {
    vlog.error("PersistentCache: failed to read header");
    file.close();
    return false;
  }
  
  // Verify magic number
  const uint32_t MAGIC = 0x50435643;  // "PCVC"
  if (header.magic != MAGIC) {
    vlog.error("PersistentCache: invalid magic number 0x%08x (expected 0x%08x)",
               header.magic, MAGIC);
    file.close();
    return false;
  }
  
  // Check version
  if (header.version != 1) {
    vlog.error("PersistentCache: unsupported version %u", header.version);
    file.close();
    return false;
  }
  
  vlog.info("PersistentCache: header valid, loading %llu entries (%llu bytes)",
            (unsigned long long)header.totalEntries,
            (unsigned long long)header.totalBytes);
  
  // Read entries
  size_t loadedEntries = 0;
  size_t loadedBytes = 0;
  
  for (uint64_t i = 0; i < header.totalEntries; i++) {
    // Read hash
    uint8_t hashLen;
    file.read(reinterpret_cast<char*>(&hashLen), 1);
    if (!file.good()) break;
    
    std::vector<uint8_t> hash(hashLen);
    file.read(reinterpret_cast<char*>(hash.data()), hashLen);
    if (!file.good()) break;
    
    // Read dimensions and format
    uint16_t width, height, stridePixels;
    file.read(reinterpret_cast<char*>(&width), sizeof(width));
    file.read(reinterpret_cast<char*>(&height), sizeof(height));
    file.read(reinterpret_cast<char*>(&stridePixels), sizeof(stridePixels));
    
    // Read PixelFormat (24 bytes)
    PixelFormat pf;
    file.read(reinterpret_cast<char*>(&pf), 24);
    
    uint32_t lastAccess;
    file.read(reinterpret_cast<char*>(&lastAccess), sizeof(lastAccess));
    
    // Read pixel data
    uint32_t pixelDataLen;
    file.read(reinterpret_cast<char*>(&pixelDataLen), sizeof(pixelDataLen));
    if (!file.good()) break;
    
    std::vector<uint8_t> pixelData(pixelDataLen);
    file.read(reinterpret_cast<char*>(pixelData.data()), pixelDataLen);
    if (!file.good()) break;
    
    // Insert into cache (will add to T1 initially)
    insert(hash, pixelData.data(), pf, width, height, stridePixels);
    
    loadedEntries++;
    loadedBytes += pixelDataLen;
    
    // Stop if we've exceeded max size
    if (arcCache_) {
      auto s = arcCache_->getStats();
      if (s.totalBytes >= maxCacheSize_) {
        vlog.info("PersistentCache: reached max size, stopping load");
        break;
      }
    }
  }
  
  // Note: We skip checksum verification for now for simplicity
  // Production code should verify the trailing SHA-256 checksum
  
  file.close();
  
  vlog.info("PersistentCache: loaded %zu entries (%zu bytes) from disk",
            loadedEntries, loadedBytes);
  
  hydrationState_ = HydrationState::FullyHydrated;  // v1 format = eagerly loaded
  return loadedEntries > 0;
}

bool GlobalClientPersistentCache::loadIndexFromDisk()
{
  // Check if file exists
  FILE* f = fopen(cacheFilePath_.c_str(), "rb");
  if (!f) {
    vlog.info("PersistentCache: no cache file found at %s (fresh start)",
              cacheFilePath_.c_str());
    hydrationState_ = HydrationState::FullyHydrated;  // Nothing to load
    return false;
  }
  
  // Read header to check version
  struct HeaderV1 {
    uint32_t magic;
    uint32_t version;
    uint64_t totalEntries;
    uint64_t totalBytes;
    uint64_t created;
    uint64_t lastAccess;
    uint8_t reserved[24];
  } header;
  
  if (fread(&header, sizeof(header), 1, f) != 1) {
    vlog.error("PersistentCache: failed to read header");
    fclose(f);
    return false;
  }
  
  const uint32_t MAGIC = 0x50435643;  // "PCVC"
  if (header.magic != MAGIC) {
    vlog.error("PersistentCache: invalid magic 0x%08x", header.magic);
    fclose(f);
    return false;
  }
  
  // Handle v1 format: delete and start fresh (per user request)
  if (header.version == 1) {
    vlog.info("PersistentCache: detected v1 format, deleting to avoid startup hang");
    fclose(f);
    // Delete the v1 cache file
    if (remove(cacheFilePath_.c_str()) != 0) {
      vlog.error("PersistentCache: failed to delete v1 cache: %s", strerror(errno));
    }
    hydrationState_ = HydrationState::FullyHydrated;  // Start fresh
    return false;
  }
  
  // Handle v2 format: read index section only
  if (header.version != 2) {
    vlog.error("PersistentCache: unsupported version %u", header.version);
    fclose(f);
    return false;
  }
  
  // v2 header has additional fields for index location
  struct HeaderV2 {
    uint32_t magic;
    uint32_t version;
    uint64_t indexOffset;    // File offset of index section
    uint64_t indexCount;     // Number of entries in index
    uint64_t payloadOffset;  // File offset of payload section (right after header)
    uint64_t created;
    uint64_t lastAccess;
    uint8_t reserved[16];
  } headerV2;
  
  // Re-read as v2 header
  fseek(f, 0, SEEK_SET);
  if (fread(&headerV2, sizeof(headerV2), 1, f) != 1) {
    vlog.error("PersistentCache: failed to read v2 header");
    fclose(f);
    return false;
  }
  
  vlog.info("PersistentCache: v2 format, loading index (%llu entries)",
            (unsigned long long)headerV2.indexCount);
  
  // Seek to index section
  if (fseek(f, headerV2.indexOffset, SEEK_SET) != 0) {
    vlog.error("PersistentCache: failed to seek to index section");
    fclose(f);
    return false;
  }
  
  // Read index entries
  indexMap_.clear();
  hydrationQueue_.clear();
  
  for (uint64_t i = 0; i < headerV2.indexCount; i++) {
    // Index entry format:
    // hashLen(1) + hash(hashLen) + offset(8) + size(4) + width(2) + height(2) + stride(2) + PixelFormat(24)
    uint8_t hashLen;
    if (fread(&hashLen, 1, 1, f) != 1) break;
    
    std::vector<uint8_t> hash(hashLen);
    if (fread(hash.data(), 1, hashLen, f) != hashLen) break;
    
    IndexEntry entry;
    if (fread(&entry.payloadOffset, sizeof(entry.payloadOffset), 1, f) != 1) break;
    if (fread(&entry.payloadSize, sizeof(entry.payloadSize), 1, f) != 1) break;
    if (fread(&entry.width, sizeof(entry.width), 1, f) != 1) break;
    if (fread(&entry.height, sizeof(entry.height), 1, f) != 1) break;
    if (fread(&entry.stridePixels, sizeof(entry.stridePixels), 1, f) != 1) break;
    if (fread(&entry.format, 24, 1, f) != 1) break;
    
    indexMap_[hash] = entry;
    hydrationQueue_.push_back(hash);
  }
  
  // Keep file open for lazy reads
  cacheFileHandle_ = f;
  hydrationState_ = HydrationState::IndexLoaded;
  
  vlog.info("PersistentCache: index loaded, %zu entries pending hydration",
            hydrationQueue_.size());
  
  return true;
}

bool GlobalClientPersistentCache::hydrateEntry(const std::vector<uint8_t>& hash)
{
  // Check if already hydrated
  if (arcCache_ && arcCache_->has(hash))
    return true;
  
  // Find in index
  auto it = indexMap_.find(hash);
  if (it == indexMap_.end())
    return false;  // Not in index
  
  // Need file handle for lazy reads
  if (!cacheFileHandle_) {
    vlog.error("PersistentCache: cannot hydrate, file handle closed");
    return false;
  }
  
  const IndexEntry& idx = it->second;
  
  // Seek to payload
  if (fseek(cacheFileHandle_, idx.payloadOffset, SEEK_SET) != 0) {
    vlog.error("PersistentCache: failed to seek to payload for hydration");
    return false;
  }
  
  // Read pixel data
  std::vector<uint8_t> pixelData(idx.payloadSize);
  if (fread(pixelData.data(), 1, idx.payloadSize, cacheFileHandle_) != idx.payloadSize) {
    vlog.error("PersistentCache: failed to read payload for hydration");
    return false;
  }
  
  // Build CachedPixels entry
  CachedPixels entry;
  entry.format = idx.format;
  entry.width = idx.width;
  entry.height = idx.height;
  entry.stridePixels = idx.stridePixels;
  entry.lastAccessTime = getCurrentTime();
  entry.pixels = std::move(pixelData);
  
  // Insert into ARC cache and persistence map
  cache_[hash] = entry;
  arcCache_->insert(hash, entry);
  
  // Remove from index and hydration queue (now fully loaded)
  indexMap_.erase(it);
  hydrationQueue_.remove(hash);
  
  // Update hydration state
  if (indexMap_.empty()) {
    hydrationState_ = HydrationState::FullyHydrated;
    // Close file handle when fully hydrated
    if (cacheFileHandle_) {
      fclose(cacheFileHandle_);
      cacheFileHandle_ = nullptr;
      vlog.debug("PersistentCache: fully hydrated, closed file handle");
    }
  } else {
    hydrationState_ = HydrationState::PartiallyHydrated;
  }
  
  return true;
}

size_t GlobalClientPersistentCache::hydrateNextBatch(size_t maxEntries)
{
  if (hydrationQueue_.empty())
    return 0;
  
  if (!cacheFileHandle_)
    return 0;
  
  size_t hydrated = 0;
  
  // Process up to maxEntries from the front of the queue
  while (hydrated < maxEntries && !hydrationQueue_.empty()) {
    std::vector<uint8_t> hash = hydrationQueue_.front();
    
    if (hydrateEntry(hash)) {
      hydrated++;
    } else {
      // Failed to hydrate, remove from queue anyway to avoid infinite loop
      hydrationQueue_.pop_front();
    }
  }
  
  if (hydrated > 0) {
    vlog.debug("PersistentCache: proactively hydrated %zu entries, %zu remaining",
               hydrated, hydrationQueue_.size());
  }
  
  return hydrated;
}

bool GlobalClientPersistentCache::saveToDisk()
{
  // Close any open file handle before writing (we may have it open for lazy reads)
  if (cacheFileHandle_) {
    fclose(cacheFileHandle_);
    cacheFileHandle_ = nullptr;
  }
  
  if (cache_.empty()) {
    vlog.debug("PersistentCache: cache empty, nothing to save");
    return true;
  }
  
  // Ensure directory exists
  size_t lastSlash = cacheFilePath_.rfind('/');
  if (lastSlash != std::string::npos) {
    std::string dir = cacheFilePath_.substr(0, lastSlash);
    
    // Create directory (mkdir -p equivalent)
    std::string currentPath;
    for (size_t i = 0; i < dir.length(); i++) {
      if (dir[i] == '/' && i > 0) {
        currentPath = dir.substr(0, i);
        mkdir(currentPath.c_str(), 0755);
      }
    }
    mkdir(dir.c_str(), 0755);
  }
  
  FILE* f = fopen(cacheFilePath_.c_str(), "wb");
  if (!f) {
    vlog.error("PersistentCache: failed to open %s for writing: %s",
               cacheFilePath_.c_str(), strerror(errno));
    return false;
  }
  
  vlog.info("PersistentCache: saving %zu entries (v2 format) to %s",
            cache_.size(), cacheFilePath_.c_str());
  
  // v2 Header structure (64 bytes)
  struct HeaderV2 {
    uint32_t magic;
    uint32_t version;
    uint64_t indexOffset;    // File offset of index section (filled later)
    uint64_t indexCount;     // Number of entries
    uint64_t payloadOffset;  // File offset of payload section (right after header)
    uint64_t created;
    uint64_t lastAccess;
    uint8_t reserved[16];
  } header;
  
  memset(&header, 0, sizeof(header));
  header.magic = 0x50435643;  // "PCVC"
  header.version = 2;
  header.indexCount = cache_.size();
  header.payloadOffset = sizeof(header);  // Payloads start right after header
  header.created = time(nullptr);
  header.lastAccess = time(nullptr);
  // indexOffset will be filled after writing payloads
  
  // Write header placeholder (we'll seek back and rewrite with correct indexOffset)
  if (fwrite(&header, sizeof(header), 1, f) != 1) {
    vlog.error("PersistentCache: failed to write header");
    fclose(f);
    return false;
  }
  
  // Build index entries while writing payloads
  struct IndexEntryWrite {
    std::vector<uint8_t> hash;
    uint64_t payloadOffset;
    uint32_t payloadSize;
    uint16_t width;
    uint16_t height;
    uint16_t stridePixels;
    PixelFormat format;
  };
  std::vector<IndexEntryWrite> indexEntries;
  indexEntries.reserve(cache_.size());
  
  // Write payloads section
  for (const auto& entry : cache_) {
    const std::vector<uint8_t>& hash = entry.first;
    const CachedPixels& pixels = entry.second;
    
    IndexEntryWrite idx;
    idx.hash = hash;
    idx.payloadOffset = ftell(f);  // Current position is payload start
    idx.payloadSize = pixels.pixels.size();
    idx.width = pixels.width;
    idx.height = pixels.height;
    idx.stridePixels = pixels.stridePixels;
    idx.format = pixels.format;
    
    // Write pixel data
    if (fwrite(pixels.pixels.data(), 1, pixels.pixels.size(), f) != pixels.pixels.size()) {
      vlog.error("PersistentCache: failed to write payload");
      fclose(f);
      return false;
    }
    
    indexEntries.push_back(idx);
  }
  
  // Record index section offset
  header.indexOffset = ftell(f);
  
  // Write index section
  for (const auto& idx : indexEntries) {
    // hashLen(1) + hash(hashLen) + offset(8) + size(4) + width(2) + height(2) + stride(2) + PixelFormat(24)
    uint8_t hashLen = idx.hash.size();
    fwrite(&hashLen, 1, 1, f);
    fwrite(idx.hash.data(), 1, hashLen, f);
    fwrite(&idx.payloadOffset, sizeof(idx.payloadOffset), 1, f);
    fwrite(&idx.payloadSize, sizeof(idx.payloadSize), 1, f);
    fwrite(&idx.width, sizeof(idx.width), 1, f);
    fwrite(&idx.height, sizeof(idx.height), 1, f);
    fwrite(&idx.stridePixels, sizeof(idx.stridePixels), 1, f);
    fwrite(&idx.format, 24, 1, f);
  }
  
  // Write footer (32-byte checksum placeholder)
  uint8_t checksum[32] = {0};
  fwrite(checksum, 1, 32, f);
  
  // Seek back and update header with correct indexOffset
  fseek(f, 0, SEEK_SET);
  if (fwrite(&header, sizeof(header), 1, f) != 1) {
    vlog.error("PersistentCache: failed to rewrite header with indexOffset");
    fclose(f);
    return false;
  }
  
  fclose(f);
  
  vlog.info("PersistentCache: saved %zu entries to disk (v2 format)", indexEntries.size());
  return true;
}

// (Replaced by shared ArcCache implementation)

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
