#!/usr/bin/env python3
"""Black-box screenshot-based end-to-end test for display corruption.

Runs two independent C++ viewers against the same deterministic content
server scenario and periodically captures screenshots of their windows.

Viewer A (ground truth): caches disabled.
Viewer B (under test): caches configured via --mode.

Any pixel-level mismatch between the two viewer screenshots is treated
as display corruption.

Separate wrapper scripts can also invoke this runner in ``--mode none`` so
that both viewers run with all caches disabled. Those "no-cache vs
no-cache" runs provide a baseline for inherent scenario differences
(e.g. dynamic browser content) independent of the cache protocol.
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
from scenarios_static import StaticScenarioRunner
from screenshot_compare import compare_screenshots
from PIL import Image


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


# Track window IDs we have already associated with specific viewers so that
# fallback logic can pick distinct windows when PID-based lookup fails.
_assigned_viewer_windows: set[str] = set()


def _find_window_id_for_pid(display: int, pid: int) -> str:
    """Find a viewer window ID on the given display.

    Primary strategy:
    - Use xdotool search --pid to locate a window whose _NET_WM_PID matches
      the viewer process. This is retried for a short period to handle
      asynchronous window mapping.

    Fallback strategy:
    - If no window is ever associated with that PID (for example, if the
      viewer spawns a helper process that owns the window), fall back to
      parsing wmctrl -lp output on the same display and pick a suitable
      candidate window that we have not already assigned.
    """

    import subprocess
    import time

    env = {**os.environ, "DISPLAY": f":{display}"}
    deadline = time.time() + 15.0
    last_stderr = ""

    # First try the PID-based lookup, which is the most precise when it works.
    while time.time() < deadline:
        result = subprocess.run(
            ["xdotool", "search", "--pid", str(pid)],
            env=env,
            capture_output=True,
            text=True,
            timeout=5.0,
        )

        stdout = result.stdout.strip()
        stderr = result.stderr.strip()

        if result.returncode == 0 and stdout:
            lines = [l for l in stdout.splitlines() if l.strip()]
            if lines:
                win_id = lines[0].strip()
                _assigned_viewer_windows.add(win_id)
                return win_id

        if stderr:
            last_stderr = stderr

        time.sleep(0.5)

    # PID-based lookup failed; fall back to wmctrl-based discovery on this
    # dedicated viewer display. We expect only the two viewer windows plus
    # a tiny helper xterm named "bb_focus_sentinel".
    try:
        result = subprocess.run(
            ["wmctrl", "-lp"],
            env=env,
            capture_output=True,
            text=True,
            timeout=5.0,
        )
    except Exception as exc:
        detail = f" (wmctrl fallback failed: {exc})"
        detail_err = f": {last_stderr}" if last_stderr else detail
        raise PreflightError(
            f"Failed to locate a window for viewer PID {pid}{detail_err}"
        ) from exc

    if result.returncode != 0:
        stderr = result.stderr.strip()
        detail = f": {stderr}" if stderr else ""
        raise PreflightError(
            f"wmctrl could not list windows on display :{display}{detail}"
        )

    candidates: list[tuple[str, int, str]] = []  # (win_id, pid_col, title)
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split(None, 4)
        if len(parts) < 5:
            continue
        win_id_hex, _desktop, pid_str, _host, title = parts
        # Skip the tiny helper xterm we create to hold focus/cursor.
        if "bb_focus_sentinel" in title:
            continue
        try:
            pid_col = int(pid_str)
        except ValueError:
            pid_col = -1
        candidates.append((win_id_hex, pid_col, title))

    # 1) Prefer any window whose wmctrl PID column matches the viewer PID.
    for win_id_hex, pid_col, _title in candidates:
        if pid_col == pid and win_id_hex not in _assigned_viewer_windows:
            _assigned_viewer_windows.add(win_id_hex)
            return win_id_hex

    # 2) Otherwise, fall back to the first unassigned non-helper window.
    for win_id_hex, _pid_col, _title in candidates:
        if win_id_hex not in _assigned_viewer_windows:
            _assigned_viewer_windows.add(win_id_hex)
            return win_id_hex

    detail = f": {last_stderr}" if last_stderr else ""
    raise PreflightError(
        f"xdotool/wmctrl could not find a suitable window for viewer PID {pid}{detail}"
    )


def _arrange_viewer_windows(
    display: int,
    win_a: str,
    win_b: str,
    total_width: int,
    total_height: int,
) -> None:
    """Place two viewer windows side-by-side on the viewer display.

    We intentionally leave a small margin at the top of the display so that
    a tiny helper window can own the keyboard focus and mouse cursor without
    overlapping the viewer windows. This keeps both viewers unfocused and
    the local cursor outside their client areas during screenshot capture.
    """

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}

    half_width = total_width // 2
    margin_top = 40
    usable_height = max(1, total_height - margin_top)

    for win_id, x in ((win_a, 0), (win_b, half_width)):
        cmd = [
            "wmctrl",
            "-i",
            "-r",
            win_id,
            "-e",
            f"0,{x},{margin_top},{half_width},{usable_height}",
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


def _frame_crop_geometry(display: int, win_id: str) -> tuple[dict, str]:
    """Return frame extents and optional ImageMagick crop geometry.

    For a managed top-level window, many EWMH-compliant window managers
    (including openbox) expose _NET_FRAME_EXTENTS which describes the
    decorations around the client area. We use this to derive a crop
    rectangle that removes borders and title bars so that screenshots
    correspond more closely to the actual client framebuffer.

    Returns (extents, crop_geom) where:
      - extents is a dict with keys left,right,top,bottom (ints)
      - crop_geom is a convert-compatible geometry string or "" if
        extents could not be determined.
    """

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}

    extents: dict[str, int] = {"left": 0, "right": 0, "top": 0, "bottom": 0}
    crop_geom = ""

    try:
        # Query frame extents
        result = subprocess.run(
            ["xprop", "-id", win_id, "_NET_FRAME_EXTENTS"],
            env=env,
            capture_output=True,
            text=True,
            timeout=5.0,
        )
        if result.returncode != 0:
            return extents, crop_geom

        line = "".join(result.stdout.splitlines())
        if "_NET_FRAME_EXTENTS" not in line or "=" not in line:
            return extents, crop_geom

        _, values = line.split("=", 1)
        parts = [p.strip() for p in values.split(",")]
        if len(parts) != 4:
            return extents, crop_geom

        left, right, top, bottom = (int(p) for p in parts)
        extents.update({
            "left": left,
            "right": right,
            "top": top,
            "bottom": bottom,
        })

        # If there are no decorations, nothing to crop.
        if left == 0 and right == 0 and top == 0 and bottom == 0:
            return extents, crop_geom

        # Query overall frame size so we can derive inner size.
        result = subprocess.run(
            ["xwininfo", "-id", win_id],
            env=env,
            capture_output=True,
            text=True,
            timeout=5.0,
        )
        if result.returncode != 0:
            return extents, crop_geom

        width = height = None
        for line in result.stdout.splitlines():
            line = line.strip()
            if line.startswith("Width:"):
                try:
                    width = int(line.split()[1])
                except Exception:
                    pass
            elif line.startswith("Height:"):
                try:
                    height = int(line.split()[1])
                except Exception:
                    pass

        if width is None or height is None:
            return extents, crop_geom

        inner_w = max(1, width - left - right)
        inner_h = max(1, height - top - bottom)
        if inner_w <= 0 or inner_h <= 0:
            return extents, crop_geom

        crop_geom = f"{inner_w}x{inner_h}+{left}+{top}"
        return extents, crop_geom

    except Exception:
        # Best-effort only; fall back to uncropped screenshots on errors.
        return extents, crop_geom


def _capture_window(
    display: int,
    backend: str,
    win_id: str,
    outfile: Path,
) -> Path:
    """Capture a screenshot of a single viewer window."""

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}
    outfile = outfile.with_suffix(".png")
    outfile.parent.mkdir(parents=True, exist_ok=True)

    # Derive optional crop geometry that removes WM decorations so that the
    # resulting PNG represents only the client area.
    _, crop_geom = _frame_crop_geometry(display, win_id)

    if backend == "xwd+convert":
        if crop_geom:
            cmd = (
                f"xwd -silent -id {win_id} | "
                f"convert xwd:- -crop {crop_geom} +repage png:{outfile}"
            )
        else:
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
        # import has limited direct cropping support, so we capture first and
        # then optionally post-process with convert if available.
        cmd = ["import", "-window", win_id, str(outfile)]
        result = subprocess.run(
            cmd,
            env=env,
            capture_output=True,
            text=True,
            timeout=60.0,
        )
        if result.returncode == 0 and crop_geom:
            # Best-effort post-crop; ignore failures and keep original.
            try:
                subprocess.run(
                    [
                        "convert",
                        str(outfile),
                        "-crop",
                        crop_geom,
                        "+repage",
                        str(outfile),
                    ],
                    env=env,
                    capture_output=True,
                    text=True,
                    timeout=30.0,
                )
            except Exception:
                pass
    else:
        raise PreflightError(f"Unknown screenshot backend: {backend}")

    if result.returncode != 0:
        raise PreflightError(
            f"Screenshot capture failed for window {win_id} (backend={backend}): {result.stderr.strip()}"
        )

    return outfile


def _defocus_and_hide_cursor(display: int, helper_win: str) -> None:
    """Give focus to a tiny helper window and move the cursor inside it.

    This ensures both viewer windows remain unfocused and the local cursor
    stays in the helper window's area (above them) while screenshots are
    captured.
    """

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}

    # Activate the helper window so that it owns the keyboard focus.
    result = subprocess.run(
        ["wmctrl", "-i", "-a", helper_win],
        env=env,
        capture_output=True,
        text=True,
        timeout=10.0,
    )
    if result.returncode != 0:
        raise PreflightError(
            f"Failed to activate helper window {helper_win}: {result.stderr.strip()}"
        )

    # Warp the cursor inside the helper window. This keeps it away from the
    # viewer windows, which start below the reserved top margin.
    result = subprocess.run(
        ["xdotool", "mousemove", "--window", helper_win, "5", "5"],
        env=env,
        capture_output=True,
        text=True,
        timeout=10.0,
    )
    if result.returncode != 0:
        raise PreflightError(
            f"Failed to move cursor into helper window {helper_win}: {result.stderr.strip()}"
        )

    time.sleep(0.2)


def _warp_pointer_to_display(display: int, x: int, y: int, what: str) -> None:
    """Warp the mouse pointer on a given X display to a safe location.

    Used to park the pointer in a deterministic corner (typically the
    top-left) on both the content server display and the viewer display
    so that it does not introduce non-deterministic pixels in the areas
    compared by the screenshot tests.
    """

    import subprocess

    env = {**os.environ, "DISPLAY": f":{display}"}

    result = subprocess.run(
        ["xdotool", "mousemove", str(x), str(y)],
        env=env,
        capture_output=True,
        text=True,
        timeout=10.0,
    )
    if result.returncode != 0:
        raise PreflightError(
            f"Failed to warp pointer on display :{display} for {what}: {result.stderr.strip()}"
        )

    time.sleep(0.1)



def _log_checkpoint_event(artifacts: ArtifactManager, checkpoint: int, phase: str) -> None:
    """Log a timestamped marker for each checkpoint capture.

    This writes to a dedicated checkpoints.log file under the artifacts logs
    directory so that viewer/server logs can be correlated with the exact
    times at which screenshots were taken.
    """

    ts = time.time()
    log_path = artifacts.logs_dir / "checkpoints.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with open(log_path, "a", encoding="utf-8") as f:
        f.write(f"{ts:.6f} checkpoint={checkpoint} phase={phase}\n")


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
        "--scenario",
        default="cache",
        choices=["cache", "browser", "image_toggle"],
        help=(
            "Content scenario: cache (xterm cache_hits_minimal), "
            "browser (long article scroll), or image_toggle (two-picture toggle). "
            "Default: cache"
        ),
    )
    parser.add_argument(
        "--browser-url",
        default=None,
        help=(
            "Override URL for the browser scenario (default: https://www.bbc.com). "
            "Ignored for non-browser scenarios."
        ),
    )
    parser.add_argument(
        "--viewer-geometry",
        default="1600x1000",
        help="Logical geometry used to arrange viewer windows side-by-side (default: 1600x1000)",
    )
    parser.add_argument(
        "--viewer-display-geometry",
        default=None,
        help=(
            "Geometry for the X server that hosts the viewer windows. "
            "Defaults to --viewer-geometry but can be larger to allow window resizes."
        ),
    )
    parser.add_argument(
        "--viewer-resize-factor",
        type=float,
        default=1.0,
        help=(
            "Optional scale factor to grow viewer windows during the run "
            "(e.g. 1.2 to increase width/height by 20%%). Values <= 1.0 disable resizing."
        ),
    )
    parser.add_argument(
        "--viewer-resize-at-checkpoint",
        type=int,
        default=0,
        help=(
            "Checkpoint index after which to resize viewer windows using the "
            "given factor (0 means no timed resize)."
        ),
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
    print(f"Scenario: {args.scenario}")
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

    # Parse viewer geometry (WIDTHxHEIGHT) used for initial window layout.
    try:
        geom_parts = args.viewer_geometry.lower().split("x")
        viewer_width = int(geom_parts[0])
        viewer_height = int(geom_parts[1])
    except Exception as exc:  # pragma: no cover - defensive
        print(f"Invalid viewer geometry '{args.viewer_geometry}': {exc}")
        return 1

    # Parse viewer display geometry, which may be larger to allow resizes.
    if args.viewer_display_geometry is None:
        viewer_display_width = viewer_width
        viewer_display_height = viewer_height
    else:
        try:
            disp_parts = args.viewer_display_geometry.lower().split("x")
            viewer_display_width = int(disp_parts[0])
            viewer_display_height = int(disp_parts[1])
        except Exception as exc:  # pragma: no cover - defensive
            print(f"Invalid viewer display geometry '{args.viewer_display_geometry}': {exc}")
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

        # 4. Start viewer window server (can be larger than the initial layout)
        print(f"\n[4/8] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "bb_viewerwin",
            artifacts,
            tracker,
            geometry=f"{viewer_display_width}x{viewer_display_height}",
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
        _arrange_viewer_windows(
            args.display_viewer,
            win_ground,
            win_cache,
            viewer_width,
            viewer_height,
        )
        print("  Viewer windows arranged")

        # Create a tiny helper window above the viewers, give it focus and
        # move the cursor into it so that both viewer windows are unfocused
        # and cursor-free during screenshot capture.
        print("  Unfocusing viewer windows and hiding cursor...")
        helper_proc = server_viewer.run_in_display(
            ["xterm", "-geometry", "10x1+0+0", "-name", "bb_focus_sentinel"],
            name="bb_focus_sentinel",
        )
        helper_win = _find_window_id_for_pid(args.display_viewer, helper_proc.pid)
        _defocus_and_hide_cursor(args.display_viewer, helper_win)

        # Park the pointer on the content server display in the top-left
        # corner of the desktop so that any remote cursor rendered by the
        # viewers appears in a small, known region near (0,0). This region
        # is already covered by the existing ignore_rects mask.
        _warp_pointer_to_display(
            args.display_content,
            5,
            5,
            "content server pointer",
        )

        print("  Viewer focus and cursor normalised and pointers parked")

        # 6. Start scenario on content server in background
        print("\n[6/8] Starting scenario on content server...")

        # Synchronisation barrier used to pause scenario motion during capture
        pause_event = threading.Event()

        def _scenario_thread_body():
            try:
                if args.scenario == "browser":
                    runner = ScenarioRunner(args.display_content, verbose=args.verbose)
                    try:
                        runner.browser_scroll_bbc(
                            duration_sec=args.duration,
                            url=args.browser_url,
                            pause_event=pause_event,
                        )
                    finally:
                        runner.cleanup()
                elif args.scenario == "image_toggle":
                    static_runner = StaticScenarioRunner(
                        args.display_content,
                        verbose=args.verbose,
                    )
                    try:
                        # Approximate number of toggles from duration (one toggle
                        # roughly every 3 seconds including viewer work).
                        est_toggles = max(6, int(args.duration / 3))
                        static_runner.toggle_two_pictures_test(
                            toggles=est_toggles,
                            delay_between=2.0,
                        )
                    finally:
                        static_runner.cleanup()
                else:
                    runner = ScenarioRunner(args.display_content, verbose=args.verbose)
                    try:
                        runner.cache_hits_minimal(duration_sec=args.duration)
                    finally:
                        runner.cleanup()
            except Exception:
                # Any unexpected exception in the scenario thread should be
                # visible in the main thread logs via traceback there.
                raise

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
        resize_performed = False
        for i in range(1, checkpoints + 1):
            sleep_time = interval
            print(f"  Waiting {sleep_time:.1f}s before checkpoint {i}...")
            time.sleep(sleep_time)

            gt_path = artifacts.screenshots_dir / f"checkpoint_{i}_ground_truth.png"
            cache_path = artifacts.screenshots_dir / f"checkpoint_{i}_cache.png"

            # Pause scenario motion to capture a stable frame on both viewers
            if args.scenario == "browser":
                pause_event.set()
                # Allow a short settling time so any in-flight damage completes
                time.sleep(0.5)

            # As an extra safety net, re-park the pointers on both displays
            # immediately before each checkpoint capture. This ensures that any
            # stray motion since startup does not place a cursor inside the
            # comparison region.
            _warp_pointer_to_display(
                args.display_viewer,
                5,
                5,
                "viewer display pointer",
            )
            _warp_pointer_to_display(
                args.display_content,
                5,
                5,
                "content server pointer",
            )

            # Emit explicit markers so we can correlate screenshot capture
            # times with viewer/server logs when analysing failures.
            _log_checkpoint_event(artifacts, i, "ground_truth_before_capture")
            gt_path = _capture_window(
                args.display_viewer,
                screenshot_backend,
                win_ground,
                gt_path,
            )

            _log_checkpoint_event(artifacts, i, "cache_before_capture")
            cache_path = _capture_window(
                args.display_viewer,
                screenshot_backend,
                win_cache,
                cache_path,
            )

            if args.scenario == "browser":
                pause_event.clear()

            checkpoint_paths.append((gt_path, cache_path))
            print(f"  Captured checkpoint {i}")

            # Optionally resize viewer windows after a given checkpoint to
            # simulate a user enlarging the viewer (which in turn triggers a
            # resize of the remote desktop on the content server).
            if (
                not resize_performed
                and args.viewer_resize_factor > 1.0
                and args.viewer_resize_at_checkpoint == i
            ):
                new_total_width = min(
                    viewer_display_width,
                    int(viewer_width * args.viewer_resize_factor),
                )
                new_total_height = min(
                    viewer_display_height,
                    int(viewer_height * args.viewer_resize_factor),
                )
                print(
                    "  Resizing viewer windows layout to "
                    f"{new_total_width}x{new_total_height} (factor {args.viewer_resize_factor})..."
                )
                _arrange_viewer_windows(
                    args.display_viewer,
                    win_ground,
                    win_cache,
                    new_total_width,
                    new_total_height,
                )
                resize_performed = True
                # Give extra time for framebuffer updates to complete after resize
                time.sleep(2.0)

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

            # Determine actual image dimensions to build dynamic ignore regions
            # that remain correct after window resize.
            try:
                with Image.open(gt_path) as _img:
                    img_w, img_h = _img.size
            except Exception:
                # Fallback to original geometry if image cannot be opened here
                img_w, img_h = viewer_width, viewer_height

            # Mask unstable regions inside the viewer window where we know
            # legitimate differences can occur even for "no-cache vs
            # no-cache" runs:
            #  - Small top-left box (possible cursor)
            #  - Thin right-edge strip (WM border variations)
            ignore_rects = [
                (0, 0, 79, 79),
                (max(0, img_w - 4), 0, max(0, img_w - 1), max(0, img_h - 1)),
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
