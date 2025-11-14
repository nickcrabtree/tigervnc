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


def _ensure_png(path: Path) -> Path:
    """Return the path with a .png suffix (without changing the filename if it already has one)."""

    if path.suffix.lower() == ".png":
        return path
    return path.with_suffix(".png")


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

    # 4x4 tiling for localising differences
    tiles_x = 4
    tiles_y = 4
    tile_w = max(1, width // tiles_x)
    tile_h = max(1, height // tiles_y)
    tile_diff_counts = [[0 for _ in range(tiles_x)] for _ in range(tiles_y)]

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

                # Update tile stats
                tx = min(tiles_x - 1, x // tile_w)
                ty = min(tiles_y - 1, y // tile_h)
                tile_diff_counts[ty][tx] += 1

                if px_diff is not None:
                    # Mark differing pixels bright red
                    px_diff[x, y] = (255, 0, 0, 255)
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

    result = ScreenshotDiffResult(
        identical=(diff_pixels == 0),
        total_pixels=total_pixels,
        diff_pixels=diff_pixels,
        diff_pct=diff_pct,
        bbox=bbox,
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
