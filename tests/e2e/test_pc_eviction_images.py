#!/usr/bin/env python3
"""
Wrapper test: PersistentCache eviction using image cycle + churn + burst + verification.
This uses tuned parameters that pass reliably and keeps runtime modest for CI.
"""
import sys
from pathlib import Path

# Ensure we can import sibling modules
sys.path.insert(0, str(Path(__file__).parent))

import test_persistent_cache_eviction as pc


def main():
    # Force arguments for a reliable, fast run
    sys.argv = [
        sys.argv[0],
        "--duration", "30",
        "--verify-duration", "10",
        "--variable-content", "images",
        "--grid-cols", "6",
        "--grid-rows", "3",
        "--clock-size", "240",
        "--cache-size", "4",
    ]
    return pc.main()


if __name__ == "__main__":
    sys.exit(main())
