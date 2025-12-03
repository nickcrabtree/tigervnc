#!/usr/bin/env python3
"""Wrapper for the black-box screenshot parity test (cache scenario).

This runs the C++ vs C++ screenshot comparison using the existing
run_black_box_screenshot_test.py harness with the default cache scenario.

It is invoked automatically by run_tests.sh via the test_*.py pattern.
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

    # Defer all heavy lifting to the shared runner.
    argv = [sys.executable, str(runner)] + sys.argv[1:]
    os.execv(sys.executable, argv)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
