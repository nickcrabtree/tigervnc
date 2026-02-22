#!/usr/bin/env python3
"""End-to-end test: Rust viewer cache protocol (ContentCache scenario).

This mirrors the structure of the existing C++ ContentCache E2E test, but
launches the Rust viewer (njcvncviewer-rs).

On some setups the Rust viewer may not yet negotiate or emit cache protocol
messages; in that case this test validates stability (no early exit) and prints
an informational note.

Runs on Linux only (requires Xtigervnc/openbox/wmctrl/xdotool). On macOS, exits
successfully with a skip message.
"""

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    ArtifactManager,
    PreflightError,
    ProcessTracker,
    VNCServer,
    check_display_available,
    check_port_available,
    preflight_check,
)
from scenarios import ScenarioRunner


def run_rust_viewer(
    viewer_path: str,
    port: int,
    artifacts: ArtifactManager,
    tracker: ProcessTracker,
    name: str,
    display_for_viewer: int | None = None,
    verbosity: int = 2,
    shared: bool = True,
) -> subprocess.Popen:
    """Run Rust viewer against the content server."""

    cmd: list[str] = [viewer_path]
    if shared:
        cmd.append("--shared")
    cmd.extend(["-v"] * max(0, int(verbosity)))
    cmd.append(f"127.0.0.1::{port}")

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()
    if display_for_viewer is not None:
        env["DISPLAY"] = f":{display_for_viewer}"
    else:
        env.pop("DISPLAY", None)

    print(f" Starting {name} with Rust viewer (verbosity={verbosity})...")
    log_file = open(log_path, "w")
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


def count_cache_markers(log_path: Path) -> dict[str, int]:
    """Best-effort scan for cache-related markers in Rust viewer logs."""
    markers = {
        "PersistentCacheQuery": 0,
        "RequestCachedData": 0,
        "PersistentCache HIT": 0,
        "PersistentCache MISS": 0,
    }
    try:
        with open(log_path, "r", encoding="utf-8", errors="replace") as f:
            for line in f:
                if "PersistentCacheQuery" in line:
                    markers["PersistentCacheQuery"] += 1
                if "RequestCachedData" in line:
                    markers["RequestCachedData"] += 1
                if "PersistentCache" in line and "HIT" in line:
                    markers["PersistentCache HIT"] += 1
                if "PersistentCache" in line and "MISS" in line:
                    markers["PersistentCache MISS"] += 1
    except OSError:
        pass
    return markers


def main() -> int:
    if sys.platform == "darwin":
        print("SKIP: Rust viewer e2e tests are Linux-only (requires X11 server stack)")
        return 0

    parser = argparse.ArgumentParser(description="Test Rust viewer cache protocol (ContentCache scenario)")
    parser.add_argument("--display-content", type=int, default=998)
    parser.add_argument("--port-content", type=int, default=6898)
    parser.add_argument("--display-viewer", type=int, default=999)
    parser.add_argument("--port-viewer", type=int, default=6899)
    parser.add_argument("--duration", type=int, default=60)
    parser.add_argument("--wm", default="openbox")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--min-cache-markers", type=int, default=0, help="Minimum number of cache-related markers to require (default: 0 = no enforcement)")
    args = parser.parse_args()

    print("=" * 70)
    print("Rust Viewer ContentCache-Scenario Test")
    print("=" * 70)

    artifacts = ArtifactManager()
    artifacts.create()

    try:
        binaries = preflight_check(verbose=args.verbose)
    except PreflightError as e:
        print("")
        print("✗ FAIL: Preflight checks failed")
        print("")
        print(f"{e}")
        return 1

    if not check_port_available(args.port_content):
        print("")
        print(f"✗ FAIL: Port {args.port_content} already in use")
        return 1
    if not check_port_available(args.port_viewer):
        print("")
        print(f"✗ FAIL: Port {args.port_viewer} already in use")
        return 1
    if not check_display_available(args.display_content):
        print("")
        print(f"✗ FAIL: Display :{args.display_content} already in use")
        return 1
    if not check_display_available(args.display_viewer):
        print("")
        print(f"✗ FAIL: Display :{args.display_viewer} already in use")
        return 1

    tracker = ProcessTracker()

    try:
        # Content server
        print("")
        print(f"[1/6] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "rust_cc_content",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice="auto",
        )
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("")
            print("✗ FAIL: Could not start content server/session")
            return 1

        # Viewer window server
        print("")
        print(f"[2/6] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "rust_cc_viewerwin",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice="auto",
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("")
            print("✗ FAIL: Could not start viewer window server/session")
            return 1

        # Launch Rust viewer
        print("")
        print("[3/6] Launching Rust viewer...")
        viewer_proc = run_rust_viewer(
            binaries["rust_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "rust_cc_test_viewer",
            display_for_viewer=args.display_viewer,
            verbosity=2,
            shared=True,
        )
        if viewer_proc.poll() is not None:
            print("")
            print("✗ FAIL: Rust viewer exited prematurely")
            return 1

        # Scenario
        print("")
        print(f"[4/6] Running repeated-content scenario ({args.duration}s)...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        runner.cache_hits_minimal(duration_sec=args.duration)
        runner.cache_hits_minimal(duration_sec=max(20, int(args.duration * 0.33)))
        time.sleep(3.0)

        if viewer_proc.poll() is not None:
            print("")
            print("✗ FAIL: Rust viewer exited during scenario")
            return 1

        # Stop viewer and analyse
        print("")
        print("[5/6] Stopping viewer and analysing logs...")
        tracker.cleanup("rust_cc_test_viewer")
        time.sleep(1.0)
        log_path = artifacts.logs_dir / "rust_cc_test_viewer.log"
        if not log_path.exists():
            print("")
            print(f"✗ FAIL: Viewer log not found: {log_path}")
            return 1

        markers = count_cache_markers(log_path)
        total_markers = sum(markers.values())
        print("")
        print("Cache marker summary:")
        for k, v in markers.items():
            print(f"  {k}: {v}")

        if args.min_cache_markers > 0 and total_markers < args.min_cache_markers:
            print("")
            print(f"✗ FAIL: Expected at least {args.min_cache_markers} cache markers, saw {total_markers}")
            return 1

        print("")
        print("[6/6] TEST PASSED")
        if total_markers == 0:
            print("NOTE: No cache markers observed in Rust viewer log; stability validated only.")
        print(f"Artifacts: {artifacts.base_dir}")
        return 0

    except KeyboardInterrupt:
        print("")
        print("Interrupted")
        return 130
    except Exception as e:
        print("")
        print(f"✗ FAIL: Unexpected error: {e}")
        import traceback

        traceback.print_exc()
        return 1
    finally:
        tracker.cleanup_all()


if __name__ == "__main__":
    raise SystemExit(main())
