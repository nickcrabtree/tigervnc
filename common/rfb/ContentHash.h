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

    // Compute hash for a rectangle region in a PixelBuffer.
    //
    // IMPORTANT: To guarantee that server and client compute identical
    // hashes for the same visual content, this helper always hashes a
    // canonical pixel representation (32bpp little-endian true-colour)
    // regardless of the underlying PixelFormat of the PixelBuffer.
    //
    // The hashing domain is therefore the tightly packed canonical
    // pixel stream only:
    //   MD5( canonical_pixels )
    // where canonical_pixels is produced via PixelFormat::bufferFromBuffer().
    // Dimensions are not included in the hash input; callers that need a
    // stronger identity use (width, height, hash) as a composite key.
    static std::vector<uint8_t> computeRect(const PixelBuffer* pb,
                                           const core::Rect& r) {
      if (!pb)
        return std::vector<uint8_t>(16);

      if (r.width() <= 0 || r.height() <= 0)
        return std::vector<uint8_t>(16);

      // Canonical 32bpp, 24-bit depth, little-endian true-colour format.
      // Masks/shifts correspond to 0x00RRGGBB layout in native byte order
      // (blue in least-significant byte).
      static const PixelFormat canonicalPF(32, 24,
                                           false,  // little-endian buffer
                                           true,   // trueColour
                                           255, 255, 255,  // red/green/blue max
                                           16, 8, 0);      // R,G,B shifts

      uint32_t width = r.width();
      uint32_t height = r.height();

      const int bytesPerPixel = canonicalPF.bpp / 8; // 4
      const size_t rowBytes = static_cast<size_t>(width) * bytesPerPixel;

      // Allocate buffer for a tightly packed canonical pixel stream.
      std::vector<uint8_t> buf;
      try {
        buf.resize(static_cast<size_t>(height) * rowBytes);
      } catch (...) {
        // On allocation failure, return a zeroed hash vector.
        return std::vector<uint8_t>(16);
      }

      // Convert the rectangle to the canonical pixel format into the
      // buffer.
      uint8_t* pixelDst = buf.data();
      try {
        // Destination stride is in pixels; use exact width so the
        // canonical representation is tightly packed.
        pb->getImage(canonicalPF, pixelDst, r, static_cast<int>(width));
      } catch (...) {
        // Any out_of_range or other exceptions indicate a programming
        // error in the caller; return a zeroed hash to avoid UB.
        return std::vector<uint8_t>(16);
      }

      // Normalise the unused/padding byte in our 32bpp representation so
      // that differences in how various PixelFormats populate the top 8 bits
      // (e.g. 0x00 vs 0xff) do not affect the hash. Only the low 24 bits of
      // each pixel (R,G,B) carry semantic information for our purposes.
      const int bppBytes = canonicalPF.bpp / 8; // expected 4
      if (bppBytes == 4) {
        const size_t totalPixels = static_cast<size_t>(width) * height;
        for (size_t i = 0; i < totalPixels; ++i) {
          pixelDst[i * 4 + 3] = 0;
        }
      }

      // Delegate to the generic byte hashing helper, which will use
      // MD5 via GnuTLS when available, or the FNV-1a-style fallback.
      return compute(buf.data(), buf.size());
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

    // Detect bordered content regions in the framebuffer.
    // A bordered region is a rectangle surrounded by a border of constant color
    // (at least minBorderWidth pixels wide on all sides).
    // This is useful for finding application content areas like slides,
    // document views, etc. that are embedded in a UI with solid-colored borders.
    //
    // Returns a list of detected regions sorted by area (largest first).
    // Only regions with area >= minArea are returned.
    struct BorderedRegion {
      core::Rect contentRect;  // The inner content rectangle
      core::Rect outerRect;    // The outer rectangle including border
      uint32_t borderColor;    // The border color (in native format)
      int borderLeft, borderRight, borderTop, borderBottom;  // Border widths
    };

    static std::vector<BorderedRegion> detectBorderedRegions(
        const PixelBuffer* pb,
        int minBorderWidth = 5,
        int minArea = 50000);
  };

}  // namespace rfb

#endif
