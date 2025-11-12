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
  auto time_t = std::chrono::system_clock::to_time_t(now);
  auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
    now.time_since_epoch()) % 1000;
  
  std::string timestamp = std::to_string(time_t) + "_" + std::to_string(ms.count());
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
    auto time_t = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
      now.time_since_epoch()) % 1000;
    
    logFile_ << "[" << time_t << "." << std::setfill('0') << std::setw(3) << ms.count() 
             << "] " << message << std::endl;
    logFile_.flush();
  }
}

// ============================================================================
// GlobalClientPersistentCache Implementation - ARC Algorithm
// ============================================================================

GlobalClientPersistentCache::GlobalClientPersistentCache(size_t maxSizeMB)
  : p_(0),
    maxCacheSize_(maxSizeMB * 1024 * 1024),
    t1Size_(0),
    t2Size_(0)
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
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache constructor EXIT: cacheFilePath=" + cacheFilePath_);
}

GlobalClientPersistentCache::~GlobalClientPersistentCache()
{
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor ENTER: entries=" + std::to_string(cache_.size()));
  
  vlog.debug("PersistentCache destroyed: %zu entries, T1=%zu T2=%zu",
             cache_.size(), t1_.size(), t2_.size());
  
  PersistentCacheDebugLogger::getInstance().log("GlobalClientPersistentCache destructor EXIT");
}

bool GlobalClientPersistentCache::has(const std::vector<uint8_t>& hash) const
{
  return cache_.find(hash) != cache_.end();
}

const GlobalClientPersistentCache::CachedPixels* 
GlobalClientPersistentCache::get(const std::vector<uint8_t>& hash)
{
  auto it = cache_.find(hash);
  if (it == cache_.end()) {
    stats_.cacheMisses++;
    return nullptr;
  }
  
  stats_.cacheHits++;
  it->second.lastAccessTime = getCurrentTime();
  
  // ARC policy: move from T1 to T2 on second access
  auto listIt = listMap_.find(hash);
  if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
    moveToT2(hash);
  }
  
  return &(it->second);
}

void GlobalClientPersistentCache::insert(const std::vector<uint8_t>& hash,
                                         const uint8_t* pixels,
                                         const PixelFormat& pf,
                                         uint16_t width, uint16_t height,
                                         uint16_t stridePixels)
{
  if (pixels == nullptr || width == 0 || height == 0)
    return;
    
  size_t pixelDataSize = height * stridePixels * (pf.bpp / 8);
  
  // Check if already in cache
  auto cacheIt = cache_.find(hash);
  if (cacheIt != cache_.end()) {
    // Update existing entry
    cacheIt->second.lastAccessTime = getCurrentTime();
    
    // Move to T2 if currently in T1
    auto listIt = listMap_.find(hash);
    if (listIt != listMap_.end() && listIt->second.list == LIST_T1) {
      moveToT2(hash);
    }
    return;
  }
  
  // Check ghost lists (recently evicted)
  auto listIt = listMap_.find(hash);
  
  if (listIt != listMap_.end() && listIt->second.list == LIST_B1) {
    // Cache hit in B1: adapt by increasing p (favor recency)
    size_t delta = (b2_.size() >= b1_.size()) ? 1 : (b2_.size() / b1_.size());
    p_ = std::min(maxCacheSize_, p_ + delta * pixelDataSize);
    
    // Make room and insert into T2
    replace(hash, pixelDataSize);
    
    // Remove from B1
    b1_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else if (listIt != listMap_.end() && listIt->second.list == LIST_B2) {
    // Cache hit in B2: adapt by decreasing p (favor frequency)
    size_t delta = (b1_.size() >= b2_.size()) ? 1 : (b1_.size() / b2_.size());
    p_ = (delta * pixelDataSize > p_) ? 0 : p_ - delta * pixelDataSize;
    
    // Make room and insert into T2
    replace(hash, pixelDataSize);
    
    // Remove from B2
    b2_.erase(listIt->second.iter);
    listMap_.erase(listIt);
    
  } else {
    // New entry: make room and insert into T1
    replace(hash, pixelDataSize);
  }
  
  // Create the entry
  CachedPixels entry;
  entry.pixels.assign(pixels, pixels + pixelDataSize);
  entry.format = pf;
  entry.width = width;
  entry.height = height;
  entry.stridePixels = stridePixels;
  entry.lastAccessTime = getCurrentTime();
  
  // Insert into cache
  cache_[hash] = entry;
  
  // Determine which list to add to
  bool wasInGhost = (listIt != listMap_.end() && 
                     (listIt->second.list == LIST_B1 || listIt->second.list == LIST_B2));
  
  if (wasInGhost) {
    // Was in ghost list, add to T2
    t2_.push_front(hash);
    listMap_[hash].list = LIST_T2;
    listMap_[hash].iter = t2_.begin();
    t2Size_ += pixelDataSize;
  } else {
    // New entry, add to T1
    t1_.push_front(hash);
    listMap_[hash].list = LIST_T1;
    listMap_[hash].iter = t1_.begin();
    t1Size_ += pixelDataSize;
  }
  
  stats_.totalEntries++;
  stats_.totalBytes += pixelDataSize;
  
  vlog.debug("PersistentCache inserted: hashLen=%zu size=%zu bytes, rect=%dx%d, T1=%zu/%zu T2=%zu p=%zu",
             hash.size(), pixelDataSize, width, height,
             t1Size_, t1_.size(), t2_.size(), p_);
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
  cache_.clear();
  t1_.clear();
  t2_.clear();
  b1_.clear();
  b2_.clear();
  listMap_.clear();
  
  t1Size_ = 0;
  t2Size_ = 0;
  p_ = 0;
  
  stats_.totalEntries = 0;
  stats_.totalBytes = 0;
  
  vlog.debug("PersistentCache cleared");
}

GlobalClientPersistentCache::Stats 
GlobalClientPersistentCache::getStats() const
{
  Stats current = stats_;
  current.totalEntries = cache_.size();
  current.totalBytes = t1Size_ + t2Size_;
  current.t1Size = t1_.size();
  current.t2Size = t2_.size();
  current.b1Size = b1_.size();
  current.b2Size = b2_.size();
  current.targetT1Size = p_;
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
  
  // Evict if we're now over the limit
  while (t1Size_ + t2Size_ > maxCacheSize_) {
    if (!t1_.empty() || !t2_.empty()) {
      replace(std::vector<uint8_t>(), 0);  // Force eviction
    } else {
      break;
    }
  }
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
    if (t1Size_ + t2Size_ >= maxCacheSize_) {
      vlog.info("PersistentCache: reached max size, stopping load");
      break;
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
  header.totalBytes = t1Size_ + t2Size_;
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

// ============================================================================
// ARC Algorithm Implementation
// ============================================================================

void GlobalClientPersistentCache::replace(const std::vector<uint8_t>&, size_t size)
{
  // Make room for new entry of given size
  while (t1Size_ + t2Size_ + size > maxCacheSize_) {
    
    // Case 1: T1 is larger than target p, evict from T1
    if (!t1_.empty() && 
        (t1Size_ > p_ || (t2_.empty() && t1Size_ == p_))) {
      
      std::vector<uint8_t> victim = t1_.back();
      t1_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        size_t victimSize = cacheIt->second.byteSize();
        t1Size_ -= victimSize;
        
        // Add to pending evictions for server notification
        pendingEvictions_.push_back(victim);
        
        // Move to B1 (ghost list) - keep metadata but remove data
        b1_.push_front(victim);
        listMap_[victim].list = LIST_B1;
        listMap_[victim].iter = b1_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B1 size
        while (b1_.size() > maxCacheSize_ / (1024 * 16)) {  // ~16KB ghost entries
          std::vector<uint8_t> oldGhost = b1_.back();
          b1_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } 
    // Case 2: Evict from T2
    else if (!t2_.empty()) {
      
      std::vector<uint8_t> victim = t2_.back();
      t2_.pop_back();
      
      auto cacheIt = cache_.find(victim);
      if (cacheIt != cache_.end()) {
        size_t victimSize = cacheIt->second.byteSize();
        t2Size_ -= victimSize;
        
        // Add to pending evictions for server notification
        pendingEvictions_.push_back(victim);
        
        // Move to B2 (ghost list)
        b2_.push_front(victim);
        listMap_[victim].list = LIST_B2;
        listMap_[victim].iter = b2_.begin();
        
        cache_.erase(cacheIt);
        stats_.totalEntries--;
        stats_.evictions++;
        
        // Limit B2 size
        while (b2_.size() > maxCacheSize_ / (1024 * 16)) {
          std::vector<uint8_t> oldGhost = b2_.back();
          b2_.pop_back();
          listMap_.erase(oldGhost);
        }
      }
      
    } else {
      // Both lists empty, nothing to evict
      break;
    }
  }
}

void GlobalClientPersistentCache::moveToT2(const std::vector<uint8_t>& hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end() || listIt->second.list != LIST_T1)
    return;
    
  // Remove from T1
  t1_.erase(listIt->second.iter);
  size_t entrySize = getEntrySize(hash);
  t1Size_ -= entrySize;
  
  // Add to T2
  t2_.push_front(hash);
  listMap_[hash].list = LIST_T2;
  listMap_[hash].iter = t2_.begin();
  t2Size_ += entrySize;
}

void GlobalClientPersistentCache::moveToB1(const std::vector<uint8_t>& hash)
{
  removeFromList(hash);
  b1_.push_front(hash);
  listMap_[hash].list = LIST_B1;
  listMap_[hash].iter = b1_.begin();
}

void GlobalClientPersistentCache::moveToB2(const std::vector<uint8_t>& hash)
{
  removeFromList(hash);
  b2_.push_front(hash);
  listMap_[hash].list = LIST_B2;
  listMap_[hash].iter = b2_.begin();
}

void GlobalClientPersistentCache::removeFromList(const std::vector<uint8_t>& hash)
{
  auto listIt = listMap_.find(hash);
  if (listIt == listMap_.end())
    return;
    
  switch (listIt->second.list) {
    case LIST_T1:
      t1_.erase(listIt->second.iter);
      t1Size_ -= getEntrySize(hash);
      break;
    case LIST_T2:
      t2_.erase(listIt->second.iter);
      t2Size_ -= getEntrySize(hash);
      break;
    case LIST_B1:
      b1_.erase(listIt->second.iter);
      break;
    case LIST_B2:
      b2_.erase(listIt->second.iter);
      break;
    case LIST_NONE:
      break;
  }
  
  listMap_.erase(listIt);
}

size_t GlobalClientPersistentCache::getEntrySize(const std::vector<uint8_t>& hash) const
{
  auto it = cache_.find(hash);
  if (it != cache_.end()) {
    return it->second.byteSize();
  }
  return 0;
}

uint32_t GlobalClientPersistentCache::getCurrentTime() const
{
  return (uint32_t)time(nullptr);
}
