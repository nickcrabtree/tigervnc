#!/usr/bin/env python3
"""Verify that idle lossless refreshes use ZRLE (bit-perfect) rather than JPEG.

This test drives a short lossy Tight/JPEG session to populate the server's
lossyRegion, then allows the connection to go idle so that
VNCSConnectionST::writeLosslessRefresh() triggers EncodeManager's lossless
refresh path.

We enable TIGERVNC_CC_DEBUG on the server and parse its logs to find:

- At least one framebuffer update where `allowLossy=no` (i.e. lossless
  refresh path was executed); and
- Within such an update, rect encodings logged as `enc=16` (ZRLE) with no
  Tight encoding (enc=7).

This asserts that idle bandwidth is used to send bit-perfect ZRLE refresh
rectangles over previously JPEG-encoded regions.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import time
from pathlib import Path

# Add parent directory for framework imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    ArtifactManager,
    PreflightError,
    ProcessTracker,
    VNCServer,
    check_display_available,
    check_port_available,
    preflight_check_cpp_only,
)
from scenarios_static import StaticScenarioRunner


def _run_lossy_tight_viewer(
    viewer_path: str,
    port: int,
    artifacts: ArtifactManager,
    tracker: ProcessTracker,
    name: str,
    display_for_viewer: int,
) -> None:
    """Launch the C++ viewer configured for Tight+JPEG.

    We disable AutoSelect and force PreferredEncoding=Tight so that the
    server will choose Tight/TightJPEG for full-colour rectangles during
    normal updates. This ensures lossyRegion is populated before the
    lossless-refresh machinery runs.
    """

    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        "Shared=1",
        "Log=*:stderr:100",
        "AutoSelect=0",
        "PreferredEncoding=Tight",
        "FullColor=1",
        # Explicitly allow JPEG and use a mid/high quality level so the
        # server will treat these updates as genuinely lossy.
        "NoJPEG=0",
        "QualityLevel=6",
    ]

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()
    env["TIGERVNC_VIEWER_DEBUG_LOG"] = "1"
    env["DISPLAY"] = f":{display_for_viewer}"

    print(f"  Starting {name} (Tight+JPEG)...")
    log_file = open(log_path, "w", encoding="utf-8")

    import subprocess

    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env,
    )

    tracker.register(name, proc)
    # Give the viewer a moment to connect and create its window
    time.sleep(2.0)


def _analyze_server_log(log_path: Path) -> tuple[bool, list[str]]:
    """Return (success, diagnostics) based on server log analysis.

    success is True if we observe at least one update block with
    allowLossy=no that:
      - Has encoder lines (CCDBG SERVER PATH) for that block; and
      - Uses encoding 16 (ZRLE) and does not use encoding 7 (Tight).
    """

    diag: list[str] = []
    if not log_path.is_file():
        return False, [f"Server log not found: {log_path}"]

    blocks: list[tuple[bool, set[int]]] = []  # (allowLossy, encodings)
    current_allow_lossy: bool | None = None
    current_encs: set[int] = set()

    allow_re = re.compile(r"allowLossy=(yes|no)")
    # We consider any encoder diagnostic that includes "CCDBG ENCODER" and
    # an "enc=<num>" field; this is logged for every sub-rectangle once its
    # concrete encoding has been selected.
    enc_re = re.compile(r"CCDBG ENCODER:.*enc=(\d+)")

    with open(log_path, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            if "CC doUpdate begin" in line:
                # Flush previous block
                if current_allow_lossy is not None:
                    blocks.append((current_allow_lossy, set(current_encs)))
                    current_encs.clear()
                current_allow_lossy = None
                continue

            # allowLossy flag may be printed on the continuation line
            # following the CC doUpdate begin header, so we look for it
            # on any subsequent line while a block is open.
            if current_allow_lossy is None:
                m_allow = allow_re.search(line)
                if m_allow:
                    current_allow_lossy = (m_allow.group(1) == "yes")

            if "CCDBG ENCODER" in line and current_allow_lossy is not None:
                m = enc_re.search(line)
                if m:
                    try:
                        enc = int(m.group(1))
                        current_encs.add(enc)
                    except ValueError:
                        diag.append(f"WARN: failed to parse encoding from line: {line.strip()}")

    if current_allow_lossy is not None:
        blocks.append((current_allow_lossy, set(current_encs)))

    if not blocks:
        diag.append("No CC doUpdate blocks found in server log (is TIGERVNC_CC_DEBUG enabled?)")
        return False, diag

    lossless_blocks = [encs for allow, encs in blocks if not allow]
    if not lossless_blocks:
        diag.append("No allowLossy=no blocks found; lossless refresh path may not have run.")
        return False, diag

    # Evaluate each lossless block: we require at least one where ZRLE (16)
    # appears and Tight (7) does not.
    for idx, encs in enumerate(lossless_blocks, start=1):
        diag.append(f"Lossless block {idx}: encodings={sorted(encs)}")
        if 16 in encs and 7 not in encs:
            diag.append(
                "✓ Found lossless-refresh block using ZRLE (16) and no Tight (7)."
            )
            return True, diag

    diag.append(
        "No lossless-refresh block used pure ZRLE: expected enc=16 without enc=7 in at least one block."
    )
    return False, diag


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify idle lossless refresh uses ZRLE instead of JPEG",
    )
    parser.add_argument("--display-content", type=int, default=998)
    parser.add_argument("--port-content", type=int, default=6898)
    parser.add_argument("--display-viewer", type=int, default=999)
    parser.add_argument("--port-viewer", type=int, default=6899)
    parser.add_argument("--duration", type=int, default=10,
                        help="Duration of active scenario before idle (seconds)")
    parser.add_argument("--idle-wait", type=int, default=8,
                        help="Idle wait after scenario to allow lossless refresh (seconds)")
    parser.add_argument("--wm", default="openbox")
    parser.add_argument("--verbose", action="store_true")

    args = parser.parse_args()

    print("=" * 70)
    print("Lossless Refresh ZRLE Test")
    print("=" * 70)
    print(f"Active duration: {args.duration}s")
    print(f"Idle wait: {args.idle_wait}s")
    print()

    artifacts = ArtifactManager()
    artifacts.create()

    # Ensure CC debug logging is enabled so we see encoding and allowLossy info
    os.environ["TIGERVNC_CC_DEBUG"] = "1"

    print("[1/5] Running preflight checks (C++ viewer only)...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed\n{e}")
        return 1

    if not check_port_available(args.port_content):
        print(f"\n✗ FAIL: Port {args.port_content} already in use")
        return 1
    if not check_port_available(args.port_viewer):
        print(f"\n✗ FAIL: Port {args.port_viewer} already in use")
        return 1
    if not check_display_available(args.display_content):
        print(f"\n✗ FAIL: Display :{args.display_content} already in use")
        return 1
    if not check_display_available(args.display_viewer):
        print(f"\n✗ FAIL: Display :{args.display_viewer} already in use")
        return 1

    print("✓ Preflight checks passed")

    tracker = ProcessTracker()

    # Prefer local Xnjcvnc when available; otherwise fall back to system server.
    from framework import BUILD_DIR

    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    server_mode = (
        "local" if (local_server_symlink.exists() or local_server_actual.exists()) else "system"
    )

    try:
        print("\n[2/5] Starting VNC servers...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "lossless_refresh_content",
            artifacts,
            tracker,
            geometry="1024x768",
            log_level="*:stderr:100",
            server_choice=server_mode,
        )
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server")
            return 1

        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "lossless_refresh_viewerwin",
            artifacts,
            tracker,
            geometry="1024x768",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server")
            return 1

        print("✓ Servers ready")

        print("\n[3/5] Launching Tight+JPEG viewer...")
        _run_lossy_tight_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "lossless_refresh_viewer",
            display_for_viewer=args.display_viewer,
        )

        print("\n[4/5] Driving lossy activity then idling...")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        try:
            # Phase 1: large image burst to generate substantial lossy content
            print("  Phase 1: large image burst (512x512)...")
            runner.image_burst(count=6, size=512, cols=2, rows=3, interval_ms=200)
            time.sleep(2.0)

            # Phase 2: random fullscreen colours for some extra motion
            print("  Phase 2: fullscreen random colours...")
            runner.random_fullscreen_colors(
                duration_sec=max(4, int(args.duration * 0.5)),
                interval_sec=0.5,
            )
            time.sleep(1.5)
        finally:
            # Ensure any helper windows are cleaned up
            runner.cleanup()

        # Idle period to allow EncodeManager / VNCSConnectionST to schedule
        # and send lossless refresh updates.
        print(f"  Idling for {args.idle_wait}s to allow lossless refresh...")
        time.sleep(args.idle_wait)

        print("\n[5/5] Analyzing server log for ZRLE lossless refresh...")
        server_log = (
            artifacts.logs_dir
            / f"lossless_refresh_content_server_{args.display_content}.log"
        )
        success, diagnostics = _analyze_server_log(server_log)

        print("\n" + "=" * 70)
        print("LOG ANALYSIS")
        print("=" * 70)
        for line in diagnostics:
            print(line)

        print("\n" + "=" * 70)
        if success:
            print("✓ TEST PASSED: Idle lossless refresh used ZRLE (encoding 16) and no Tight (7).")
            print("=" * 70)
            return 0

        print("✗ TEST FAILED: did not observe expected ZRLE-only lossless refresh block.")
        print("=" * 70)
        return 1

    finally:
        print("\nCleaning up...")
        try:
            tracker.cleanup_all()
        except Exception:
            pass
        print("✓ Cleanup complete")


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
