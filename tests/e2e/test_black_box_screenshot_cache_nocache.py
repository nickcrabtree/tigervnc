#!/usr/bin/env python3
"""Baseline black-box screenshot test (cache scenario, no caches).

Runs the same cache-focused black-box screenshot scenario as
``test_black_box_screenshot_cache.py`` but forces ``--mode none`` so
that both C++ viewers run with ContentCache=0 and PersistentCache=0.

This provides a "no-cache vs no-cache" baseline for mismatch rates in
scenarios where dynamic content (rather than the cache protocol) may be
responsible for visual differences.
"""

import os
import sys
from pathlib import Path


def main() -> int:
    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"
    if not runner.is_file():
        print(f"ERROR: black-box screenshot runner not found: {runner}", file=sys.stderr)
        return 1

    argv = [
        sys.executable,
        str(runner),
        "--mode",
        "none",  # both viewers: ContentCache=0, PersistentCache=0
        *sys.argv[1:],
    ]
    os.execv(sys.executable, argv)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())