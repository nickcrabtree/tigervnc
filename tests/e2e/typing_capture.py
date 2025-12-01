#!/usr/bin/env python3
"""Interactive typing capture tool for production :2.

Run this inside an xterm on your real desktop session (e.g. :2). It will:

- Read keystrokes directly from stdin (raw mode)
- Echo them back to stdout so you can see what you type
- Log timestamp + character code for each keystroke to a log file you specify

You can then point the resize latency harness at that log to replay the
exact typing pattern on the disposable :998 test display.

Example usage (from the repo root, on :2):

  xterm -geometry 80x24+200+200 -e \
    python3 tests/e2e/typing_capture.py --log /tmp/typing_capture.log

Exit the capture with Ctrl-D.
"""

import argparse
import datetime
import sys
import termios
import tty
import time
from pathlib import Path


def capture(log_path: Path) -> None:
    fd = sys.stdin.fileno()
    old_settings = termios.tcgetattr(fd)

    log_path.parent.mkdir(parents=True, exist_ok=True)

    with log_path.open("a", buffering=1) as f:
        start_ts = datetime.datetime.now().isoformat()
        f.write(f"# typing_capture start {start_ts}\n")
        f.flush()

        try:
            tty.setraw(fd)
            while True:
                ch = sys.stdin.read(1)
                if not ch:
                    break

                # Ctrl-D: end capture session gracefully
                if ch == "\x04":
                    end_ts = datetime.datetime.now().isoformat()
                    f.write(f"# typing_capture end {end_ts}\n")
                    f.flush()
                    break

                ts = time.time()
                code = ord(ch)
                # Log: <unix_timestamp> <codepoint>
                f.write(f"{ts:.6f} {code}\n")
                f.flush()

                # Echo to the terminal so you see exactly what you type
                sys.stdout.write(ch)
                sys.stdout.flush()
        finally:
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Capture typing timestamps and characters for later replay",
    )
    parser.add_argument(
        "--log",
        required=True,
        help="Path to log file where keystrokes will be recorded",
    )

    args = parser.parse_args()
    log_path = Path(args.log).expanduser()

    capture(log_path)
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
