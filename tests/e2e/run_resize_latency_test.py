#!/usr/bin/env python3
"""
Ad-hoc end-to-end harness to reproduce and inspect resize-induced latency
behaviour using the existing VNC-in-VNC e2e framework.

This script:
  * Starts a disposable content server on :998 and a viewer-window server on :999
  * Launches the C++ viewer against the content server
  * Runs an xrandr framebuffer resize inside the content server session
  * Collects the server log and prints a summary of EncodeManager updates
    immediately after the framebuffer size change (bbox + rect counts)

It is intended as a debugging aid to confirm/characterise the resize bug,
not as a stable pass/fail test.
"""

import argparse
import sys
import time
from pathlib import Path

# Add parent directory for framework imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (  # type: ignore
    preflight_check_cpp_only,
    PreflightError,
    ArtifactManager,
    ProcessTracker,
    VNCServer,
    check_port_available,
    check_display_available,
    BUILD_DIR,
)
from scenarios import ScenarioRunner
from scenarios_static import StaticScenarioRunner


def parse_post_resize_updates(log_path: Path, max_updates: int = 20):
    """Parse server log and summarise updates after the last FB size change.

    Returns a dict with:
      - fb_change_line: the framebuffer change line (or None)
      - examples: list of up to `max_updates` CC doUpdate lines after the change
      - metrics: dict with basic rect statistics (may be partially None)
    """
    import re

    if not log_path.exists():
        return {"fb_change_line": None, "examples": [], "metrics": {}}

    text = log_path.read_text(errors="replace")
    lines = text.splitlines()

    last_change_idx = None
    last_change_line = None
    fb_width = None
    fb_height = None

    for idx, line in enumerate(lines):
        if "Framebuffer size changed from" in line:
            last_change_idx = idx
            last_change_line = line
            # Try to infer new framebuffer rect: ... to [x,y-X,Y]
            m = re.search(r"to\s+\[(\d+),(\d+)-(\d+),(\d+)\]", line)
            if m:
                x1, y1, x2, y2 = map(int, m.groups())
                fb_width = x2 - x1
                fb_height = y2 - y1

    if last_change_idx is None:
        return {"fb_change_line": None, "examples": [], "metrics": {}}

    examples = []
    metrics = {
        "fb_width": fb_width,
        "fb_height": fb_height,
        "num_updates": 0,
        "max_rects": 0,
        "num_large_rects": 0,
    }

    for line in lines[last_change_idx + 1 :]:
        if "CC doUpdate begin" not in line:
            continue

        line_stripped = line.strip()
        examples.append(line_stripped)

        # Parse bbox and rects: bbox:(x,y wxh) rects:N
        m = re.search(r"bbox:\(([-\d]+),([-=\d]+) (\d+)x(\d+)\) rects:(\d+)", line)
        if not m:
            m = re.search(r"bbox:\(([-\d]+),([-\d]+) (\d+)x(\d+)\).*rects:(\d+)", line)
        if m:
            _, _, bw_s, bh_s, rects_s = m.groups()
            try:
                bw = int(bw_s)
                bh = int(bh_s)
                rects = int(rects_s)
            except ValueError:
                bw = bh = rects = 0
            metrics["num_updates"] += 1
            if rects > metrics["max_rects"]:
                metrics["max_rects"] = rects
            if fb_width and fb_height:
                # Treat as "large" if it covers most of the framebuffer
                if bw >= fb_width * 0.9 and bh >= fb_height * 0.8:
                    metrics["num_large_rects"] += 1

        if len(examples) >= max_updates:
            break

    return {"fb_change_line": last_change_line, "examples": examples, "metrics": metrics}


def run_cpp_viewer(viewer_path: str, port: int, artifacts, tracker, name: str,
                   display_for_viewer: int) -> None:
    """Launch the C++ viewer in a separate display, connected to given port."""
    import os
    import subprocess

    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        # Mirror iPad viewer behaviour: do not negotiate ContentCache or
        # PersistentCache, even if the server could support them.
        "ContentCache=0",
        "PersistentCache=0",
        "Shared=1",
        "Log=*:stderr:100",
    ]

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()
    env["DISPLAY"] = f":{display_for_viewer}"

    print(f"  Starting {name} on :{display_for_viewer} -> port {port}...")
    log_file = open(log_path, "w")

    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env,
    )

    tracker.register(name, proc)
    time.sleep(3.0)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Reproduce/inspect resize-induced latency using e2e harness",
    )
    parser.add_argument("--display-content", type=int, default=998,
                        help="Display number for content server (default: 998)")
    parser.add_argument("--port-content", type=int, default=6898,
                        help="Port for content server (default: 6898)")
    parser.add_argument("--display-viewer", type=int, default=999,
                        help="Display for viewer window (default: 999)")
    parser.add_argument("--port-viewer", type=int, default=6899,
                        help="Port for viewer window server (default: 6899)")
    parser.add_argument("--geometry-initial", default="1920x1080",
                        help="Initial framebuffer geometry (default: 1920x1080)")
    parser.add_argument("--geometry-resized", default="1366x1024",
                        help="Target framebuffer size for xrandr --fb (default: 1366x1024)")
    parser.add_argument("--wm", default="openbox",
                        help="Window manager for test displays (default: openbox)")
    parser.add_argument("--use-xstartup", action="store_true",
                        help="Run the content server session via ~/.config/tigervnc/xstartup (XFCE) instead of a simple WM")
    parser.add_argument("--duration-after-resize", type=int, default=20,
                        help="Seconds to idle after resize to collect updates (default: 20)")
    parser.add_argument("--typing-log", default=None,
                        help="Path to typing_capture log to replay instead of synthetic typing")
    parser.add_argument("--typing-speed-scale", type=float, default=1.0,
                        help="Scale factor for timing during typing_log replay (default: 1.0)")
    parser.add_argument("--verbose", action="store_true",
                        help="Verbose output")

    args = parser.parse_args()

    print("=" * 70)
    print("Resize Latency Debug Harness (e2e)")
    print("=" * 70)
    print(f"Initial geometry : {args.geometry_initial}")
    print(f"Resized geometry : {args.geometry_resized} (via xrandr --fb)")
    print(f"Post-resize idle : {args.duration_after_resize}s")
    print()

    # 1. Artifacts
    print("[1/6] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight
    print("\n[2/6] Running preflight checks (C++ viewer only)...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
    except PreflightError as e:
        print("\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1

    # Ports/displays
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

    print("✓ Preflight OK")

    tracker = ProcessTracker()

    # Server mode: require local Xnjcvnc; no fallback to system Xtigervnc.
    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = "local"
        print("\nUsing local Xnjcvnc server (required)")
    else:
        print("\n✗ FAIL: Local Xnjcvnc server not found under build tree; this harness does not use system Xtigervnc")
        return 1

    try:
        # 3. Start content server
        print(f"\n[3/6] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "resize_content",
            artifacts,
            tracker,
            geometry=args.geometry_initial,
            log_level="*:stderr:100",
            server_choice=server_mode,
            # Disable server-side PersistentCache; ContentCache will remain
            # effectively unused because the viewer does not negotiate the
            # corresponding pseudo-encodings.
            server_params={
                "EnablePersistentCache": "0",
            },
        )
        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        # Optionally mirror the real :2 desktop session via xstartup for the
        # content server only. The viewer window server can keep a simple WM.
        wm_content = "xstartup" if args.use_xstartup else args.wm
        if not server_content.start_session(wm=wm_content):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        # 4. Start viewer window server
        print(f"\n[4/6] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "resize_viewerwin",
            artifacts,
            tracker,
            geometry=args.geometry_initial,
            log_level="*:stderr:30",
            server_choice=server_mode,
            server_params={
                "EnablePersistentCache": "0",
            },
        )
        if not server_viewer.start():
            print("\n✗ FAIL: Could not start viewer window server")
            return 1
        if not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server session")
            return 1
        print("✓ Viewer window server ready")

        # 5. Launch C++ viewer (external) displayed on :display-viewer, connected to content port
        print("\n[5/7] Launching C++ viewer...")
        run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "resize_cpp_viewer",
            display_for_viewer=args.display_viewer,
        )

        # Give viewer a moment to stabilise
        time.sleep(3.0)

        # 6. Drive a typing-focused scenario before resize. If a
        #    typing_capture log is provided, replay that exact pattern;
        #    otherwise fall back to synthetic typing_stress.
        print("\\n[6/7] Running pre-resize typing scenario on content server...")
        scenario_runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        if args.typing_log:
            scenario_runner.typing_replay_from_log(args.typing_log, speed_scale=args.typing_speed_scale)
        else:
            scenario_runner.typing_stress(duration_sec=20.0, delay_ms=80)

        # 7. Run xrandr resize inside an xterm on the content display, then
        #    drive content again. This makes the resize command visible in the
        #    viewer (xterm window) while it executes.
        print("\\n[7/7] Applying framebuffer resize via xterm + xrandr --fb and running post-resize logos...")
        resize_cmd = [
            "xterm",
            "-geometry",
            "100x30+0+0",
            "-e",
            "bash",
            "-lc",
            "date; xrandr --output VNC-0 --fb 1366x1024 || echo 'xrandr failed'; "
            f"sleep {args.duration_after_resize}; date",
        ]
        resize_proc = server_content.run_in_display(resize_cmd, "resize_fb")
        resize_proc.wait(timeout=args.duration_after_resize + 10)

        # After resize + idle, exercise the server again with the same
        # typing scenario (replay or synthetic) to approximate continued
        # interactive use while the resized framebuffer is in effect.
        if args.typing_log:
            scenario_runner.typing_replay_from_log(args.typing_log, speed_scale=args.typing_speed_scale)
        else:
            scenario_runner.typing_stress(duration_sec=20.0, delay_ms=80)

        print("✓ Resize and post-resize content run completed; collecting logs")

    finally:
        # Always clean up processes
        tracker.cleanup_all()

    # Analyse server log
    server_log = artifacts.logs_dir / f"resize_content_server_{args.display_content}.log"
    summary = parse_post_resize_updates(server_log)

    print("\n" + "=" * 70)
    print("Post-resize server log summary")
    print("=" * 70)

    if summary["fb_change_line"] is None:
        print("No 'Framebuffer size changed' line found in server log.")
        print(f"Log path: {server_log}")
        return 0

    print("Framebuffer size change:")
    print(f"  {summary['fb_change_line']}")

    examples = summary["examples"]
    if not examples:
        print("\nNo 'CC doUpdate begin' entries found after size change.")
        print(f"Check full log at: {server_log}")
        return 0

    metrics = summary.get("metrics", {})
    fb_w = metrics.get("fb_width")
    fb_h = metrics.get("fb_height")

    print("\nFirst few CC doUpdate entries after size change:")
    for line in examples:
        print(f"  {line}")

    if metrics:
        print("\nPost-resize CC doUpdate metrics:")
        if fb_w and fb_h:
            print(f"  FB size         : {fb_w}x{fb_h}")
        print(f"  Updates parsed  : {metrics.get('num_updates', 0)}")
        print(f"  Max rects/update: {metrics.get('max_rects', 0)}")
        print(f"  Large full-FB-ish updates: {metrics.get('num_large_rects', 0)}")

    print("\n(Use this as a baseline to characterise or regression-test the resize bug.)")
    print(f"Full server log: {server_log}")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
