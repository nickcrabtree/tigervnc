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
    here = Path(__file__).resolve().parent
    runner = here / "run_black_box_screenshot_test.py"

    def run_without_wan() -> int:
        if not runner.is_file():
            print(f"ERROR: black-box screenshot runner not found: {runner}", file=sys.stderr)
            return 1
        argv = [
            sys.executable,
            str(runner),
            "--mode",
            "persistent",
        ] + sys.argv[1:]
        completed = subprocess.run(argv)
        return completed.returncode

    # If we are not running as root, attempt passwordless sudo for WAN shaping.
    # If that is not available, fall back to running the screenshot test
    # without WAN emulation (do not fail the whole suite for missing sudoers).
    if os.geteuid() != 0:
        can_sudo = (
            subprocess.run(
                ["sudo", "-n", "true"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            ).returncode
            == 0
        )
        if can_sudo:
            script = Path(__file__).resolve()
            argv = ["sudo", "-n", sys.executable, str(script)] + sys.argv[1:]
            completed = subprocess.run(argv)
            return completed.returncode

        print(
            "WARNING: Passwordless sudo not available; running WAN screenshot test without WAN shaping.",
            file=sys.stderr,
        )
        return run_without_wan()

    # Ensure we do not go through the external helper path when running as
    # root; we want to talk to tc directly from this elevated invocation.
    os.environ.pop("TIGERVNC_WAN_HELPER", None)


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
            print(
                "WARNING: Failed to apply WAN profile 'satellite'; running without WAN shaping.",
                file=sys.stderr,
            )
            return run_without_wan()

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
