#!/usr/bin/env python3
"""Regression test using real-world corrupt screenshots.

This test uses actual screenshots captured from a user's environment showing
PersistentCache corruption. The test verifies that our screenshot comparison
correctly identifies this corruption.

If this test FAILS, it means the detection logic has regressed and would miss
real corruption that exists in production.

If this test PASSES, it means the detection is working correctly.

The underlying bug in PersistentCache that causes the corruption is tracked
separately. This test ensures we can detect it when it occurs.

Screenshots:
- Screenshot 2025-12-16 at 18.09.18 (2).png: Ground truth (before cache usage)
- Screenshot 2025-12-16 at 18.10.39 (2).png: Cache-corrupted version

Visual corruption observed:
- Pink/magenta color cast in menu bar region
- Entire rectangular regions with wrong colors
- ~23% pixel difference with SSIM still ~0.99 (perceptually similar at a glance)
"""

from __future__ import annotations

import sys
from pathlib import Path

# Ensure we can import from tests/e2e when running as a script.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from screenshot_compare import compare_screenshots


# Location of real-world corrupt screenshots from user's environment.
REAL_SCREENSHOTS_DIR = Path("/ncloud/Nick/code/tigervnc")
SCREENSHOT_GT = REAL_SCREENSHOTS_DIR / "Screenshot 2025-12-16 at 18.09.18 (2).png"
SCREENSHOT_CORRUPT = REAL_SCREENSHOTS_DIR / "Screenshot 2025-12-16 at 18.10.39 (2).png"


def main() -> int:
    # Skip if screenshots not available (e.g., running on CI without access)
    if not SCREENSHOT_GT.exists() or not SCREENSHOT_CORRUPT.exists():
        print(f"SKIP: Real-world screenshots not available at {REAL_SCREENSHOTS_DIR}")
        print(f"  Expected: {SCREENSHOT_GT}")
        print(f"  Expected: {SCREENSHOT_CORRUPT}")
        return 0  # Return success to not fail CI, but note the skip

    res = compare_screenshots(SCREENSHOT_GT, SCREENSHOT_CORRUPT)

    # The images must differ
    if res.identical:
        print("FAIL: Expected images to differ (known corruption case)")
        return 1

    # Report metrics for transparency
    print(f"Metrics for real-world corruption case:")
    print(f"  diff_pct: {res.diff_pct:.2f}%")
    print(f"  ssim_score: {res.ssim_score}")
    print(f"  perceptual_hash_distance: {res.perceptual_hash_distance}")
    print(f"  has_solid_black_regions: {res.has_solid_black_regions}")
    print(f"  has_high_contrast_edges: {res.has_high_contrast_edges}")
    print(f"  has_large_color_shifts: {res.has_large_color_shifts}")

    # Key assertion: The corruption MUST be detected.
    # This is the whole point of the color shift detection logic.
    if not res.has_large_color_shifts:
        print("FAIL: has_large_color_shifts should be True for this known corruption")
        print("  This regression means the detection logic is too permissive and")
        print("  would miss real-world PersistentCache corruption!")
        return 1

    # Secondary check: The perceptual metrics should show this is "similar"
    # (which is why we need the color shift detection - the metrics alone miss it)
    if res.ssim_score is not None and res.ssim_score < 0.90:
        print(f"INFO: SSIM lower than expected ({res.ssim_score:.3f}), detection may be redundant")

    if res.perceptual_hash_distance is not None and res.perceptual_hash_distance >= 10:
        print(f"INFO: phash_distance higher than expected ({res.perceptual_hash_distance}), detection may be redundant")

    # This is the expected test behavior: detection correctly flags the corruption
    print("PASS: Detection correctly identifies real-world corruption")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
