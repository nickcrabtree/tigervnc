#!/usr/bin/env python3
"""TDD test for dark rectangle visual corruption in cached screenshots.

This test detects small dark/black rectangular artifacts that appear in one
viewer's screenshot but not another, which can indicate cache-related
rendering corruption bugs.

The test compares:
1. Two no-cache viewers (sanity check - should have no exclusive dark rects)
2. One cached viewer vs one no-cache viewer (test case - should have no exclusive dark rects)

Based on the netweaver browser resize scenario which has exhibited this corruption.
"""

import os
import sys
from pathlib import Path

from dark_rect_detector import compare_dark_rectangles
from framework import PreflightError


def main() -> int:
    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"
    if not runner.is_file():
        print(f"ERROR: black-box screenshot runner not found: {runner}", file=sys.stderr)
        return 1
    
    # Check if we're in validation mode (checking for dark rect corruption)
    if "--validate-dark-rects" in sys.argv:
        return validate_dark_rects()
    
    # Run the netweaver test with both cache modes
    # First: sanity check with two no-cache viewers
    print("=" * 80)
    print("PHASE 1: Sanity check (no-cache vs no-cache)")
    print("=" * 80)
    print()
    
    argv_sanity = [
        sys.executable,
        str(runner),
        "--scenario",
        "browser",
        "--browser-url",
        "https://www.netweaver.uk/going-cookie-free/",
        "--viewer-geometry",
        "1600x900",
        "--viewer-display-geometry",
        "3200x1800",
        "--viewer-resize-factor",
        "2.0",
        "--viewer-resize-at-checkpoint",
        "3",
        "--mode",
        "none",  # Both viewers with no cache
        "--lossless",  # force deterministic, lossless encoding
    ]
    
    # Filter out our custom arg
    extra_args = [arg for arg in sys.argv[1:] if arg != "--validate-dark-rects"]
    argv_sanity.extend(extra_args)
    
    print(f"Running: {' '.join(argv_sanity)}")
    print()
    
    ret = os.spawnv(os.P_WAIT, sys.executable, argv_sanity)
    if ret != 0:
        print(f"\nSanity check failed with exit code {ret}", file=sys.stderr)
        return ret
    
    # Validate sanity check for dark rects
    print("\n" + "=" * 80)
    print("Validating sanity check for dark rectangle corruption...")
    print("=" * 80)
    
    # Find most recent artifacts dir
    artifacts_base = here / "_artifacts"
    if not artifacts_base.exists():
        print("ERROR: No artifacts directory found", file=sys.stderr)
        return 1
    
    # Get most recent artifacts directory
    artifact_dirs = sorted(artifacts_base.glob("202*"), reverse=True)
    if not artifact_dirs:
        print("ERROR: No artifact directories found", file=sys.stderr)
        return 1
    
    sanity_artifacts = artifact_dirs[0]
    print(f"Checking artifacts: {sanity_artifacts}")
    
    # Check checkpoint 1 for dark rects
    screenshots_dir = sanity_artifacts / "screenshots"
    if not screenshots_dir.exists():
        print(f"ERROR: Screenshots directory not found: {screenshots_dir}", file=sys.stderr)
        return 1
    
    ground_truth_png = screenshots_dir / "checkpoint_1_ground_truth.png"
    cache_png = screenshots_dir / "checkpoint_1_cache.png"
    
    if not ground_truth_png.exists() or not cache_png.exists():
        print("ERROR: Checkpoint 1 screenshots not found", file=sys.stderr)
        return 1
    
    try:
        result = compare_dark_rectangles(ground_truth_png, cache_png)
        
        if result.has_exclusive_rects:
            print(f"\n✗ SANITY CHECK FAILED: Found {result.total_exclusive_rects} exclusive dark rectangles")
            print(f"  Ground truth only: {len(result.image1_only)}")
            for rect in result.image1_only:
                print(f"    {rect}")
            print(f"  Cache only: {len(result.image2_only)}")
            for rect in result.image2_only:
                print(f"    {rect}")
            return 1
        else:
            print("\n✓ Sanity check passed: No exclusive dark rectangles found")
    
    except PreflightError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1
    
    # Phase 2: Test with persistent cache enabled
    print("\n" + "=" * 80)
    print("PHASE 2: Cache test (no-cache vs persistent-cache)")
    print("=" * 80)
    print()
    
    argv_cache = [
        sys.executable,
        str(runner),
        "--scenario",
        "browser",
        "--browser-url",
        "https://www.netweaver.uk/going-cookie-free/",
        "--viewer-geometry",
        "1600x900",
        "--viewer-display-geometry",
        "3200x1800",
        "--viewer-resize-factor",
        "2.0",
        "--viewer-resize-at-checkpoint",
        "3",
        "--mode",
        "persistent",  # Enable PersistentCache
        "--lossless",  # force deterministic, lossless encoding
    ]
    
    argv_cache.extend(extra_args)
    
    print(f"Running: {' '.join(argv_cache)}")
    print()
    
    ret = os.spawnv(os.P_WAIT, sys.executable, argv_cache)
    if ret != 0:
        print(f"\nCache test failed with exit code {ret}", file=sys.stderr)
        return ret
    
    # Validate cache test for dark rects
    print("\n" + "=" * 80)
    print("Validating cache test for dark rectangle corruption...")
    print("=" * 80)
    
    # Get most recent artifacts directory (should be the cache test)
    artifact_dirs = sorted(artifacts_base.glob("202*"), reverse=True)
    if len(artifact_dirs) < 2:
        print("ERROR: Not enough artifact directories found", file=sys.stderr)
        return 1
    
    cache_artifacts = artifact_dirs[0]
    print(f"Checking artifacts: {cache_artifacts}")
    
    screenshots_dir = cache_artifacts / "screenshots"
    ground_truth_png = screenshots_dir / "checkpoint_1_ground_truth.png"
    cache_png = screenshots_dir / "checkpoint_1_cache.png"
    
    if not ground_truth_png.exists() or not cache_png.exists():
        print("ERROR: Checkpoint 1 screenshots not found", file=sys.stderr)
        return 1
    
    try:
        result = compare_dark_rectangles(ground_truth_png, cache_png)
        
        if result.has_exclusive_rects:
            print(f"\n✗ TEST FAILED: Found {result.total_exclusive_rects} exclusive dark rectangles")
            print(f"  Ground truth only: {len(result.image1_only)}")
            for rect in result.image1_only:
                print(f"    {rect}")
            print(f"  Cache only: {len(result.image2_only)}")
            for rect in result.image2_only:
                print(f"    {rect}")
            print("\nThis indicates visual corruption in the PersistentCache viewer.")
            print("The cache implementation may be corrupting tile boundaries or")
            print("failing to properly decode/render cached content.")
            return 1
        else:
            print("\n✓ Test passed: No exclusive dark rectangles found")
            return 0
    
    except PreflightError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1


def validate_dark_rects() -> int:
    """Validate existing artifacts for dark rectangle corruption."""
    here = Path(__file__).resolve().parent
    artifacts_base = here / "_artifacts"
    
    if not artifacts_base.exists():
        print("ERROR: No artifacts directory found", file=sys.stderr)
        return 1
    
    # Get most recent artifacts directory
    artifact_dirs = sorted(artifacts_base.glob("202*"), reverse=True)
    if not artifact_dirs:
        print("ERROR: No artifact directories found", file=sys.stderr)
        return 1
    
    artifacts_dir = artifact_dirs[0]
    print(f"Validating artifacts: {artifacts_dir}")
    
    screenshots_dir = artifacts_dir / "screenshots"
    ground_truth_png = screenshots_dir / "checkpoint_1_ground_truth.png"
    cache_png = screenshots_dir / "checkpoint_1_cache.png"
    
    if not ground_truth_png.exists() or not cache_png.exists():
        print("ERROR: Checkpoint 1 screenshots not found", file=sys.stderr)
        return 1
    
    try:
        result = compare_dark_rectangles(ground_truth_png, cache_png)
        
        print(f"\nDark rectangle analysis:")
        print(f"  Ground truth only: {len(result.image1_only)} rectangle(s)")
        for rect in result.image1_only:
            print(f"    {rect}")
        
        print(f"  Cache only: {len(result.image2_only)} rectangle(s)")
        for rect in result.image2_only:
            print(f"    {rect}")
        
        if result.has_exclusive_rects:
            print(f"\n✗ CORRUPTION DETECTED: {result.total_exclusive_rects} exclusive dark rectangles")
            return 1
        else:
            print("\n✓ No corruption: No exclusive dark rectangles found")
            return 0
    
    except PreflightError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
