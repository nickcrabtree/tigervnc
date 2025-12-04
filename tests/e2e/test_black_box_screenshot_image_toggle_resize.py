#!/usr/bin/env python3
"""Black-box screenshot test: image toggle with viewer resize.

This test uses the black-box screenshot harness with the "image_toggle"
content scenario (two-picture fullscreen toggle) instead of the browser.

It is designed to reproduce resize-related corruption under a simpler,
more controlled content pattern:

- The content server runs the toggle_two_pictures_test scenario from
  scenarios_static.py on a 1920x1080 desktop.
- Two C++ viewers connect as usual (ground-truth vs cache-under-test).
- The viewers are initially arranged for a 1600x900 logical layout on
  the viewer display, which is itself 1920x1080.
- After a few screenshot checkpoints, both viewer windows are enlarged
  by ~20% in width and height, then the image toggling continues while
  further checkpoints are captured.
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

    # Keep the same geometry and resize pattern as the NetWeaver browser
    # resize test so that only the content scenario differs.
    argv = [
        sys.executable,
        str(runner),
        "--scenario",
        "image_toggle",
        "--viewer-geometry",
        "1600x900",
        "--viewer-display-geometry",
        "1920x1080",
        "--viewer-resize-factor",
        "1.2",  # grow viewer windows by ~20%
        "--viewer-resize-at-checkpoint",
        "3",    # resize after checkpoint 3
        *sys.argv[1:],
    ]

    os.execv(sys.executable, argv)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
