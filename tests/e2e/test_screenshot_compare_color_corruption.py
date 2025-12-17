#!/usr/bin/env python3
"""Unit-style regression test for screenshot_compare color corruption detection.

This test is intentionally lightweight (no VNC servers/viewers) and focuses on
the screenshot comparison logic.

We construct two synthetic PNGs:
- A ground-truth image (uniform grey)
- A cache-on image with a small cyan rectangle (high chroma difference)

The rectangle is sized to be small enough that grayscale SSIM and the current
average-hash metric can still report the images as "perceptually similar".

The comparison must still flag this as corruption.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

# Ensure we can import from tests/e2e when running as a script.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from screenshot_compare import compare_screenshots


def _write_png(path: Path, img: Image.Image) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)


def _make_uniform_rgba(size: int, rgba: tuple[int, int, int, int]) -> Image.Image:
    return Image.new("RGBA", (size, size), rgba)


def _fill_rect(img: Image.Image, rect: tuple[int, int, int, int], rgba: tuple[int, int, int, int]) -> None:
    draw = ImageDraw.Draw(img)
    draw.rectangle(rect, fill=rgba)


def _make_small_cyan_patch(img: Image.Image, rect: tuple[int, int, int, int]) -> None:
    _fill_rect(img, rect, (0, 255, 255, 255))


def _make_small_noise_variant(img: Image.Image, seed: int = 0, max_delta: int = 2) -> Image.Image:
    # Create a deterministic small-noise variant to emulate "many small diffs"
    # without resembling structured corruption.
    arr = np.array(img, dtype=np.int16)
    rng = np.random.default_rng(seed)
    noise = rng.integers(-max_delta, max_delta + 1, size=arr.shape, dtype=np.int16)
    # Keep alpha fixed.
    noise[:, :, 3] = 0
    arr2 = np.clip(arr + noise, 0, 255).astype(np.uint8)
    return Image.fromarray(arr2, mode="RGBA")


def main() -> int:
    # Use an image large enough that a small corruption region can slip past
    # grayscale-based perceptual metrics.
    size = 512

    # A 12x12 patch is deliberately chosen because it can produce:
    #  - phash_dist == 0
    #  - SSIM still >= 0.95
    # while still being a visually obvious corruption when it affects text.
    patch = (10, 10, 21, 21)  # inclusive bounds => 12x12

    with tempfile.TemporaryDirectory() as td:
        td_path = Path(td)
        gt_path = td_path / "gt.png"
        cache_path = td_path / "cache.png"

        gt = _make_uniform_rgba(size, (200, 200, 200, 255))
        cache = _make_uniform_rgba(size, (200, 200, 200, 255))
        _make_small_cyan_patch(cache, patch)

        _write_png(gt_path, gt)
        _write_png(cache_path, cache)

        res = compare_screenshots(gt_path, cache_path)

        if res.identical:
            print("FAIL: expected images to differ")
            return 1

        # Confirm the corruption can slip through the existing perceptual gates.
        if res.perceptual_hash_distance is None or res.ssim_score is None:
            print("FAIL: expected perceptual metrics to be computed")
            return 1

        if res.perceptual_hash_distance != 0:
            print(f"FAIL: expected phash_dist==0 for this synthetic case, got {res.perceptual_hash_distance}")
            return 1

        if res.ssim_score < 0.95:
            print(f"FAIL: expected SSIM>=0.95 for this synthetic case, got {res.ssim_score}")
            return 1

        # This is the actual behavior we want: flag severe localized color shifts.
        if not getattr(res, "has_large_color_shifts", False):
            print("FAIL: expected has_large_color_shifts=True for localized cyan patch corruption")
            return 1

        # Regression: a mild magenta/pink cast can have max-channel deltas < 80
        # (so it would not be caught by a strict per-channel threshold) while
        # still being visually catastrophic.
        #
        # Use a colour with saturation 77 (255,178,255) to match a real-world
        # menubar corruption case.
        gt_mag_path = td_path / "gt_mag.png"
        cache_mag_path = td_path / "cache_mag.png"
        cache_mag = _make_uniform_rgba(size, (200, 200, 200, 255))
        _fill_rect(cache_mag, patch, (255, 178, 255, 255))
        _write_png(gt_mag_path, gt)
        _write_png(cache_mag_path, cache_mag)

        res_mag = compare_screenshots(gt_mag_path, cache_mag_path)
        if res_mag.identical:
            print("FAIL: expected magenta cast images to differ")
            return 1
        if res_mag.perceptual_hash_distance is None or res_mag.ssim_score is None:
            print("FAIL: expected perceptual metrics for magenta cast case")
            return 1
        if res_mag.perceptual_hash_distance >= 10:
            print(
                "FAIL: expected magenta cast to still be perceptually similar "
                f"(phash_dist={res_mag.perceptual_hash_distance})"
            )
            return 1
        if res_mag.ssim_score < 0.95:
            print(f"FAIL: expected SSIM>=0.95 for magenta cast case, got {res_mag.ssim_score}")
            return 1
        if not getattr(res_mag, "has_large_color_shifts", False):
            print("FAIL: expected has_large_color_shifts=True for localized magenta cast corruption")
            return 1

        # Guardrail: small distributed noise should not be flagged as corruption.
        noise_cache = _make_small_noise_variant(gt, seed=0, max_delta=2)
        noise_gt_path = td_path / "gt2.png"
        noise_cache_path = td_path / "cache2.png"
        _write_png(noise_gt_path, gt)
        _write_png(noise_cache_path, noise_cache)

        res2 = compare_screenshots(noise_gt_path, noise_cache_path)
        if getattr(res2, "has_large_color_shifts", False):
            print("FAIL: expected has_large_color_shifts=False for small distributed noise")
            return 1

    print("PASS")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
