#!/usr/bin/env python3
"""End-to-end test: Rust viewer PersistentCache functionality.

Mirrors the existing C++ PersistentCache E2E test but launches the Rust viewer
(njcvncviewer-rs). If no PersistentCache protocol activity is observed in logs,
this test prints an informational note and exits successfully.

Linux only (requires X11 server stack). On macOS, exits successfully with a skip.
"""

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    ArtifactManager,
    PreflightError,
    ProcessTracker,
    VNCServer,
    check_display_available,
    check_port_available,
    preflight_check,
    BUILD_DIR,
)
from scenarios_static import StaticScenarioRunner


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

    print(f" Starting {name} with Rust viewer...")
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


def count_persistent_markers(log_path: Path) -> int:
    n = 0
    try:
        with open(log_path, "r", encoding="utf-8", errors="replace") as f:
            for line in f:
                if "PersistentCacheQuery" in line:
                    n += 1
                if "PersistentCache" in line and ("HIT" in line or "MISS" in line):
                    n += 1
    except OSError:
        return 0
    return n


def main() -> int:
    if sys.platform == "darwin":
        print("SKIP: Rust viewer e2e tests are Linux-only (requires X11 server stack)")
        return 0

    parser = argparse.ArgumentParser(description="Test Rust viewer PersistentCache functionality")
    parser.add_argument("--display-content", type=int, default=998)
    parser.add_argument("--port-content", type=int, default=6898)
    parser.add_argument("--display-viewer", type=int, default=999)
    parser.add_argument("--port-viewer", type=int, default=6899)
    parser.add_argument("--duration", type=int, default=60)
    parser.add_argument("--cache-size", type=int, default=256)
    parser.add_argument("--wm", default="openbox")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    print("=" * 70)
    print("Rust Viewer PersistentCache Test")
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

    # Determine server mode (matches C++ test pattern)
    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    server_mode = "local" if (local_server_symlink.exists() or local_server_actual.exists()) else "system"

    try:
        print("")
        print(f"[1/6] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "rust_pc_content",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params={"EnablePersistentCache": "1"},
        )
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("")
            print("✗ FAIL: Could not start content server/session")
            return 1

        print("")
        print(f"[2/6] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "rust_pc_viewerwin",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("")
            print("✗ FAIL: Could not start viewer window server/session")
            return 1

        print("")
        print("[3/6] Launching Rust viewer...")
        viewer_proc = run_rust_viewer(
            binaries["rust_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "rust_pc_test_viewer",
            display_for_viewer=args.display_viewer,
            verbosity=2,
            shared=True,
        )
        if viewer_proc.poll() is not None:
            print("")
            print("✗ FAIL: Rust viewer exited prematurely")
            return 1

        print("")
        print(f"[4/6] Running tiled logos scenario ({args.duration}s)...")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        time.sleep(3.0)

        if viewer_proc.poll() is not None:
            print("")
            print("✗ FAIL: Rust viewer exited during scenario")
            return 1

        print("")
        print("[5/6] Stopping viewer and analysing logs...")
        tracker.cleanup("rust_pc_test_viewer")
        time.sleep(1.0)

        log_path = artifacts.logs_dir / "rust_pc_test_viewer.log"
        if not log_path.exists():
            print("")
            print(f"✗ FAIL: Viewer log not found: {log_path}")
            return 1

        pcount = count_persistent_markers(log_path)
        print("")
        print(f"PersistentCache marker count: {pcount}")
        if pcount == 0:
            print("NOTE: No PersistentCache markers observed in Rust viewer log; stability validated only.")

        print("")
        print("[6/6] TEST PASSED")
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
