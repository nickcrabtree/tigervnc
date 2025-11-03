/* Copyright (C) 2025 TigerVNC Team.  All Rights Reserved.
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

#ifndef __RFB_CONTENTHASH_H__
#define __RFB_CONTENTHASH_H__

#include <stdint.h>
#include <cstring>
#include <vector>

#include <openssl/evp.h>

#include <core/Rect.h>
#include <rfb/PixelBuffer.h>

namespace rfb {

  // ContentHash: Stable content-based hashing for PersistentCache protocol
  //
  // Uses SHA-256 truncated to 128 bits (16 bytes) for:
  // - Low collision probability (< 0.0001% for 100K entries)
  // - Cross-platform stability
  // - Fast computation
  class ContentHash {
  public:
    // Compute hash over raw byte data
    static std::vector<uint8_t> compute(const uint8_t* data, size_t len) {
      std::vector<uint8_t> hash(16);

      EVP_MD_CTX* ctx = EVP_MD_CTX_new();
      if (!ctx)
        return hash;

      if (EVP_DigestInit_ex(ctx, EVP_sha256(), nullptr) != 1) {
        EVP_MD_CTX_free(ctx);
        return hash;
      }

      EVP_DigestUpdate(ctx, data, len);

      uint8_t full_hash[32];
      unsigned int hash_len;
      EVP_DigestFinal_ex(ctx, full_hash, &hash_len);
      EVP_MD_CTX_free(ctx);

      // Truncate to 16 bytes
      memcpy(hash.data(), full_hash, 16);
      return hash;
    }

    // Compute hash for a rectangle region in a PixelBuffer
    // CRITICAL: Handles stride-in-pixels correctly (multiply by bytesPerPixel)
    static std::vector<uint8_t> computeRect(const PixelBuffer* pb,
                                           const core::Rect& r) {
      int stride;
      const uint8_t* pixels = pb->getBuffer(r, &stride);

      int bytesPerPixel = pb->getPF().bpp / 8;
      size_t rowBytes = r.width() * bytesPerPixel;
      size_t strideBytes = stride * bytesPerPixel;  // CRITICAL: multiply by bytesPerPixel!

      // Hash row-major pixel data (only the actual pixels, not padding)
      EVP_MD_CTX* ctx = EVP_MD_CTX_new();
      if (!ctx)
        return std::vector<uint8_t>(16);

      if (EVP_DigestInit_ex(ctx, EVP_sha256(), nullptr) != 1) {
        EVP_MD_CTX_free(ctx);
        return std::vector<uint8_t>(16);
      }

      for (int y = 0; y < r.height(); y++) {
        const uint8_t* row = pixels + (y * strideBytes);
        EVP_DigestUpdate(ctx, row, rowBytes);
      }

      uint8_t full_hash[32];
      unsigned int hash_len;
      EVP_DigestFinal_ex(ctx, full_hash, &hash_len);
      EVP_MD_CTX_free(ctx);

      std::vector<uint8_t> hash(16);
      memcpy(hash.data(), full_hash, 16);
      return hash;
    }

    // Hash vector hasher for use with unordered containers
    struct HashVectorHasher {
      size_t operator()(const std::vector<uint8_t>& v) const {
        // Simple FNV-1a hash for vector<uint8_t>
        size_t hash = 14695981039346656037ULL;
        for (uint8_t byte : v) {
          hash ^= byte;
          hash *= 1099511628211ULL;
        }
        return hash;
      }
    };
  };

}  // namespace rfb

#endif
