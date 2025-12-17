#!/usr/bin/env python3
"""WAN-shaped 4K refresh-stability test (single viewer).

This test is designed to catch real-world corruption that can be cleared or
changed by requesting a full refresh.

It runs:
- 4K content server (3840x2160)
- 4K viewer window server (3840x2200, leaving 40px margin used by the harness)
- WAN shaping (satellite)
- Forced Tight/JPEG to exercise lossy behaviour

Pass criteria:
- Before/after refresh screenshots are perceptually similar and show no
  corruption patterns.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

# Ensure we can import the shared WAN helper from this directory.
sys.path.insert(0, str(Path(__file__).parent))

from wanem import apply_wan_profile, clear_wan_shaping


def main() -> int:
    here = Path(__file__).resolve().parent
    runner = here / "run_refresh_stability_test.py"

    if not runner.is_file():
        print(f"ERROR: refresh-stability runner not found: {runner}", file=sys.stderr)
        return 1

    if os.geteuid() != 0:
        can_sudo = (
            subprocess.run(
                ["sudo", "-n", "true"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            ).returncode
            == 0
        )
        if not can_sudo:
            print(
                "ERROR: Passwordless sudo not available; cannot enable WAN shaping for this test.",
                file=sys.stderr,
            )
            return 1

        if "TIGERVNC_WAN_HELPER" not in os.environ:
            helper = (Path(__file__).resolve().parent / "wanem_helper.py").resolve()
            os.environ["TIGERVNC_WAN_HELPER"] = f"sudo -n {sys.executable} {helper}"

    if os.geteuid() == 0:
        os.environ.pop("TIGERVNC_WAN_HELPER", None)

    content_w = 3840
    content_h = 2160

    viewer_w = content_w
    viewer_h = content_h + 40

    wan_profile = "satellite"
    wan_dev = os.environ.get("TIGERVNC_WAN_DEV", "lo")
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
                "ERROR: Failed to apply WAN profile 'satellite' (WAN shaping is required for this test).",
                file=sys.stderr,
            )
            return 1

        argv = [
            sys.executable,
            str(runner),
            "--mode",
            "persistent",
            "--scenario",
            "tiled_logos",
            "--force-tight-jpeg",
            "--duration",
            "90",
            "--capture-after-sec",
            "30",
            "--refresh-delay-sec",
            "8.0",
            "--content-geometry",
            f"{content_w}x{content_h}",
            "--viewer-display-geometry",
            f"{viewer_w}x{viewer_h}",
        ] + sys.argv[1:]

        completed = subprocess.run(argv)
        return completed.returncode

    finally:
        if shaping_active:
            clear_wan_shaping(dev=wan_dev, verbose=False)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
