#!/usr/bin/env python3
"""Corruption test with lossy encoding (Tight/JPEG).

This extends the minimal corruption test to verify no visual artifacts
occur when using lossy (Tight/JPEG) encoding with caches disabled.

Validates:
- No pixel-level corruption with Tight encoding
- Both viewers produce identical output
- Lossy encoding doesn't introduce visual artifacts

This is a critical regression test for the cache system's lossy
encoding support. Any failure indicates visual corruption bugs.
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

    # Run black-box test with caches OFF but using --lossless flag
    # to force ZRLE/NoJPEG. The flag is already supported by the runner.
    # We test corruption WITHOUT this flag (allowing lossy Tight)
    # vs WITH this flag (forcing lossless).
    
    argv = [
        sys.executable,
        str(runner),
        "--mode", "none",                 # Both viewers: caches OFF
        "--duration", "15",               # Minimal duration
        "--checkpoints", "1",             # Single screenshot
        # NOTE: Do NOT use --lossless here - we want to test Tight/JPEG
        # The default encoding allows lossy Tight which is what we're testing
        *sys.argv[1:],
    ]
    os.execv(sys.executable, argv)

if __name__ == "__main__":
    raise SystemExit(main())
