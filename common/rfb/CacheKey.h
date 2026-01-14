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
#include <array>
#include <cstring>

namespace rfb {

  // 16-byte content-addressable cache key.
//
// This key is the canonical protocol identity for cached rectangles.
// It is computed as a stable 128-bit hash over a canonical 32bpp/24-depth
// true-colour pixel stream, including width/height in the hashed domain.
//
// NOTE: Width/height are intentionally NOT stored here. They are already
// included in the hash domain and are carried separately where required
// (e.g. for protocol messages and debugging).
struct CacheKey {
  std::array<uint8_t, 16> bytes;

  CacheKey() { bytes.fill(0); }

  explicit CacheKey(const uint8_t* p) {
    if (p) std::memcpy(bytes.data(), p, 16);
    else bytes.fill(0);
  }

  bool operator==(const CacheKey& other) const {
    return bytes == other.bytes;
  }
};

// Hash function for unordered_map.
// Mix two 64-bit lanes with MurmurHash3 finalizers.
struct CacheKeyHash {
  std::size_t operator()(const CacheKey& key) const {
    uint64_t a = 0, b = 0;
    std::memcpy(&a, key.bytes.data(), 8);
    std::memcpy(&b, key.bytes.data() + 8, 8);
    uint64_t v = a ^ (b * 0x9e3779b97f4a7c15ULL);
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
