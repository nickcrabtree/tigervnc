#!/usr/bin/env python3
"""WAN-shaped 4K black-box screenshot test (tiled logos + Tight/JPEG).

Goal: reproduce corruption that appears on large displays over slow links.

This wrapper:
- Applies a high-latency / low-bandwidth WAN profile via tc netem.
- Runs the shared black-box screenshot harness against a deterministic,
  repetitive content pattern (tiled TigerVNC logos).
- Forces lossy Tight/JPEG encoding to exercise the suspected WAN/JPEG path.
- Uses a 4K remote desktop geometry (3840x2160).
- Uses a 2x-wide viewer layout so each of the two viewer windows is 4K.

If cache-induced corruption still exists, this test should fail with a
visual mismatch and ideally the "large localized color shifts" detector.
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
    runner = here / "run_black_box_screenshot_test.py"

    if not runner.is_file():
        print(f"ERROR: black-box screenshot runner not found: {runner}", file=sys.stderr)
        return 1

    # WAN shaping requires CAP_NET_ADMIN (typically root). Prefer to run the
    # test unprivileged and delegate shaping to the privileged helper via sudo.
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

    # Ensure we do not go through the external helper path when running as root.
    if os.geteuid() == 0:
        os.environ.pop("TIGERVNC_WAN_HELPER", None)

    # 4K target.
    content_w = 3840
    content_h = 2160

    # Two viewers side-by-side; allow for the runner's fixed 40px top margin.
    viewer_layout_w = content_w * 2
    viewer_layout_h = content_h + 40

    # High-latency, low-bandwidth profile suitable for WAN-style testing.
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
            "--checkpoints",
            "3",
            "--viewer-geometry",
            f"{viewer_layout_w}x{viewer_layout_h}",
            "--viewer-display-geometry",
            f"{viewer_layout_w}x{viewer_layout_h}",
            "--content-geometry",
            f"{content_w}x{content_h}",
        ] + sys.argv[1:]

        completed = subprocess.run(argv)
        return completed.returncode

    finally:
        if shaping_active:
            clear_wan_shaping(dev=wan_dev, verbose=False)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
