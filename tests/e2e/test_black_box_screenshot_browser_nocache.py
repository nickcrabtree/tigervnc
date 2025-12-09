#!/usr/bin/env python3
"""Baseline black-box screenshot test (browser scenario, no caches).

Runs the same browser-focused black-box screenshot scenario as
``test_black_box_screenshot_browser.py`` but forces ``--mode none`` so
that both C++ viewers run with ContentCache=0 and PersistentCache=0.

This provides a "no-cache vs no-cache" baseline for mismatch rates in
highly dynamic browser content where differences are expected even
without any cache protocol enabled.
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
        "--scenario",
        "browser",
        "--mode",
        "none",  # both viewers: ContentCache=0, PersistentCache=0
        "--lossless",  # force deterministic, lossless encoding for strict pixel equality
        *sys.argv[1:],
    ]
    os.execv(sys.executable, argv)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())