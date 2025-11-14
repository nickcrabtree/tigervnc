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

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#ifdef HAVE_GNUTLS
#include <gnutls/gnutls.h>
#include <gnutls/crypto.h>
#endif

#include <core/Rect.h>
#include <rfb/PixelBuffer.h>

namespace rfb {

  // ContentHash: Stable content-based hashing for cache protocols
  //
  // Primary implementation: MD5 via GnuTLS (when available).
  // Fallback: simple FNV-1a-style hash (non-cryptographic but stable).
  class ContentHash {
  public:
    // Compute hash over raw byte data
    static std::vector<uint8_t> compute(const uint8_t* data, size_t len) {
      std::vector<uint8_t> hash(16);
      if (!data || len == 0)
        return hash;

#ifdef HAVE_GNUTLS
      gnutls_hash_hd_t ctx;
      if (gnutls_hash_init(&ctx, GNUTLS_DIG_MD5) != 0)
        return hash;

      gnutls_hash(ctx, data, len);
      gnutls_hash_deinit(ctx, hash.data());
      return hash;
#else
      // Fallback: simple FNV-1a variant into 128 bits
      const uint64_t FNV_OFFSET = 0xcbf29ce484222325ULL;
      const uint64_t FNV_PRIME  = 0x100000001b3ULL;
      uint64_t h1 = FNV_OFFSET;
      uint64_t h2 = FNV_OFFSET ^ 0x123456789abcdef0ULL;
      for (size_t i = 0; i < len; ++i) {
        h1 ^= data[i]; h1 *= FNV_PRIME;
        h2 ^= data[i]; h2 *= FNV_PRIME;
      }
      memcpy(hash.data(), &h1, 8);
      memcpy(hash.data() + 8, &h2, 8);
      return hash;
#endif
    }

    // Compute hash for a rectangle region in a PixelBuffer
    // Handles stride-in-pixels correctly and includes width/height.
    static std::vector<uint8_t> computeRect(const PixelBuffer* pb,
                                           const core::Rect& r) {
      int stride;
      const uint8_t* pixels = pb->getBuffer(r, &stride);
      if (!pixels)
        return std::vector<uint8_t>(16);

      int bytesPerPixel = pb->getPF().bpp / 8;
      size_t rowBytes = r.width() * bytesPerPixel;
      size_t strideBytes = stride * bytesPerPixel;

#ifdef HAVE_GNUTLS
      gnutls_hash_hd_t ctx;
      if (gnutls_hash_init(&ctx, GNUTLS_DIG_MD5) != 0)
        return std::vector<uint8_t>(16);

      // Include dimensions
      uint32_t width = r.width();
      uint32_t height = r.height();
      gnutls_hash(ctx, &width, sizeof(width));
      gnutls_hash(ctx, &height, sizeof(height));

      for (int y = 0; y < r.height(); y++) {
        const uint8_t* row = pixels + (y * strideBytes);
        gnutls_hash(ctx, row, rowBytes);
      }

      std::vector<uint8_t> hash(16);
      gnutls_hash_deinit(ctx, hash.data());
      return hash;
#else
      // Fallback: build a temporary buffer of the exact rect bytes
      std::vector<uint8_t> tmp;
      tmp.reserve(sizeof(uint32_t) * 2 + (size_t)r.height() * rowBytes);
      uint32_t width = r.width();
      uint32_t height = r.height();
      tmp.insert(tmp.end(), reinterpret_cast<uint8_t*>(&width),
                 reinterpret_cast<uint8_t*>(&width) + sizeof(width));
      tmp.insert(tmp.end(), reinterpret_cast<uint8_t*>(&height),
                 reinterpret_cast<uint8_t*>(&height) + sizeof(height));
      for (int y = 0; y < r.height(); y++) {
        const uint8_t* row = pixels + (y * strideBytes);
        tmp.insert(tmp.end(), row, row + rowBytes);
      }
      return compute(tmp.data(), tmp.size());
#endif
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
