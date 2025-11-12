// ArcCache - Adaptive Replacement Cache (ARC) implementation (header-only)
//
// Template parameters:
//   Key      - key type (must be hashable in unordered_map; provide custom Hasher/Eq if needed)
//   Entry    - value type stored in cache
//   Hasher   - hasher for Key (defaults to std::hash<Key>)
//   Eq       - equality for Key (defaults to std::equal_to<Key>)
//
// Notes:
// - Capacity is specified in bytes. Provide a size function that returns the byte
//   size for a given Entry instance.
// - Thread safety: none. Caller must ensure external synchronization if used
//   from multiple threads.
// - This implementation follows the core ARC promotion/eviction rules with a
//   byte-based capacity. Ghost lists (B1/B2) store keys only and track recency
//   of evicted items to adapt target size p_.
//
#pragma once

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <list>
#include <stdexcept>
#include <unordered_map>

namespace rfb { namespace cache {

enum class ArcList { NONE, T1, T2, B1, B2 };

template <typename Key, typename Entry,
          typename Hasher = std::hash<Key>,
          typename Eq = std::equal_to<Key>>
class ArcCache {
public:
  using ByteSizeFunc = std::function<size_t(const Entry&)>;
  using EvictionCallback = std::function<void(const Key&)>;

  struct Stats {
    size_t totalEntries = 0;
    size_t totalBytes = 0;
    uint64_t cacheHits = 0;
    uint64_t cacheMisses = 0;
    uint64_t evictions = 0;
    size_t t1Size = 0;
    size_t t2Size = 0;
    size_t b1Size = 0;
    size_t b2Size = 0;
    size_t targetT1Size = 0; // p_
  };

  ArcCache(size_t maxBytes, ByteSizeFunc sizeFunc, EvictionCallback evictCb = nullptr)
    : maxBytes_(maxBytes), currentBytes_(0), pBytes_(0),
      sizeFunc_(std::move(sizeFunc)), evictCb_(std::move(evictCb)) {
    if (!sizeFunc_) throw std::invalid_argument("ArcCache: sizeFunc must be provided");
  }

  void clear() {
    t1_.clear(); t2_.clear(); b1_.clear(); b2_.clear();
    listMap_.clear();
    for (auto &kv : cache_) {
      (void)kv; // no-op; let cache_ be cleared
    }
    cache_.clear();
    currentBytes_ = 0;
    pBytes_ = 0;
  }

  bool has(const Key& key) const {
    return cache_.find(key) != cache_.end();
  }

  // Returns pointer to entry if present (promotes to T2), nullptr otherwise
  const Entry* get(const Key& key) {
    auto it = cache_.find(key);
    if (it == cache_.end()) {
      stats_.cacheMisses++;
      return nullptr;
    }

    // Promote to T2
    moveToList(key, ArcList::T2);
    stats_.cacheHits++;
    return &it->second;
  }

  // Insert or update entry. May evict multiple entries to satisfy capacity.
  void insert(const Key& key, Entry entry) {
    size_t sz = sizeFunc_(entry);
    if (sz > maxBytes_) {
      // Item larger than cache, drop it and do not cache
      stats_.cacheMisses++;
      return;
    }

    auto inCache = cache_.find(key);
    if (inCache != cache_.end()) {
      // Update existing, adjust size and promote to T2
      size_t old = sizeFunc_(inCache->second);
      inCache->second = std::move(entry);
      currentBytes_ = currentBytes_ - old + sz;
      moveToList(key, ArcList::T2);
      return;
    }

    // Check ghost lists hits
    auto lmIt = listMap_.find(key);
    bool inB1 = (lmIt != listMap_.end() && lmIt->second.list == ArcList::B1);
    bool inB2 = (lmIt != listMap_.end() && lmIt->second.list == ArcList::B2);

    if (inB1) {
      // Increase p towards maxBytes_
      size_t delta = b1_.empty() ? 1 : (b2_.size() / b1_.size());
      pBytes_ = std::min(maxBytes_, pBytes_ + std::max<size_t>(1, delta));
      replace(sz);
      // Move key to T2 from B1
      eraseFromGhostList(key);
      addToListFront(key, ArcList::T2);
      cache_.emplace(key, std::move(entry));
      currentBytes_ += sz;
      stats_.totalEntries = cache_.size();
      stats_.totalBytes = currentBytes_;
      return;
    }

    if (inB2) {
      // Decrease p towards 0
      size_t delta = b2_.empty() ? 1 : (b1_.size() / b2_.size());
      if (pBytes_ > 0)
        pBytes_ = pBytes_ - std::min(pBytes_, std::max<size_t>(1, delta));
      replace(sz);
      // Move key to T2 from B2
      eraseFromGhostList(key);
      addToListFront(key, ArcList::T2);
      cache_.emplace(key, std::move(entry));
      currentBytes_ += sz;
      stats_.totalEntries = cache_.size();
      stats_.totalBytes = currentBytes_;
      return;
    }

    // Non-resident miss: ensure space and insert into T1
    if (currentBytes_ + sz > maxBytes_) {
      replace(sz);
    }

    addToListFront(key, ArcList::T1);
    cache_.emplace(key, std::move(entry));
    currentBytes_ += sz;
    stats_.cacheMisses++;
    stats_.totalEntries = cache_.size();
    stats_.totalBytes = currentBytes_;
  }

  Stats getStats() const {
    Stats s = stats_;
    s.t1Size = t1_.size();
    s.t2Size = t2_.size();
    s.b1Size = b1_.size();
    s.b2Size = b2_.size();
    s.targetT1Size = pBytes_;
    return s;
  }

private:
  struct ListInfo {
    ArcList list = ArcList::NONE;
    typename std::list<Key>::iterator iter;
  };

  // Lists
  std::list<Key> t1_, t2_, b1_, b2_;
  std::unordered_map<Key, ListInfo, Hasher, Eq> listMap_;

  // Storage
  std::unordered_map<Key, Entry, Hasher, Eq> cache_;

  // Capacities
  size_t maxBytes_;
  size_t currentBytes_;
  size_t pBytes_; // adaptive target for T1 (in bytes)

  // Utilities
  ByteSizeFunc sizeFunc_;
  EvictionCallback evictCb_;
  Stats stats_{};

  void moveToList(const Key& key, ArcList dst) {
    auto it = listMap_.find(key);
    if (it == listMap_.end()) return; // not tracked (shouldn't happen for cache resident)

    // Remove from current list
    removeFromList(it->first, it->second.list);

    // Add to front of destination
    addToListFront(key, dst);
  }

  void addToListFront(const Key& key, ArcList which) {
    switch (which) {
      case ArcList::T1: {
        t1_.push_front(key);
        auto& li = listMap_[key]; li.list = which; li.iter = t1_.begin();
        break; }
      case ArcList::T2: {
        t2_.push_front(key);
        auto& li = listMap_[key]; li.list = which; li.iter = t2_.begin();
        break; }
      case ArcList::B1: {
        b1_.push_front(key);
        auto& li = listMap_[key]; li.list = which; li.iter = b1_.begin();
        break; }
      case ArcList::B2: {
        b2_.push_front(key);
        auto& li = listMap_[key]; li.list = which; li.iter = b2_.begin();
        break; }
      default: break;
    }
  }

  void removeFromList(const Key& key, ArcList which) {
    auto it = listMap_.find(key);
    if (it == listMap_.end()) return;
    if (it->second.list != which) return;

    switch (which) {
      case ArcList::T1: t1_.erase(it->second.iter); break;
      case ArcList::T2: t2_.erase(it->second.iter); break;
      case ArcList::B1: b1_.erase(it->second.iter); break;
      case ArcList::B2: b2_.erase(it->second.iter); break;
      default: break;
    }
    it->second.list = ArcList::NONE;
  }

  void eraseFromGhostList(const Key& key) {
    auto it = listMap_.find(key);
    if (it == listMap_.end()) return;
    if (it->second.list == ArcList::B1) {
      b1_.erase(it->second.iter);
      listMap_.erase(it);
    } else if (it->second.list == ArcList::B2) {
      b2_.erase(it->second.iter);
      listMap_.erase(it);
    }
  }

  void replace(size_t incomingSize) {
    // Evict until we have room for incomingSize
    // Choose from T1 or T2 depending on pBytes_ target and ghost pressure.
    while (currentBytes_ + incomingSize > maxBytes_) {
      bool evictT1 = false;

      if (!t1_.empty() && (bytesOf(t1_) > pBytes_)) {
        evictT1 = true;
      } else if (t1_.empty() && !t2_.empty()) {
        evictT1 = false;
      } else if (!t2_.empty() && (bytesOf(t1_) <= pBytes_)) {
        evictT1 = false;
      } else if (!t1_.empty()) {
        evictT1 = true;
      }

      if (evictT1) {
        // Evict LRU from T1 -> B1 ghost
        const Key victim = t1_.back();
        t1_.pop_back();
        auto cit = cache_.find(victim);
        if (cit != cache_.end()) {
          size_t vsz = sizeFunc_(cit->second);
          currentBytes_ -= vsz;
          if (evictCb_) evictCb_(victim);
          cache_.erase(cit);
          stats_.evictions++;
          stats_.totalEntries = cache_.size();
          stats_.totalBytes = currentBytes_;
        }
        // Track ghost in B1
        listMap_.erase(victim);
        addToListFront(victim, ArcList::B1);
      } else {
        // Evict LRU from T2 -> B2 ghost
        const Key victim = t2_.back();
        t2_.pop_back();
        auto cit = cache_.find(victim);
        if (cit != cache_.end()) {
          size_t vsz = sizeFunc_(cit->second);
          currentBytes_ -= vsz;
          if (evictCb_) evictCb_(victim);
          cache_.erase(cit);
          stats_.evictions++;
          stats_.totalEntries = cache_.size();
          stats_.totalBytes = currentBytes_;
        }
        // Track ghost in B2
        listMap_.erase(victim);
        addToListFront(victim, ArcList::B2);
      }

      // Trim ghost lists if they grow too large relative to cache size
      trimGhosts();
    }
  }

  void trimGhosts() {
    const size_t maxGhost = 4 * (t1_.size() + t2_.size() + 1);
    while (b1_.size() > maxGhost) {
      const Key k = b1_.back(); b1_.pop_back(); listMap_.erase(k);
    }
    while (b2_.size() > maxGhost) {
      const Key k = b2_.back(); b2_.pop_back(); listMap_.erase(k);
    }
  }

  size_t bytesOf(const std::list<Key>& lst) const {
    // Approximate: sum bytes of resident entries only; ghost lists return 0
    if (&lst == &t1_ || &lst == &t2_) {
      size_t sum = 0;
      for (const auto& k : lst) {
        auto it = cache_.find(k);
        if (it != cache_.end()) sum += sizeFunc_(it->second);
      }
      return sum;
    }
    return 0;
  }
};

}} // namespace rfb::cache
