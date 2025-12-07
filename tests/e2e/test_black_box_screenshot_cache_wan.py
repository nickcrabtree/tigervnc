#!/usr/bin/env python3
"""WAN-shaped variant of the black-box persistent cache screenshot test.

This thin wrapper enables a high-latency, low-bandwidth link (via the
`wanem` helper and Linux `tc netem`) and then invokes the existing
`run_black_box_screenshot_test.py` runner in `--mode persistent`.

It is picked up automatically by `run_tests.sh` via the `test_*.py`
pattern and is intended to reproduce WAN-induced corruption scenarios
under PersistentCache.

Notes
-----
- WAN shaping is applied only to the default test ports (6898/6899) on
  the loopback interface. If you override ports via CLI, shaping may not
  apply to the alternate ports.
- If `tc` is unavailable or CAP_NET_ADMIN privileges are missing, the
  test logs a warning and falls back to running without WAN emulation.
"""

from __future__ import annotations

import os
import sys
import subprocess
from pathlib import Path

# Ensure we can import the shared WAN helper from this directory.
sys.path.insert(0, str(Path(__file__).parent))

from wanem import apply_wan_profile, clear_wan_shaping


def main() -> int:
    # If we are not running as root, re-exec this script via sudo -n so that
    # tc/netem operations can succeed without requiring the entire test suite
    # to be invoked under sudo. A sudoers drop-in should grant NOPASSWD for
    # this exact command.
    if os.geteuid() != 0:
        script = Path(__file__).resolve()
        argv = ["sudo", "-n", sys.executable, str(script)] + sys.argv[1:]
        os.execvp("sudo", argv)

    # Ensure we do not go through the external helper path when running as
    # root; we want to talk to tc directly from this elevated invocation.
    os.environ.pop("TIGERVNC_WAN_HELPER", None)

    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"
    if not runner.is_file():
        print(f"ERROR: black-box screenshot runner not found: {runner}", file=sys.stderr)
        return 1

    # High-latency, low-bandwidth profile suitable for WAN-style testing.
    wan_profile = "satellite"
    wan_dev = os.environ.get("TIGERVNC_WAN_DEV", "lo")

    # Default ports used by the runner; shaping is applied to these only.
    # If the user overrides ports via CLI, WAN shaping may not affect them.
    shaped_ports = [6898, 6899]

    shaping_active = False
    try:
        shaping_active = apply_wan_profile(
            wan_profile,
            shaped_ports,
            dev=wan_dev,
            verbose=False,
        )
        if not shaping_active:
            # Fail fast rather than silently falling back to an unshaped
            # link. If WAN emulation components (tc, CAP_NET_ADMIN, etc.)
            # are missing or misconfigured, this test must fail so that
            # users do not get a false sense of confidence about
            # slow-link behaviour.
            print(
                "ERROR: Failed to apply WAN profile 'satellite' (required for this test).",
                file=sys.stderr,
            )
            return 1

        # Always force persistent-cache mode so this test is explicitly
        # about PersistentCache behaviour under WAN conditions.
        argv = [
            sys.executable,
            str(runner),
            "--mode",
            "persistent",
        ] + sys.argv[1:]

        completed = subprocess.run(argv)
        return completed.returncode

    finally:
        if shaping_active:
            clear_wan_shaping(dev=wan_dev, verbose=False)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
