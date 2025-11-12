#ifndef __RFB_CACHE_SERVERHASHSET_H__
#define __RFB_CACHE_SERVERHASHSET_H__

#include <unordered_set>
#include <functional>

namespace rfb {

  /**
   * ServerHashSet - Shared utility for tracking client-known cache keys
   * 
   * Used by both ContentCache (uint64_t IDs) and PersistentCache (hash vectors)
   * to maintain server-side knowledge of what the client has cached.
   * 
   * Template parameters:
   *   KeyType: uint64_t for ContentCache, std::vector<uint8_t> for PersistentCache
   *   Hasher: Hash function for KeyType (optional, uses std::hash by default)
   */
  template<typename KeyType, typename Hasher = std::hash<KeyType>>
  class ServerHashSet {
  public:
    ServerHashSet() : knownCount_(0), evictedCount_(0) {}
    ~ServerHashSet() {}

    /**
     * Check if client knows this key
     */
    bool has(const KeyType& key) const {
      return knownKeys_.find(key) != knownKeys_.end();
    }

    /**
     * Add key to known set (when server sends init/data to client)
     */
    void add(const KeyType& key) {
      if (knownKeys_.insert(key).second) {
        knownCount_++;
      }
    }

    /**
     * Remove key from known set (when client sends eviction notification)
     * Returns true if key was present
     */
    bool remove(const KeyType& key) {
      if (knownKeys_.erase(key) > 0) {
        evictedCount_++;
        return true;
      }
      return false;
    }

    /**
     * Remove multiple keys from known set
     * Returns number of keys actually removed
     */
    size_t removeMultiple(const std::vector<KeyType>& keys) {
      size_t removed = 0;
      for (const auto& key : keys) {
        if (remove(key)) {
          removed++;
        }
      }
      return removed;
    }

    /**
     * Clear all known keys
     */
    void clear() {
      knownKeys_.clear();
      knownCount_ = 0;
      evictedCount_ = 0;
    }

    /**
     * Get current size of known set
     */
    size_t size() const {
      return knownKeys_.size();
    }

    /**
     * Statistics
     */
    struct Stats {
      size_t currentSize;     // Current number of known keys
      uint64_t totalAdded;    // Cumulative count of keys added
      uint64_t totalEvicted;  // Cumulative count of keys evicted
    };

    Stats getStats() const {
      Stats s;
      s.currentSize = knownKeys_.size();
      s.totalAdded = knownCount_;
      s.totalEvicted = evictedCount_;
      return s;
    }

  private:
    std::unordered_set<KeyType, Hasher> knownKeys_;
    uint64_t knownCount_;    // Total keys ever added
    uint64_t evictedCount_;  // Total keys ever evicted
  };

} // namespace rfb

#endif // __RFB_CACHE_SERVERHASHSET_H__
