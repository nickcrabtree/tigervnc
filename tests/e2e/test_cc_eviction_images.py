#!/usr/bin/env python3
"""
Wrapper test: ContentCache eviction using image-based churn + eviction burst + verification.
This uses tuned parameters that pass reliably and keeps runtime short for CI.
"""
import sys
from pathlib import Path

# Ensure we can import sibling modules
sys.path.insert(0, str(Path(__file__).parent))

import test_cache_eviction as cc


def main():
    # Force arguments for a reliable, fast run
    sys.argv = [
        sys.argv[0],
        "--duration", "20",
        "--verify-duration", "12",
        "--verify-tiles", "18",
        "--variable-content", "images",
        "--grid-cols", "8",
        "--grid-rows", "3",
        "--clock-size", "200",
    ]
    return cc.main()


if __name__ == "__main__":
    sys.exit(main())
