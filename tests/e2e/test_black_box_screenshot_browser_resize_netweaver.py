#!/usr/bin/env python3
"""Black-box screenshot test: browser scroll with viewer resize (NetWeaver).

This test reuses the generic black-box screenshot harness but targets a
real-world long article (NetWeaver "Going Cookie Free") and exercises a
window-resize path that has been observed to cause display corruption in
practice.

Scenario outline:
- Start the browser scenario as in test_black_box_screenshot_browser.py,
  but open the NetWeaver article instead of bbc.com.
- Host the viewer windows on a slightly larger X server so they can be
  safely enlarged without hitting the edges of the display.
- After a few checkpoints, enlarge both viewer windows by ~20% in width
  and height, then continue scrolling and capturing screenshots.
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

    # We keep the logical viewer layout at 1600x900 but host the viewer
    # windows on a larger 3200x1800 X server so there is headroom to grow
    # the windows to roughly double width (and height) without hitting the
    # display boundaries.
    #
    # The resize is triggered after checkpoint 3 so that the first half of
    # the run exercises the initial size and the second half exercises the
    # substantially enlarged viewer, while the browser continues scrolling.
    argv = [
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
        "2.0",  # grow viewer windows by ~2x
        "--viewer-resize-at-checkpoint",
        "3",    # resize after checkpoint 3
        *sys.argv[1:],
    ]

    os.execv(sys.executable, argv)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
