/* Test viewer-managed dual-hash cache flow (NEW DESIGN)
 *
 * This test simulates the new dual-hash design:
 * 1. Server computes canonical hash and sends JPEG-encoded data
 * 2. Viewer decodes, computes lossy hash, stores entry with BOTH hashes
 * 3. Viewer reports hash type (LOSSY) to server
 * 4. Server re-encounters same content, sends reference with CANONICAL hash
 * 5. Viewer looks up by CANONICAL hash and should HIT (finds lossy entry)
 */

#include <rfb/ContentHash.h>
#include <rfb/PixelBuffer.h>
#include <rfb/PixelFormat.h>
#include <rfb/CacheKey.h>
#include <rfb/JpegCompressor.h>
#include <rfb/JpegDecompressor.h>

#include <stdio.h>
#include <string.h>
#include <vector>
#include <unordered_map>

using namespace rfb;

// Hash type enum (matches protocol)
enum HashType {
  HASH_TYPE_NONE = 0,
  HASH_TYPE_LOSSY = 1,
  HASH_TYPE_LOSSLESS = 2
};

// Simulated server state (NEW: no lossy mapping, just tracks what viewer has)
struct SimulatedServer {
  std::unordered_map<uint64_t, HashType> viewerCache; // canonical → quality
  
  void receiveHashTypeReport(uint64_t canonical, HashType type) {
    printf("   [Server] Received hash type report: canonical=0x%016llx, type=%s\n",
           (unsigned long long)canonical,
           type == HASH_TYPE_LOSSLESS ? "LOSSLESS" :
           type == HASH_TYPE_LOSSY ? "LOSSY" : "NONE");
    viewerCache[canonical] = type;
  }
  
  bool viewerHasContent(uint64_t canonical) {
    auto it = viewerCache.find(canonical);
    return (it != viewerCache.end() && it->second != HASH_TYPE_NONE);
  }
};

// Simulated viewer cache entry (NEW: stores both hashes)
struct CacheEntry {
  uint64_t canonicalHash;
  uint64_t actualHash;
  std::vector<uint8_t> pixels;
  
  CacheEntry(uint64_t canonical, uint64_t actual, const uint8_t* data, size_t size)
    : canonicalHash(canonical), actualHash(actual), pixels(data, data + size) {}
};

// Simulated viewer cache (NEW: dual-hash storage and lookup)
struct SimulatedCache {
  std::unordered_map<CacheKey, CacheEntry, CacheKeyHash> cache;
  
  void store(uint16_t width, uint16_t height, uint64_t canonical, uint64_t actual,
             const uint8_t* pixels, size_t size) {
    // Index by actual hash (fast direct lookup)
    CacheKey key(width, height, actual);
    printf("   [Viewer] Storing entry: canonical=0x%016llx, actual=0x%016llx\n",
           (unsigned long long)canonical, (unsigned long long)actual);
    printf("           Indexed by: CacheKey(w=%u, h=%u, id=0x%016llx)\n",
           key.width, key.height, (unsigned long long)key.contentHash);
    cache.emplace(key, CacheEntry(canonical, actual, pixels, size));
  }
  
  // NEW: Lookup by canonical hash (server sends this)
  HashType lookupByCanonical(uint16_t width, uint16_t height, uint64_t canonical) {
    printf("   [Viewer] Looking up by canonical: 0x%016llx\n", (unsigned long long)canonical);
    
    // Search all entries for matching canonical hash
    for (const auto& pair : cache) {
      const CacheKey& key = pair.first;
      const CacheEntry& entry = pair.second;
      
      if (key.width == width && key.height == height && entry.canonicalHash == canonical) {
        printf("           Found entry: actual=0x%016llx\n", (unsigned long long)entry.actualHash);
        
        if (entry.actualHash == canonical) {
          printf("           → LOSSLESS HIT\n");
          return HASH_TYPE_LOSSLESS;
        } else {
          printf("           → LOSSY HIT\n");
          return HASH_TYPE_LOSSY;
        }
      }
    }
    
    printf("           → MISS\n");
    return HASH_TYPE_NONE;
  }
};

// Create test pattern
void fillTestPattern(uint8_t* buffer, int width, int height, int stride) {
  for (int y = 0; y < height; y++) {
    uint8_t* row = buffer + y * stride * 4;
    for (int x = 0; x < width; x++) {
      uint8_t* pixel = row + x * 4;
      pixel[0] = (x * 255) / width;
      pixel[1] = (y * 255) / height;
      pixel[2] = 128;
      pixel[3] = 255;
    }
  }
}

// Compute hash
uint64_t computeHash(const uint8_t* buffer, int width, int height, int stride, const PixelFormat& pf) {
  ManagedPixelBuffer pb(pf, width, height);
  pb.imageRect(pf, pb.getRect(), buffer, stride);
  
  std::vector<uint8_t> hash = ContentHash::computeRect(&pb, pb.getRect());
  if (hash.empty()) return 0;
  
  uint64_t hashId = 0;
  size_t n = std::min(hash.size(), sizeof(uint64_t));
  memcpy(&hashId, hash.data(), n);
  return hashId;
}

int main() {
  printf("Testing viewer-managed dual-hash cache flow (NEW DESIGN)...\n\n");
  
  // Setup
  const int width = 128;
  const int height = 128;
  const int stride = width;
  PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  SimulatedServer server;
  SimulatedCache viewerCache;
  
  // Create test content
  std::vector<uint8_t> framebuffer(height * stride * 4);
  fillTestPattern(framebuffer.data(), width, height, stride);
  
  printf("=== FIRST OCCURRENCE (seed) ===\n\n");
  
  // Step 1: Server computes canonical hash
  uint64_t canonicalHash = computeHash(framebuffer.data(), width, height, stride, pf);
  printf("1. [Server] Framebuffer canonical hash: 0x%016llx\n", (unsigned long long)canonicalHash);
  
  // Step 2: Server compresses with JPEG and sends to viewer
  printf("\n2. [Server] Encoding with JPEG and sending to viewer...\n");
  JpegCompressor compressor;
  compressor.compress(framebuffer.data(), stride, core::Rect(0, 0, width, height), pf, 75, 422);
  size_t compressedSize = compressor.length();
  std::vector<uint8_t> jpegData(compressedSize);
  memcpy(jpegData.data(), compressor.data(), compressedSize);
  printf("   Compressed: %zu bytes\n", compressedSize);
  
  // Step 3: Viewer receives and decodes
  printf("\n3. [Viewer] Decoding received JPEG data...\n");
  std::vector<uint8_t> decodedPixels(height * stride * 4);
  JpegDecompressor decompressor;
  decompressor.decompress(jpegData.data(), compressedSize, decodedPixels.data(),
                          stride, core::Rect(0, 0, width, height), pf);
  
  // Step 4: Viewer computes lossy hash
  uint64_t lossyHash = computeHash(decodedPixels.data(), width, height, stride, pf);
  printf("   Computed lossy hash: 0x%016llx\n", (unsigned long long)lossyHash);
  
  if (lossyHash == canonicalHash) {
    printf("   ✗ ERROR: Lossy hash matches canonical (JPEG should produce different hash!)\n");
    return 1;
  }
  
  // Step 5: Viewer stores with BOTH hashes
  printf("\n4. [Viewer] Storing decoded pixels with BOTH canonical and actual hash...\n");
  viewerCache.store(width, height, canonicalHash, lossyHash, decodedPixels.data(), decodedPixels.size());
  
  // Step 6: Viewer reports hash type to server
  printf("\n5. [Viewer] Reporting hash type (LOSSY) to server...\n");
  server.receiveHashTypeReport(canonicalHash, HASH_TYPE_LOSSY);
  
  printf("\n=== SECOND OCCURRENCE (should be cache hit) ===\n\n");
  
  // Step 7: Server re-encounters same content
  printf("6. [Server] Re-encountering same framebuffer content...\n");
  uint64_t canonicalHash2 = computeHash(framebuffer.data(), width, height, stride, pf);
  printf("   Canonical hash: 0x%016llx\n", (unsigned long long)canonicalHash2);
  
  if (canonicalHash2 != canonicalHash) {
    printf("   ✗ ERROR: Canonical hash changed (content should be identical!)\n");
    return 1;
  }
  printf("   ✓ Canonical hash matches (content is identical)\n");
  
  // Step 8: Server checks if viewer has content
  printf("\n7. [Server] Checking if viewer has content...\n");
  if (!server.viewerHasContent(canonicalHash2)) {
    printf("   ✗ ERROR: Server doesn't know viewer has content (but viewer reported it!)\n");
    return 1;
  }
  printf("   ✓ Server knows viewer has this content (quality: LOSSY)\n");
  
  // Step 9: Server sends reference with CANONICAL hash (NEW: not lossy hash!)
  printf("\n8. [Server] Sending PersistentCachedRect reference with CANONICAL hash...\n");
  printf("   Reference ID: 0x%016llx (canonical)\n", (unsigned long long)canonicalHash2);
  
  // Step 10: Viewer looks up by CANONICAL hash
  printf("\n9. [Viewer] Looking up by canonical hash (NEW: finds lossy entry!)...\n");
  HashType result = viewerCache.lookupByCanonical(width, height, canonicalHash2);
  
  if (result == HASH_TYPE_NONE) {
    printf("\n   ✗✗✗ CACHE MISS! This is the bug! ✗✗✗\n");
    printf("\n   The viewer stored entry with canonical=0x%016llx, actual=0x%016llx\n",
           (unsigned long long)canonicalHash, (unsigned long long)lossyHash);
    printf("   The server sent reference with canonical hash 0x%016llx\n",
           (unsigned long long)canonicalHash2);
    printf("   Viewer should find entry by canonical hash but failed!\n");
    printf("\n   Possible causes:\n");
    printf("   - Canonical hash not stored with entry\n");
    printf("   - Lookup by canonical hash not implemented\n");
    printf("   - CacheKey mismatch (width/height different?)\n");
    return 1;
  }
  
  if (result != HASH_TYPE_LOSSY) {
    printf("\n   ✗ ERROR: Expected LOSSY hit but got %s\n",
           result == HASH_TYPE_LOSSLESS ? "LOSSLESS" : "NONE");
    return 1;
  }
  
  printf("\n   ✓✓✓ LOSSY CACHE HIT! The dual-hash flow works correctly! ✓✓✓\n");
  printf("   Server sent canonical hash, viewer found lossy entry!\n");
  
  printf("\n===================\n");
  printf("All tests passed!\n");
  printf("===================\n");
  printf("\nSummary:\n");
  printf("- Canonical hash: 0x%016llx\n", (unsigned long long)canonicalHash);
  printf("- Lossy hash:     0x%016llx\n", (unsigned long long)lossyHash);
  printf("- Viewer stored entry with BOTH hashes\n");
  printf("- Server sent reference with canonical hash only\n");
  printf("- Viewer successfully found lossy entry by canonical hash\n");
  printf("- NEW DESIGN: Viewer manages canonical→lossy mapping!\n");
  
  return 0;
}
