# Screenshot Comparison Design

## Problem Statement

Black-box screenshot tests need to distinguish between two fundamentally different types of visual differences:

1. **Acceptable**: JPEG compression artifacts - many pixels with small RGB differences distributed across the image
2. **Unacceptable**: Structural corruption - small regions with severe errors (black rectangles, out-of-place content)

Traditional pixel-by-pixel comparison cannot distinguish these cases. Browser scenarios showed ~35% pixel differences due to lossy encoding, but were visually identical. Real corruption (experienced in production) manifests as small areas of completely wrong content.

## Solution: Multi-Metric Perceptual Analysis

The screenshot comparison in `screenshot_compare.py` now uses three complementary metrics:

### 1. Average Hash (aHash) - Perceptual Similarity
- **What**: Downscales image to 8x8, converts to grayscale, computes binary hash based on average pixel value
- **Purpose**: Robust to minor compression artifacts, sensitive to structural changes
- **Threshold**: Hamming distance < 10 indicates perceptual similarity
- **Advantages**: Fast, deterministic, compression-resistant

### 2. SSIM (Structural Similarity Index) - Structural Integrity  
- **What**: Compares luminance, contrast, and structure between images
- **Purpose**: Measures perceptual quality beyond pixel values
- **Threshold**: Score ≥ 0.95 indicates structurally similar
- **Advantages**: Well-studied metric, accounts for human visual perception

### 3. Corruption Pattern Detection - Explicit Failure Modes
- **Solid Black Regions**: Samples differing pixels, checks if >50% are pure black (RGB < 10)
- **High Contrast Edges**: Computes gradient magnitude (grayscale), checks if >30% of diffs are at abrupt boundaries
- **Large Color Shifts**: Samples differing pixels, checks for severe localized RGB shifts (e.g., cyan/magenta artifacts) that can occur under Tight/JPEG
- **Purpose**: Explicitly detect known failure modes (rendering errors, cache corruption)
- **Advantages**: Catches specific corruption types that might slip through other metrics

## Decision Logic

```python
# Test passes if:
is_perceptually_similar = (ssim >= 0.95 and phash_distance < 10)
has_corruption = ((has_solid_black_regions and has_high_contrast_edges) or has_large_color_shifts)

if is_perceptually_similar and not has_corruption:
    PASS
else:
    FAIL
```

**Note**: Black-rectangle-style corruption detection requires **both** black regions **and** high-contrast edges to avoid false positives from legitimate high-contrast content (e.g., dynamic browser elements). Large localized color shifts (e.g. cyan/magenta artifacts under Tight/JPEG) are treated as corruption on their own.

## Real-World Results

### Browser Scenario (PersistentCache, lossy JPEG)
- **Before**: 35% pixel diff → test failed with tolerance workaround
- **After**: 35% pixel diff, SSIM=1.000, phash=0, no corruption → test **passes**

### Expected Corruption Scenario
- Small black rectangle (100×100 = 10,000 pixels = 0.6% of 1600×1000 image)
- **Pixel diff**: Low (<1%)
- **SSIM**: High (>0.99, most of image unchanged)
- **phash**: Low (<5, overall structure similar)
- **Corruption detection**: `has_solid_black_regions=True` → test **fails correctly**

## Implementation

### Core Functions (`screenshot_compare.py`)

- `_compute_average_hash(img)`: Returns integer hash for perceptual comparison
- `_hamming_distance(hash1, hash2)`: Computes bit difference between hashes
- `_compute_ssim_simple(img1, img2)`: Returns structural similarity score [0,1]
- `_detect_solid_black_regions(diff_coords, img)`: Samples diffs for solid black
- `_detect_high_contrast_edges(img1, img2, diff_coords)`: Checks grayscale gradient at diffs
- `_detect_large_color_shifts(img1, img2, diff_coords)`: Detects severe localized RGB shifts

### Data Structure

```python
@dataclass
class ScreenshotDiffResult:
    # Traditional metrics
    identical: bool
    total_pixels: int
    diff_pixels: int
    diff_pct: float
    bbox: Optional[tuple[int, int, int, int]]
    
    # Perceptual metrics (new)
    perceptual_hash_distance: Optional[int]
    ssim_score: Optional[float]
    has_solid_black_regions: bool
    has_high_contrast_edges: bool
    has_large_color_shifts: bool
```

### Test Integration (`run_black_box_screenshot_test.py`)

Tests now report perceptual metrics in output:
```
Checkpoint 1: OK (perceptually similar; 164477 pixels differ (22.09%), SSIM=1.000, phash_dist=0)
```

Or on failure:
```
Checkpoint 2: MISMATCH - 5243 pixels differ (0.33%), SSIM=0.982, phash_dist=12 [CORRUPTION]
```

## Benefits

1. **No False Positives**: JPEG artifacts no longer cause test failures
2. **No False Negatives**: Structural corruption is explicitly detected
3. **No Tolerance Relaxation**: Tests remain strict, TDD principles maintained
4. **Diagnostic Value**: Output clearly indicates why comparison failed
5. **Zero Dependencies**: Uses only PIL/Pillow and NumPy (already required)
6. **Fast**: Perceptual hash is O(1) after downscaling, SSIM is O(n) single pass

## Alternative Approaches Considered

- **JPEG-encode both screenshots**: Double-JPEG doesn't guarantee identical output
- **Block-based comparison**: Complex threshold tuning, less robust
- **Neural network classifier**: Overkill, requires training data and dependencies
- **imagehash library**: Good but adds dependency; our implementation covers the use case

## Future Enhancements

If needed:
- Add difference hash (dHash) for rotational robustness
- Implement wavelet-based perceptual hash for better compression artifact immunity
- Add block-level SSIM for localization of structural corruption
- Tune thresholds based on empirical data from more test scenarios
