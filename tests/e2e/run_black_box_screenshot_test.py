#!/usr/bin/env python3
"""Black-box screenshot-based end-to-end test for display corruption.

Runs two independent C++ viewers against the same deterministic content
server scenario and periodically captures screenshots of their windows.

Viewer A (ground truth): caches disabled.
Viewer B (under test): caches configured via --mode.

Any pixel-level mismatch between the two viewer screenshots is treated
as display corruption.
"""

from __future__ import annotations

import argparse
import os
import sys
import threading
import time
from pathlib import Path
from typing import Tuple

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    ArtifactManager,
    PreflightError,
    ProcessTracker,
    PROJECT_ROOT,
    BUILD_DIR,
    VNCServer,
    check_display_available,
    check_port_available,
    preflight_check_cpp_only,
)
from scenarios import ScenarioRunner
from screenshot_compare import compare_screenshots


def _select_server_mode() -> str:
    """Choose server mode: local Xnjcvnc if available, else system Xtigervnc."""

    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    if local_server_symlink.exists() or local_server_actual.exists():
        return "local"
    return "system"


def _ensure_screenshot_tool(binaries) -> str:
    """Ensure at least one screenshot tool is available.

    Returns a string identifying which backend to use: "xwd+convert" or "import".
    """

    has_xwd = "xwd" in binaries
    has_convert = "convert" in binaries

    if has_xwd and has_convert:
        return "xwd+convert"

    # Fallback: detect ImageMagick import directly via PATH
    from shutil import which

    if which("import") is not None:
        return "import"

    raise PreflightError(
        "No suitable screenshot tool found. Install ImageMagick (xwd+convert or import)."
    )


def _run_cpp_viewer(
    viewer_path: str,
    port: int,
    artifacts: ArtifactManager,
    tracker: ProcessTracker,
    name: str,
    display_for_viewer: int,
    content_cache: int,
    persistent_cache: int,
    persistent_cache_size_mb: int,
    lossless: bool = False,
):
    """Launch the C++ viewer with specified cache configuration.

    The viewer's encoding behaviour is controlled purely via its own
    command-line/configuration parameters (PreferredEncoding, NoJPEG,
    QualityLevel, etc.), rather than hard-wired in the harness.
    """

    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        "Shared=1",
        # Use a lower log level to avoid enabling DesktopWindow debug graph
        "Log=*:stderr:30",
        f"ContentCache={content_cache}",
        f"PersistentCache={persistent_cache}",
    ]

    if persistent_cache:
        cmd.append(f"PersistentCacheSize={persistent_cache_size_mb}")

    # Optional lossless mode: configure viewer via its own parameters
    # to prefer a lossless encoding path (ZRLE, no JPEG, full colour).
    if lossless:
        cmd.extend([
            "AutoSelect=0",            # disable auto tuning
            "PreferredEncoding=ZRLE",  # force lossless ZRLE
            "FullColor=1",             # keep full colour
            "NoJPEG=1",                # disable lossy JPEG in Tight
            "QualityLevel=9",          # highest quality if JPEG ever used
        ])

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()
    env["DISPLAY"] = f":{display_for_viewer}"

    print(f"  Starting {name} (ContentCache={content_cache}, PersistentCache={persistent_cache})...")
    log_file = open(log_path, "w")

    proc = os.spawnlp  # placeholder to satisfy type checkers
    import subprocess

    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env,
    )

    tracker.register(name, proc)
    time.sleep(2.0)
    return proc


def _find_window_id_for_pid(display: int, pid: int) -> str:
    """Use xdotool to find a window ID for the given PID on the display."""

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}
    result = subprocess.run(
        ["xdotool", "search", "--pid", str(pid)],
        env=env,
        capture_output=True,
        text=True,
        timeout=10.0,
    )
    if result.returncode != 0:
        raise PreflightError(
            f"xdotool could not find a window for viewer PID {pid}: {result.stderr.strip()}"
        )

    lines = [l for l in result.stdout.splitlines() if l.strip()]
    if not lines:
        raise PreflightError(f"No window IDs returned for viewer PID {pid}")

    return lines[0].strip()


def _arrange_viewer_windows(
    display: int,
    win_a: str,
    win_b: str,
    total_width: int,
    total_height: int,
) -> None:
    """Place two viewer windows side-by-side on the viewer display."""

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}

    half_width = total_width // 2

    for win_id, x in ((win_a, 0), (win_b, half_width)):
        cmd = [
            "wmctrl",
            "-i",
            "-r",
            win_id,
            "-e",
            f"0,{x},0,{half_width},{total_height}",
        ]
        result = subprocess.run(
            cmd,
            env=env,
            capture_output=True,
            text=True,
            timeout=10.0,
        )
        if result.returncode != 0:
            raise PreflightError(
                f"Failed to move/resize window {win_id}: {result.stderr.strip()}"
            )

    # Give WM time to settle
    time.sleep(1.0)


def _capture_window(
    display: int,
    backend: str,
    win_id: str,
    outfile: Path,
) -> Path:
    """Capture a screenshot of a single viewer window (client area).

    This avoids including window-manager decorations and background,
    reducing spurious differences between viewers.
    """

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}
    outfile = outfile.with_suffix(".png")
    outfile.parent.mkdir(parents=True, exist_ok=True)

    if backend == "xwd+convert":
        cmd = f"xwd -silent -id {win_id} | convert xwd:- png:{outfile}"
        result = subprocess.run(
            cmd,
            shell=True,
            env=env,
            capture_output=True,
            text=True,
            timeout=60.0,
        )
    elif backend == "import":
        cmd = ["import", "-window", win_id, str(outfile)]
        result = subprocess.run(
            cmd,
            env=env,
            capture_output=True,
            text=True,
            timeout=60.0,
        )
    else:
        raise PreflightError(f"Unknown screenshot backend: {backend}")

    if result.returncode != 0:
        raise PreflightError(
            f"Screenshot capture failed for window {win_id} (backend={backend}): {result.stderr.strip()}"
        )

    # Crop away a small border to avoid window frame / rounding artefacts
    try:
        from PIL import Image  # type: ignore

        img = Image.open(outfile).convert("RGBA")
        w, h = img.size
        margin = 4
        if w > 2 * margin and h > 2 * margin:
            cropped = img.crop((margin, margin, w - margin, h - margin))
            cropped.save(outfile)
    except Exception:
        # If cropping fails for any reason, keep the original image
        pass

    return outfile


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Black-box screenshot-based end-to-end test for display corruption",
    )
    parser.add_argument(
        "--display-content",
        type=int,
        default=998,
        help="Display number for content server (default: 998)",
    )
    parser.add_argument(
        "--port-content",
        type=int,
        default=6898,
        help="Port for content server (default: 6898)",
    )
    parser.add_argument(
        "--display-viewer",
        type=int,
        default=999,
        help="Display number for viewer windows (default: 999)",
    )
    parser.add_argument(
        "--port-viewer",
        type=int,
        default=6899,
        help="Port for viewer window server (default: 6899)",
    )
    parser.add_argument(
        "--duration",
        type=int,
        default=90,
        help="Scenario duration in seconds (default: 90)",
    )
    parser.add_argument(
        "--wm", default="openbox", help="Window manager (default: openbox)"
    )
    parser.add_argument(
        "--checkpoints",
        type=int,
        default=6,
        help="Number of screenshot checkpoints (default: 6)",
    )
    parser.add_argument(
        "--mode",
        default="persistent",
        choices=["none", "content", "persistent", "both"],
        help=(
            "Cache mode under test: "
            "none (both off), content, persistent, both (default: persistent)"
        ),
    )
    parser.add_argument(
        "--viewer-geometry",
        default="1600x1000",
        help="Geometry for viewer window display (default: 1600x1000)",
    )
    parser.add_argument(
        "--lossless",
        action="store_true",
        help=(
            "Force both viewers to request lossless encodings via their own "
            "command-line options (PreferredEncoding=ZRLE, NoJPEG, etc.)",
        ),
    )
    parser.add_argument(
        "--verbose", action="store_true", help="Verbose output",
    )

    args = parser.parse_args()

    print("=" * 70)
    print("Black-Box Screenshot Test (C++ viewer vs C++ viewer)")
    print("=" * 70)
    print(f"Mode: {args.mode}")
    print(f"Duration: {args.duration}s, Checkpoints: {args.checkpoints}")
    print()

    print("[1/8] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    print("\n[2/8] Running preflight checks (C++ viewer only)...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
        screenshot_backend = _ensure_screenshot_tool(binaries)
    except PreflightError as e:
        print("\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1

    # Check ports/displays
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

    print("✓ All preflight checks passed")

    tracker = ProcessTracker()
    server_mode = _select_server_mode()
    print(f"\nServer mode: {server_mode}")

    # Parse viewer geometry (WIDTHxHEIGHT)
    try:
        geom_parts = args.viewer_geometry.lower().split("x")
        viewer_width = int(geom_parts[0])
        viewer_height = int(geom_parts[1])
    except Exception as exc:  # pragma: no cover - defensive
        print(f"Invalid viewer geometry '{args.viewer_geometry}': {exc}")
        return 1

    try:
        # 3. Start content server
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "bb_content",
            artifacts,
            tracker,
            geometry=f"{viewer_width}x{viewer_height}",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        # 4. Start viewer window server
        print(f"\n[4/8] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "bb_viewerwin",
            artifacts,
            tracker,
            geometry=f"{viewer_width}x{viewer_height}",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer.start():
            print("\n✗ FAIL: Could not start viewer window server")
            return 1
        if not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server session")
            return 1
        print("✓ Viewer window server ready")

        # 5. Launch two viewers with appropriate cache settings
        print("\n[5/8] Launching viewers...")
        cpp_viewer = binaries["cpp_viewer"]

        # Mode mapping for viewer B
        if args.mode == "none":
            v_b_content = 0
            v_b_persistent = 0
        elif args.mode == "content":
            v_b_content = 1
            v_b_persistent = 0
        elif args.mode == "persistent":
            v_b_content = 0
            v_b_persistent = 1
        else:  # both
            v_b_content = 1
            v_b_persistent = 1

        viewer_ground = _run_cpp_viewer(
            cpp_viewer,
            args.port_content,
            artifacts,
            tracker,
            "viewer_ground_truth",
            display_for_viewer=args.display_viewer,
            content_cache=0,
            persistent_cache=0,
            persistent_cache_size_mb=256,
            lossless=args.lossless,
        )
        if viewer_ground.poll() is not None:
            print("\n✗ FAIL: Ground-truth viewer exited prematurely")
            return 1

        viewer_cache = _run_cpp_viewer(
            cpp_viewer,
            args.port_content,
            artifacts,
            tracker,
            "viewer_cache",
            display_for_viewer=args.display_viewer,
            content_cache=v_b_content,
            persistent_cache=v_b_persistent,
            persistent_cache_size_mb=256,
            lossless=args.lossless,
        )
        if viewer_cache.poll() is not None:
            print("\n✗ FAIL: Cache-on viewer exited prematurely")
            return 1

        # Find and arrange viewer windows
        print("  Arranging viewer windows side-by-side...")
        win_ground = _find_window_id_for_pid(args.display_viewer, viewer_ground.pid)
        win_cache = _find_window_id_for_pid(args.display_viewer, viewer_cache.pid)
        _arrange_viewer_windows(args.display_viewer, win_ground, win_cache, viewer_width, viewer_height)
        print("  Viewer windows arranged")

        # 6. Start scenario on content server in background
        print("\n[6/8] Starting scenario on content server...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)

        def _scenario_thread_body():
            try:
                runner.cache_hits_minimal(duration_sec=args.duration)
            finally:
                runner.cleanup()

        scenario_thread = threading.Thread(target=_scenario_thread_body, daemon=True)
        scenario_thread.start()

        # 7. Capture screenshots at checkpoints
        print("\n[7/8] Capturing screenshots at checkpoints...")
        if args.checkpoints <= 0:
            print("No checkpoints requested; nothing to capture")
            checkpoints = 0
        else:
            checkpoints = args.checkpoints

        # Simple timing strategy: evenly spaced throughout duration, with margin
        min_interval = 3.0
        interval = max(min_interval, args.duration / float(checkpoints + 1)) if checkpoints else 0

        checkpoint_paths = []
        for i in range(1, checkpoints + 1):
            sleep_time = interval
            print(f"  Waiting {sleep_time:.1f}s before checkpoint {i}...")
            time.sleep(sleep_time)

            gt_path = artifacts.screenshots_dir / f"checkpoint_{i}_ground_truth.png"
            cache_path = artifacts.screenshots_dir / f"checkpoint_{i}_cache.png"

            gt_path = _capture_window(
                args.display_viewer,
                screenshot_backend,
                win_ground,
                gt_path,
            )
            cache_path = _capture_window(
                args.display_viewer,
                screenshot_backend,
                win_cache,
                cache_path,
            )

            checkpoint_paths.append((gt_path, cache_path))
            print(f"  Captured checkpoint {i}")

        # Wait for scenario to finish (with a bit of slack)
        scenario_thread.join(timeout=args.duration + 30.0)
        if scenario_thread.is_alive():
            print("\n✗ FAIL: Scenario thread did not finish in expected time")
            return 1

        # 8. Compare screenshots
        print("\n[8/8] Comparing screenshots...")
        first_failure = None
        for idx, (gt_path, cache_path) in enumerate(checkpoint_paths, start=1):
            diff_png = artifacts.screenshots_dir / f"checkpoint_{idx}_diff.png"
            diff_json = artifacts.reports_dir / f"checkpoint_{idx}_diff.json"

            # Ignore small regions where the local cursor or stats widget live.
            # Coordinates are in window space; tuned for 800x1000-ish windows but
            # normalised and clipped inside compare_screenshots.
            ignore_rects = [
                # Bottom-left corner: mouse cursor
                (0, viewer_height - 200, 160, viewer_height - 1),
                # Bottom-right corner: network stats widget
                (viewer_width - 320, viewer_height - 220, viewer_width - 1, viewer_height - 1),
            ]

            result = compare_screenshots(
                gt_path,
                cache_path,
                diff_out=diff_png,
                json_out=diff_json,
                ignore_rects=ignore_rects,
            )

            if result.identical:
                print(f"  Checkpoint {idx}: OK (identical)")
            else:
                print(
                    f"  Checkpoint {idx}: MISMATCH - "
                    f"{result.diff_pixels} pixels differ ({result.diff_pct:.4f}%)."
                )
                if first_failure is None:
                    first_failure = (idx, result)

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"All artifacts saved to: {artifacts.base_dir}")
        print(f"  Logs: {artifacts.logs_dir}")
        print(f"  Screenshots: {artifacts.screenshots_dir}")
        print(f"  Reports: {artifacts.reports_dir}")

        if first_failure is None:
            print("\n" + "=" * 70)
            print("✓ TEST PASSED (all checkpoints identical)")
            print("=" * 70)
            return 0

        idx, res = first_failure
        print("\n" + "=" * 70)
        print("✗ TEST FAILED (visual mismatch detected)")
        print("=" * 70)
        print(
            f"First failing checkpoint: {idx} "
            f"({res.diff_pixels} / {res.total_pixels} pixels differ, {res.diff_pct:.4f}% pixels)"
        )
        print(
            "See diff image and JSON summary under "
            f"{artifacts.screenshots_dir} and {artifacts.reports_dir}."
        )
        return 1

    except KeyboardInterrupt:
        print("\nInterrupted by user")
        return 130
    except PreflightError as e:
        print(f"\n✗ FAIL: {e}")
        return 1
    except Exception as e:  # pragma: no cover - defensive
        print(f"\n✗ FAIL: Unexpected error: {e}")
        import traceback

        traceback.print_exc()
        return 1
    finally:
        print("\nCleaning up...")
        tracker.cleanup_all()

        # More aggressive cleanup for our dedicated high-number test displays.
        # These displays/ports are reserved for tests, so we can safely
        # remove any leftover X11 lock/socket files if no server process
        # is actually running.
        try:
            for display in (args.display_content, args.display_viewer):
                socket_path = Path(f"/tmp/.X11-unix/X{display}")
                lock_path = Path(f"/tmp/.X{display}-lock")
                # If either artifact exists but no X server is running for
                # this display, it is safe to remove.
                if socket_path.exists() or lock_path.exists():
                    # Check for a matching Xtigervnc/Xnjcvnc process
                    import subprocess
                    pattern = f":{display}"
                    result = subprocess.run(
                        ["ps", "aux"],
                        capture_output=True,
                        text=True,
                        timeout=5.0,
                    )
                    has_server = False
                    if result.returncode == 0:
                        for line in result.stdout.splitlines():
                            if ("Xtigervnc" in line or "Xnjcvnc" in line) and pattern in line:
                                has_server = True
                                break
                    if not has_server:
                        try:
                            if socket_path.exists():
                                socket_path.unlink()
                            if lock_path.exists():
                                lock_path.unlink()
                        except Exception:
                            # Best-effort; ignore failures
                            pass

        except Exception:
            # Best-effort only; do not fail the test on aggressive cleanup errors
            pass

        print("✓ Cleanup complete")

        # Sanity check: warn if our ports still appear to be in use.
        try:
            if not check_port_available(args.port_content):
                print(f"⚠ Note: port {args.port_content} still appears to be in use after cleanup.")
            if not check_port_available(args.port_viewer):
                print(f"⚠ Note: port {args.port_viewer} still appears to be in use after cleanup.")
        except Exception:
            # Best-effort only; do not fail the test on check errors
            pass


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main())
