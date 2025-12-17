#!/usr/bin/env python3
"""Screenshot comparator for black-box visual corruption tests.

Compares two PNG screenshots pixel-by-pixel and optionally writes a
highlighted diff image and JSON summary.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Iterable, Tuple

import numpy as np
from PIL import Image

from framework import PreflightError


@dataclass
class ScreenshotDiffResult:
    """Result of a screenshot comparison."""

    identical: bool
    total_pixels: int
    diff_pixels: int
    diff_pct: float
    bbox: Optional[tuple[int, int, int, int]]  # (min_x, min_y, max_x, max_y) or None
    # Perceptual similarity metrics (robust to JPEG artifacts)
    perceptual_hash_distance: Optional[int] = None  # Hamming distance (0=identical, >10=different)
    ssim_score: Optional[float] = None  # Structural similarity (1.0=perfect, <0.95=corrupted)
    has_solid_black_regions: bool = False  # Detected solid black rectangles
    has_high_contrast_edges: bool = False  # Detected abrupt grayscale boundaries
    has_large_color_shifts: bool = False  # Detected abrupt localized RGB color shifts (e.g. magenta/cyan artifacts)


def _ensure_png(path: Path) -> Path:
    """Return the path with a .png suffix (without changing the filename if it already has one)."""

    if path.suffix.lower() == ".png":
        return path
    return path.with_suffix(".png")


def _compute_average_hash(img: Image.Image, hash_size: int = 8) -> int:
    """Compute average hash (aHash) for perceptual similarity.
    
    Returns hash as integer. Hamming distance between hashes indicates
    perceptual difference. Robust to JPEG compression artifacts.
    """
    # Resize to hash_size x hash_size and convert to grayscale
    img_small = img.resize((hash_size, hash_size), Image.Resampling.LANCZOS).convert('L')
    pixels = list(img_small.getdata())
    
    # Compute average pixel value
    avg = sum(pixels) / len(pixels)
    
    # Create hash: 1 if pixel > average, 0 otherwise
    hash_value = 0
    for i, pixel in enumerate(pixels):
        if pixel > avg:
            hash_value |= (1 << i)
    
    return hash_value


def _hamming_distance(hash1: int, hash2: int) -> int:
    """Compute Hamming distance between two integer hashes."""
    return bin(hash1 ^ hash2).count('1')


def _compute_ssim_simple(img1: Image.Image, img2: Image.Image, window_size: int = 11) -> float:
    """Compute simplified SSIM (Structural Similarity Index).
    
    Returns value in [0, 1] where 1.0 = identical structure.
    Robust to minor compression artifacts, sensitive to structural changes.
    """
    # Convert to numpy arrays (grayscale for simplicity)
    arr1 = np.array(img1.convert('L'), dtype=np.float64)
    arr2 = np.array(img2.convert('L'), dtype=np.float64)
    
    # Constants to stabilize division
    C1 = (0.01 * 255) ** 2
    C2 = (0.03 * 255) ** 2
    
    # Compute means
    mu1 = arr1.mean()
    mu2 = arr2.mean()
    
    # Compute variances and covariance
    sigma1_sq = arr1.var()
    sigma2_sq = arr2.var()
    sigma12 = np.mean((arr1 - mu1) * (arr2 - mu2))
    
    # SSIM formula
    numerator = (2 * mu1 * mu2 + C1) * (2 * sigma12 + C2)
    denominator = (mu1**2 + mu2**2 + C1) * (sigma1_sq + sigma2_sq + C2)
    
    return numerator / denominator


def _detect_solid_black_regions(diff_pixels_mask: list[tuple[int, int]], 
                                 img: Image.Image,
                                 min_region_size: int = 64) -> bool:
    """Detect if differing pixels form solid black rectangular regions.
    
    Returns True if large contiguous regions of pure black are found.
    This indicates corruption (e.g., failed decoding) rather than JPEG artifacts.
    """
    if not diff_pixels_mask:
        return False
    
    # Sample some differing pixels and check if they're solid black
    px = img.load()
    black_count = 0
    sample_size = min(100, len(diff_pixels_mask))
    
    for i in range(0, len(diff_pixels_mask), max(1, len(diff_pixels_mask) // sample_size)):
        x, y = diff_pixels_mask[i]
        r, g, b = px[x, y][:3]
        # Consider "black" if all channels < 10
        if r < 10 and g < 10 and b < 10:
            black_count += 1
    
    # If >50% of differing pixels are solid black, likely corruption
    return black_count > sample_size * 0.5


def _detect_high_contrast_edges(img1: Image.Image, img2: Image.Image, 
                                 diff_pixels_mask: list[tuple[int, int]]) -> bool:
    """Detect abrupt grayscale boundaries indicating structural corruption.

    JPEG artifacts cause gradual color changes. Structural corruption
    (wrong rectangles) causes abrupt boundaries.
    """
    if len(diff_pixels_mask) < 10:
        return False

    arr1 = np.array(img1.convert('L'), dtype=np.float64)
    arr2 = np.array(img2.convert('L'), dtype=np.float64)

    # Compute gradient magnitude at differing pixels
    gy1, gx1 = np.gradient(arr1)
    gy2, gx2 = np.gradient(arr2)

    gradient_mag1 = np.sqrt(gx1**2 + gy1**2)
    gradient_mag2 = np.sqrt(gx2**2 + gy2**2)

    # Sample differing pixels and check for high gradients
    high_gradient_count = 0
    sample_size = min(100, len(diff_pixels_mask))

    for i in range(0, len(diff_pixels_mask), max(1, len(diff_pixels_mask) // sample_size)):
        x, y = diff_pixels_mask[i]
        # High gradient (>50) indicates abrupt edge
        if gradient_mag1[y, x] > 50 or gradient_mag2[y, x] > 50:
            high_gradient_count += 1

    # If >30% of differing pixels are at high-gradient edges, likely structural corruption
    return high_gradient_count > sample_size * 0.3


def _is_large_color_shift_pixel(
    r1: int,
    g1: int,
    b1: int,
    r2: int,
    g2: int,
    b2: int,
    *,
    max_channel_delta_threshold: int = 80,
    sum_delta_threshold: int = 120,
    # Some real-world corruption presents as a *cast* (e.g. pink/magenta) with
    # max-channel deltas below the strict threshold. Detect this by looking for
    # a large saturation increase relative to ground truth.
    sat_after_threshold: int = 70,
    sat_before_max: int = 20,
    brightness_min: int = 200,
) -> bool:
    """Return True if this pixel indicates a severe localized color shift.

    This is intended to catch "completely wrong colour" regions that can slip
    past grayscale SSIM + average-hash gating under Tight/JPEG.

    The thresholds are deliberately high to avoid flagging normal JPEG artifacts.
    """

    dr = abs(r1 - r2)
    dg = abs(g1 - g2)
    db = abs(b1 - b2)

    # Case 1: large absolute RGB delta (strong corruption)
    if max(dr, dg, db) >= max_channel_delta_threshold and (dr + dg + db) >= sum_delta_threshold:
        return True

    # Case 2: saturation "explosion" (grey -> pink/magenta casts)
    sat1 = max(r1, g1, b1) - min(r1, g1, b1)
    sat2 = max(r2, g2, b2) - min(r2, g2, b2)
    if sat2 >= sat_after_threshold and sat1 <= sat_before_max and (r2 + g2 + b2) >= brightness_min:
        return True

    return False


def _decide_has_large_color_shifts(
    tile_color_shift_counts: list[list[int]],
    *,
    tile_w: int,
    tile_h: int,
    total_color_shift_pixels: int,
    max_tiles_for_cluster: int = 2,
    min_total_pixels: int = 100,
    tile_area_fraction_threshold: float = 0.0005,
    min_pixels_in_one_tile: int = 50,
) -> bool:
    """Decide if the comparison shows large localized color shifts.

    Important: this must remain robust even when overall diff_pct is large (e.g.
    many small JPEG artifacts). We therefore look for a *cluster* of pixels that
    satisfy the strong per-pixel criteria.
    """

    if total_color_shift_pixels <= 0:
        return False

    max_tile = 0
    tiles_hit = 0
    for row in tile_color_shift_counts:
        for n in row:
            if n > 0:
                tiles_hit += 1
            if n > max_tile:
                max_tile = n

    tile_area = max(1, tile_w) * max(1, tile_h)
    per_tile_threshold = max(
        min_pixels_in_one_tile,
        int(tile_area * tile_area_fraction_threshold),
    )

    # Strong signal: many severe-shift pixels inside at least one tile.
    if max_tile >= per_tile_threshold:
        return True

    # Weaker-but-still-important signal: a small localized cluster (e.g. small
    # corrupted text area) that stays within a couple of tiles.
    return total_color_shift_pixels >= min_total_pixels and tiles_hit <= max_tiles_for_cluster

def compare_screenshots(
    ground_truth: Path,
    cache_on: Path,
    diff_out: Optional[Path] = None,
    json_out: Optional[Path] = None,
    ignore_rects: Optional[Iterable[Tuple[int, int, int, int]]] = None,
) -> ScreenshotDiffResult:
    """Compare two screenshots at the pixel level.

    Args:
        ground_truth: PNG image for cache-off viewer.
        cache_on: PNG image for cache-on viewer.
        diff_out: Optional path for a visual diff image.
        json_out: Optional path for a JSON summary report.

    Returns:
        ScreenshotDiffResult with summary stats.

    Raises:
        PreflightError: if images cannot be loaded or dimensions mismatch.
    """

    ground_truth = Path(ground_truth)
    cache_on = Path(cache_on)

    if not ground_truth.is_file():
        raise PreflightError(f"Ground-truth screenshot not found: {ground_truth}")
    if not cache_on.is_file():
        raise PreflightError(f"Cache-on screenshot not found: {cache_on}")

    try:
        img_gt = Image.open(ground_truth).convert("RGBA")
        img_cache = Image.open(cache_on).convert("RGBA")
    except Exception as exc:  # pragma: no cover - defensive
        raise PreflightError(f"Failed to open screenshots: {exc}") from exc

    if img_gt.size != img_cache.size:
        raise PreflightError(
            "Screenshot dimensions differ: "
            f"ground_truth={img_gt.size}, cache_on={img_cache.size}"
        )

    width, height = img_gt.size

    # Normalise ignore rectangles (x0, y0, x1, y1, inclusive bounds)
    norm_ignore: list[Tuple[int, int, int, int]] = []
    if ignore_rects:
        for (x0, y0, x1, y1) in ignore_rects:
            x0 = max(0, min(width - 1, x0))
            y0 = max(0, min(height - 1, y0))
            x1 = max(0, min(width - 1, x1))
            y1 = max(0, min(height - 1, y1))
            if x1 >= x0 and y1 >= y0:
                norm_ignore.append((x0, y0, x1, y1))

    total_pixels = width * height

    px_gt = img_gt.load()
    px_cache = img_cache.load()

    diff_img = None
    if diff_out is not None:
        diff_out = _ensure_png(diff_out)
        diff_img = Image.new("RGBA", (width, height), (0, 0, 0, 255))
        px_diff = diff_img.load()
    else:
        px_diff = None

    diff_pixels = 0
    min_x = width
    min_y = height
    max_x = -1
    max_y = -1

    # Track diff pixel coordinates for downstream detection heuristics.
    diff_pixel_coords: list[tuple[int, int]] = []

    # 4x4 tiling for localising differences
    tiles_x = 4
    tiles_y = 4
    tile_w = max(1, width // tiles_x)
    tile_h = max(1, height // tiles_y)
    tile_diff_counts = [[0 for _ in range(tiles_x)] for _ in range(tiles_y)]

    # Track severe localized color-shift pixels (magenta/cyan casts, etc.)
    tile_color_shift_counts = [[0 for _ in range(tiles_x)] for _ in range(tiles_y)]
    color_shift_pixels = 0

    def _ignored(x: int, y: int) -> bool:
        for x0, y0, x1, y1 in norm_ignore:
            if x0 <= x <= x1 and y0 <= y <= y1:
                return True
        return False

    for y in range(height):
        for x in range(width):
            if _ignored(x, y):
                # Treat masked regions as "don't care"; ensure diff image is black there
                if px_diff is not None:
                    px_diff[x, y] = (0, 0, 0, 255)
                continue

            a = px_gt[x, y]
            b = px_cache[x, y]
            if a != b:
                diff_pixels += 1
                diff_pixel_coords.append((x, y))

                # Update tile stats
                tx = min(tiles_x - 1, x // tile_w)
                ty = min(tiles_y - 1, y // tile_h)
                tile_diff_counts[ty][tx] += 1

                # Detect severe localized color shifts (e.g. grey -> magenta cast)
                r1, g1, b1 = a[:3]
                r2, g2, b2 = b[:3]
                is_color_shift = _is_large_color_shift_pixel(r1, g1, b1, r2, g2, b2)
                if is_color_shift:
                    color_shift_pixels += 1
                    tile_color_shift_counts[ty][tx] += 1

                if px_diff is not None:
                    # Mark differing pixels red, but highlight large color shifts as yellow
                    px_diff[x, y] = (255, 255, 0, 255) if is_color_shift else (255, 0, 0, 255)
                if x < min_x:
                    min_x = x
                if y < min_y:
                    min_y = y
                if x > max_x:
                    max_x = x
                if y > max_y:
                    max_y = y

    if diff_pixels == 0:
        bbox = None
        diff_pct = 0.0
    else:
        bbox = (min_x, min_y, max_x, max_y)
        diff_pct = (diff_pixels / float(total_pixels)) * 100.0

    if diff_img is not None and diff_pixels > 0:
        diff_out.parent.mkdir(parents=True, exist_ok=True)
        diff_img.save(diff_out)

    # Decide whether the differences contain localized severe color shifts.
    has_color_shifts = _decide_has_large_color_shifts(
        tile_color_shift_counts,
        tile_w=tile_w,
        tile_h=tile_h,
        total_color_shift_pixels=color_shift_pixels,
    )

    # Compute perceptual similarity metrics
    phash_distance = None
    ssim_score = None
    has_black_regions = False
    has_edges = False

    if diff_pixels > 0:
        # Perceptual hashing (robust to compression artifacts)
        try:
            hash_gt = _compute_average_hash(img_gt)
            hash_cache = _compute_average_hash(img_cache)
            phash_distance = _hamming_distance(hash_gt, hash_cache)
        except Exception:
            pass  # Skip if computation fails

        # SSIM (structural similarity)
        try:
            ssim_score = _compute_ssim_simple(img_gt, img_cache)
        except Exception:
            pass  # Skip if computation fails

        # Detect structural corruption patterns
        try:
            has_black_regions = _detect_solid_black_regions(diff_pixel_coords, img_cache)
            has_edges = _detect_high_contrast_edges(img_gt, img_cache, diff_pixel_coords)
        except Exception:
            pass  # Skip if detection fails
    
    result = ScreenshotDiffResult(
        identical=(diff_pixels == 0),
        total_pixels=total_pixels,
        diff_pixels=diff_pixels,
        diff_pct=diff_pct,
        bbox=bbox,
        perceptual_hash_distance=phash_distance,
        ssim_score=ssim_score,
        has_solid_black_regions=has_black_regions,
        has_high_contrast_edges=has_edges,
        has_large_color_shifts=has_color_shifts,
    )

    if json_out is not None:
        json_out = json_out.with_suffix(".json")
        json_out.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "identical": result.identical,
            "total_pixels": result.total_pixels,
            "diff_pixels": result.diff_pixels,
            "diff_pct": result.diff_pct,
            "bbox": result.bbox,
            "perceptual": {
                "hash_distance": result.perceptual_hash_distance,
                "ssim_score": result.ssim_score,
                "has_solid_black_regions": result.has_solid_black_regions,
                "has_high_contrast_edges": result.has_high_contrast_edges,
                "has_large_color_shifts": result.has_large_color_shifts,
            },
            "tiles": {
                "grid": [row[:] for row in tile_diff_counts],
                "tiles_x": tiles_x,
                "tiles_y": tiles_y,
                "tile_width": tile_w,
                "tile_height": tile_h,
            },
        }
        json_out.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    return result
