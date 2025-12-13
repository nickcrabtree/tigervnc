/* Test lossy vs lossless cache hash behavior
 *
 * This test verifies that:
 * 1. Lossless encoding produces identical hashes when decoded
 * 2. Lossy encoding (JPEG) produces different hashes when decoded
 * 3. The lossy hash mapping mechanism works correctly
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
#include <assert.h>

using namespace rfb;

// Create a simple test pattern
void fillTestPattern(uint8_t* buffer, int width, int height, int stride) {
  for (int y = 0; y < height; y++) {
    uint8_t* row = buffer + y * stride * 4; // RGBA
    for (int x = 0; x < width; x++) {
      uint8_t* pixel = row + x * 4;
      // Simple gradient pattern
      pixel[0] = (x * 255) / width;   // R
      pixel[1] = (y * 255) / height;  // G
      pixel[2] = 128;                  // B
      pixel[3] = 255;                  // A
    }
  }
}

// Compute hash of a buffer
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
  printf("Testing lossy vs lossless cache hash behavior...\n\n");
  
  // Setup
  const int width = 128;
  const int height = 128;
  const int stride = width;
  PixelFormat pf(32, 24, false, true, 255, 255, 255, 16, 8, 0);
  
  // Create original test pattern
  std::vector<uint8_t> original(height * stride * 4);
  fillTestPattern(original.data(), width, height, stride);
  
  // Compute canonical (lossless) hash
  uint64_t canonicalHash = computeHash(original.data(), width, height, stride, pf);
  printf("1. Original (canonical) hash: 0x%016llx\n", (unsigned long long)canonicalHash);
  
  // Test 1: Lossless round-trip produces identical hash
  printf("\n2. Testing lossless round-trip...\n");
  std::vector<uint8_t> losslessCopy(original);
  uint64_t losslessHash = computeHash(losslessCopy.data(), width, height, stride, pf);
  printf("   Lossless copy hash: 0x%016llx\n", (unsigned long long)losslessHash);
  
  if (losslessHash == canonicalHash) {
    printf("   ✓ PASS: Lossless hash matches canonical\n");
  } else {
    printf("   ✗ FAIL: Lossless hash differs from canonical!\n");
    return 1;
  }
  
  // Test 2: JPEG compression/decompression produces different hash
  printf("\n3. Testing lossy (JPEG) round-trip...\n");
  
  // Compress with JPEG (quality 75, subsample 422)
  JpegCompressor compressor;
  compressor.compress(original.data(), stride, core::Rect(0, 0, width, height), pf, 75, 422);
  size_t compressedSize = compressor.length();
  std::vector<uint8_t> compressed(compressedSize);
  memcpy(compressed.data(), compressor.data(), compressedSize);
  printf("   Compressed to %zu bytes (%.1f%% of original)\n", 
         compressedSize, (compressedSize * 100.0) / (width * height * 4));
  
  // Decompress
  std::vector<uint8_t> decompressed(height * stride * 4);
  JpegDecompressor decompressor;
  decompressor.decompress(compressed.data(), compressedSize, decompressed.data(),
                          stride, core::Rect(0, 0, width, height), pf);
  
  uint64_t lossyHash1 = computeHash(decompressed.data(), width, height, stride, pf);
  printf("   First lossy hash: 0x%016llx\n", (unsigned long long)lossyHash1);
  
  if (lossyHash1 != canonicalHash) {
    printf("   ✓ PASS: Lossy hash differs from canonical (expected)\n");
  } else {
    printf("   ✗ FAIL: Lossy hash matches canonical (JPEG should introduce artifacts!)\n");
    return 1;
  }
  
  // Test 3: Re-decompressing same JPEG produces different hash each time
  printf("\n4. Testing JPEG decompression determinism...\n");
  
  std::vector<uint8_t> decompressed2(height * stride * 4);
  decompressor.decompress(compressed.data(), compressedSize, decompressed2.data(),
                          stride, core::Rect(0, 0, width, height), pf);
  
  uint64_t lossyHash2 = computeHash(decompressed2.data(), width, height, stride, pf);
  printf("   Second lossy hash: 0x%016llx\n", (unsigned long long)lossyHash2);
  
  if (lossyHash2 == lossyHash1) {
    printf("   ✓ PASS: JPEG decompression is deterministic\n");
  } else {
    printf("   ✗ WARNING: JPEG decompression is NON-DETERMINISTIC!\n");
    printf("   This means each decode produces different pixels, breaking cache hits.\n");
    printf("   Hash difference: 0x%016llx XOR 0x%016llx = 0x%016llx\n",
           (unsigned long long)lossyHash1,
           (unsigned long long)lossyHash2,
           (unsigned long long)(lossyHash1 ^ lossyHash2));
  }
  
  // Test 4: CacheKey behavior
  printf("\n5. Testing CacheKey matching...\n");
  CacheKey canonicalKey(width, height, canonicalHash);
  CacheKey lossyKey1(width, height, lossyHash1);
  CacheKey lossyKey2(width, height, lossyHash2);
  
  printf("   Canonical key: (w=%u, h=%u, id=0x%016llx)\n",
         canonicalKey.width, canonicalKey.height,
         (unsigned long long)canonicalKey.contentHash);
  printf("   Lossy key 1:    (w=%u, h=%u, id=0x%016llx)\n",
         lossyKey1.width, lossyKey1.height,
         (unsigned long long)lossyKey1.contentHash);
  printf("   Lossy key 2:    (w=%u, h=%u, id=0x%016llx)\n",
         lossyKey2.width, lossyKey2.height,
         (unsigned long long)lossyKey2.contentHash);
  
  if (canonicalKey == lossyKey1) {
    printf("   ✗ FAIL: Canonical key matches lossy key (should differ!)\n");
    return 1;
  }
  
  if (lossyKey1 == lossyKey2) {
    printf("   ✓ PASS: Same JPEG decoded twice produces same CacheKey\n");
  } else {
    printf("   ✗ FAIL: Same JPEG decoded twice produces different CacheKeys!\n");
    printf("   This will break cache hits completely.\n");
    return 1;
  }
  
  printf("\n=================\n");
  printf("All tests passed!\n");
  printf("=================\n");
  
  return 0;
}
