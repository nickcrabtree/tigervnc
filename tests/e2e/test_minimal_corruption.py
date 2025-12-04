#!/usr/bin/env python3
"""Minimal test to reproduce screenshot corruption with caches disabled.

This is a stripped-down version of the black-box screenshot test that:
- Takes only ONE screenshot (at ~10 seconds)
- Uses static xterm content (faster than browser)
- Has both viewers with caches OFF
- Runs for minimal duration (15 seconds)

Usage:
    python3 tests/e2e/test_minimal_corruption.py
"""

import os
import sys
from pathlib import Path

def main() -> int:
    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"
    if not runner.is_file():
        print(f"ERROR: runner not found: {runner}", file=sys.stderr)
        return 1

    argv = [
        sys.executable,
        str(runner),
        # "--scenario", "cache",         # Default is cache (static xterm)
        "--mode", "none",                 # Both viewers: caches OFF
        "--duration", "15",               # Minimal duration
        "--checkpoints", "1",             # Single screenshot only
        *sys.argv[1:],
    ]
    os.execv(sys.executable, argv)

if __name__ == "__main__":
    raise SystemExit(main())
