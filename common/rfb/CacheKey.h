/* Unified cache key for ContentCache/PersistentCache
 *
 * This header defines the shared key type used by the unified cache engine
 * on both server and client. It replaces the old ContentKey/ContentKeyHash
 * types that used to live inside ContentCache.
 */

#ifndef __RFB_CACHE_KEY_H__
#define __RFB_CACHE_KEY_H__

#include <stdint.h>
#include <cstddef>
#include <functional>

namespace rfb {

  // 12-byte composite cache key
  struct CacheKey {
    uint16_t width;       // Rectangle width (2 bytes)
    uint16_t height;      // Rectangle height (2 bytes)
    uint64_t contentHash; // Content hash (8 bytes)

    CacheKey() : width(0), height(0), contentHash(0) {}

    CacheKey(uint16_t w, uint16_t h, uint64_t hash)
      : width(w), height(h), contentHash(hash) {}

    bool operator==(const CacheKey& other) const {
      return width == other.width &&
             height == other.height &&
             contentHash == other.contentHash;
    }
  };

  // Hash function for unordered_map (bit-packing, no magic primes)
  struct CacheKeyHash {
    std::size_t operator()(const CacheKey& key) const {
      // Pack the fields into a 64-bit value in a stable way.
      uint64_t v = 0;
      v ^= static_cast<uint64_t>(key.width) << 48;
      v ^= static_cast<uint64_t>(key.height) << 32;
      v ^= key.contentHash;
      // Final mix (xorshift)
      v ^= v >> 33;
      v *= 0xff51afd7ed558ccdULL;
      v ^= v >> 33;
      v *= 0xc4ceb9fe1a85ec53ULL;
      v ^= v >> 33;
      return static_cast<std::size_t>(v);
    }
  };

} // namespace rfb

#endif // __RFB_CACHE_KEY_H__
