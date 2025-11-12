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

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxSizeMB)
  : maxCacheSize_(maxSizeMB * 1024 * 1024)
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor ENTER: maxSizeMB=" + std::to_string(maxSizeMB));
  
  memset(&stats_, 0, sizeof(stats_));
  
  // Determine cache file path
  const char* home = getenv("HOME");
  if (home) {
    cacheFilePath_ = std::string(home) + "/.cache/tigervnc/persistentcache.dat";
  } else {
    cacheFilePath_ = "/tmp/tigervnc_persistentcache.dat";
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
  
  vlog.debug("PersistentCache destroyed: %zu entries", cache_.size());
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor EXIT");
}

bool GlobalClientPersistentCache::has(const std::vector<uint8_t>& hash) const
{
  return arcCache_ && arcCache_->has(hash);
}

const GlobalClientPersistentCache::CachedPixels* 
GlobalClientPersistentCache::get(const std::vector<uint8_t>& hash)
{
  if (!arcCache_) return nullptr;
  const CachedPixels* e = arcCache_->get(hash);
  if (e == nullptr) {
    stats_.cacheMisses++;
    return nullptr;
  }
  stats_.cacheHits++;
  return e;
}

void GlobalClientPersistentCache::insert(const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels)
{
  if (!arcCache_ || pixels == nullptr || width == 0 || height == 0)
    return;

  // Build CachedPixels entry and copy rows respecting stride (pixels)
  CachedPixels entry;
  entry.format = pf;
  entry.width = width;
  entry.height = height;
  entry.stridePixels = stridePixels;
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
  hashes.reserve(cache_.size());
  for (const auto& entry : cache_) {
    hashes.push_back(entry.first);
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
  
  return loadedEntries > 0;
}

bool GlobalClientPersistentCache::saveToDisk()
{
  if (cache_.empty()) {
    vlog.debug("PersistentCache: cache empty, nothing to save");
    return true;
  }
  
  // Ensure directory exists
  size_t lastSlash = cacheFilePath_.rfind('/');
  if (lastSlash != std::string::npos) {
    std::string dir = cacheFilePath_.substr(0, lastSlash);
    
    // Create directory (mkdir -p equivalent)
    // Try to create each level
    std::string currentPath;
    for (size_t i = 0; i < dir.length(); i++) {
      if (dir[i] == '/' && i > 0) {
        currentPath = dir.substr(0, i);
        mkdir(currentPath.c_str(), 0755);
      }
    }
    mkdir(dir.c_str(), 0755);
  }
  
  std::ofstream file(cacheFilePath_, std::ios::binary | std::ios::trunc);
  if (!file.is_open()) {
    vlog.error("PersistentCache: failed to open %s for writing: %s",
               cacheFilePath_.c_str(), strerror(errno));
    return false;
  }
  
  vlog.info("PersistentCache: saving %zu entries to %s",
            cache_.size(), cacheFilePath_.c_str());
  
  // Write header
  struct Header {
    uint32_t magic;
    uint32_t version;
    uint64_t totalEntries;
    uint64_t totalBytes;
    uint64_t created;
    uint64_t lastAccess;
    uint8_t reserved[24];
  } header;
  
  memset(&header, 0, sizeof(header));
  header.magic = 0x50435643;  // "PCVC"
  header.version = 1;
  header.totalEntries = cache_.size();
  header.totalBytes = arcCache_ ? arcCache_->getStats().totalBytes : 0;
  header.created = time(nullptr);
  header.lastAccess = time(nullptr);
  
  file.write(reinterpret_cast<const char*>(&header), sizeof(header));
  
  // Write entries
  size_t written = 0;
  for (const auto& entry : cache_) {
    const std::vector<uint8_t>& hash = entry.first;
    const CachedPixels& pixels = entry.second;
    
    // Write hash
    uint8_t hashLen = hash.size();
    file.write(reinterpret_cast<const char*>(&hashLen), 1);
    file.write(reinterpret_cast<const char*>(hash.data()), hashLen);
    
    // Write dimensions
    file.write(reinterpret_cast<const char*>(&pixels.width), sizeof(pixels.width));
    file.write(reinterpret_cast<const char*>(&pixels.height), sizeof(pixels.height));
    file.write(reinterpret_cast<const char*>(&pixels.stridePixels), sizeof(pixels.stridePixels));
    
    // Write PixelFormat (24 bytes)
    file.write(reinterpret_cast<const char*>(&pixels.format), 24);
    
    // Write lastAccess
    file.write(reinterpret_cast<const char*>(&pixels.lastAccessTime), sizeof(pixels.lastAccessTime));
    
    // Write pixel data
    uint32_t pixelDataLen = pixels.pixels.size();
    file.write(reinterpret_cast<const char*>(&pixelDataLen), sizeof(pixelDataLen));
    file.write(reinterpret_cast<const char*>(pixels.pixels.data()), pixelDataLen);
    
    written++;
  }
  
  // Write simple checksum (CRC32 for now, SHA-256 would be better)
  // For simplicity, we'll skip the checksum for now
  uint8_t checksum[32] = {0};
  file.write(reinterpret_cast<const char*>(checksum), 32);
  
  file.close();
  
  vlog.info("PersistentCache: saved %zu entries to disk", written);
  return true;
}

// (Replaced by shared ArcCache implementation)

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
