#!/usr/bin/env python3
"""
End-to-end test: C++ viewer with both ContentCache and PersistentCache disabled.

Validates that when all cache options are disabled on both server and client,
no cache protocol activity is observed (no CachedRect/CachedRectInit, no
PersistentCache hits/misses/evictions).
"""

import sys
import time
import argparse
import os
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only,
    PreflightError,
    ArtifactManager,
    ProcessTracker,
    VNCServer,
    check_port_available,
    check_display_available,
    BUILD_DIR,
)
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, compute_metrics


def run_cpp_viewer_no_caches(viewer_path, port, artifacts, tracker, name,
                             display_for_viewer=None):
    """Run C++ viewer with both ContentCache and PersistentCache disabled."""
    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        "Shared=1",
        "Log=*:stderr:100",
        "ContentCache=0",
        "PersistentCache=0",
    ]

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()

    if display_for_viewer is not None:
        env["DISPLAY"] = f":{display_for_viewer}"
    else:
        env.pop("DISPLAY", None)

    print(f"  Starting {name} with ContentCache=0, PersistentCache=0 ...")
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

    return proc, log_path


def main():
    parser = argparse.ArgumentParser(
        description="Test C++ viewer with all caches disabled",
    )
    parser.add_argument("--display-content", type=int, default=998,
                        help="Display number for content server (default: 998)")
    parser.add_argument("--port-content", type=int, default=6898,
                        help="Port for content server (default: 6898)")
    parser.add_argument("--display-viewer", type=int, default=999,
                        help="Display number for viewer window (default: 999)")
    parser.add_argument("--port-viewer", type=int, default=6899,
                        help="Port for viewer window server (default: 6899)")
    parser.add_argument("--duration", type=int, default=30,
                        help="Scenario duration in seconds (default: 30)")
    parser.add_argument("--wm", default="openbox",
                        help="Window manager (default: openbox)")
    parser.add_argument("--verbose", action="store_true",
                        help="Verbose output")

    args = parser.parse_args()

    print("=" * 70)
    print("C++ Viewer No-Caches Test")
    print("=" * 70)
    print(f"\nDuration: {args.duration}s")
    print()

    # 1. Create artifacts
    print("[1/8] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight checks (C++ viewer only)
    print("\n[2/8] Running preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
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

    # Determine server mode
    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = "local"
        print("\nUsing local Xnjcvnc server")
    else:
        server_mode = "system"
        print("\nUsing system Xtigervnc server")

    try:
        # 3. Start content server with both caches disabled
        print(f"\n[3/8] Starting content server (:{args.display_content}) with all caches disabled...")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "cpp_no_cache_content",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
            server_params={
                "EnableContentCache": "0",
                "EnablePersistentCache": "0",
            },
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
            "cpp_no_cache_viewerwin",
            artifacts,
            tracker,
            geometry="1920x1080",
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

        # 5. Launch viewer with all caches disabled
        print("\n[5/8] Launching C++ viewer with ContentCache=0 and PersistentCache=0...")
        viewer_proc, log_path = run_cpp_viewer_no_caches(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "cpp_no_cache_test_viewer",
            display_for_viewer=args.display_viewer,
        )
        if viewer_proc.poll() is not None:
            print("\n✗ FAIL: Viewer exited prematurely")
            return 1
        print("✓ Viewer connected")

        # 6. Run a simple tiled-logos scenario to exercise drawing paths.
        print(f"\n[6/8] Running tiled-logos scenario (no caches expected)...")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        stats = runner.tiled_logos_test(tiles=6, duration=args.duration, delay_between=2.0)
        print(f"  Scenario completed: {stats}")
        time.sleep(2.0)

        # 7. Stop viewer and analyze
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup("cpp_no_cache_test_viewer")

        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        cc = metrics["cache_operations"]
        proto = metrics["protocol_messages"]
        pers = metrics["persistent"]

        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        print("\nContentCache metrics:")
        print(f"  Lookups: {cc['total_lookups']}")
        print(f"  Hits:    {cc['total_hits']}")
        print(f"  Misses:  {cc['total_misses']}")

        print("\nContentCache protocol messages:")
        print(f"  CachedRect:     {proto['CachedRect']}")
        print(f"  CachedRectInit: {proto['CachedRectInit']}")
        print(f"  RequestCachedData: {proto['RequestCachedData']}")

        print("\nPersistentCache metrics:")
        print(f"  Hits:           {pers['hits']}")
        print(f"  Misses:         {pers['misses']}")
        print(f"  Evictions:      {pers['eviction_count']}")
        print(f"  Init events:    {pers['init_events']}")

        # Validation: all cache-related metrics must remain zero.
        success = True
        failures = []

        if cc['total_lookups'] != 0 or cc['total_hits'] != 0 or cc['total_misses'] != 0:
            success = False
            failures.append(
                f"ContentCache counters non-zero (lookups={cc['total_lookups']}, hits={cc['total_hits']}, misses={cc['total_misses']})",
            )

        if (
            proto['CachedRect'] != 0
            or proto['CachedRectInit'] != 0
            or proto['RequestCachedData'] != 0
        ):
            success = False
            failures.append(
                f"ContentCache protocol messages observed (CachedRect={proto['CachedRect']}, "
                f"CachedRectInit={proto['CachedRectInit']}, RequestCachedData={proto['RequestCachedData']})",
            )

        if (
            pers['hits'] != 0
            or pers['misses'] != 0
            or pers['eviction_count'] != 0
            or pers['init_events'] != 0
        ):
            success = False
            failures.append(
                f"PersistentCache activity observed (hits={pers['hits']}, misses={pers['misses']}, "
                f"evictions={pers['eviction_count']}, init_events={pers['init_events']})",
            )

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")

        print("\n" + "=" * 70)
        if success:
            print("✓ TEST PASSED")
            print("=" * 70)
            return 0
        else:
            print("✗ TEST FAILED")
            print("=" * 70)
            for f in failures:
                print(f"  • {f}")
            return 1

    except KeyboardInterrupt:
        print("\nInterrupted by user")
        return 130
    except Exception as e:
        print(f"\n✗ FAIL: Unexpected error: {e}")
        import traceback
        traceback.print_exc()
        return 1
    finally:
        print("\nCleaning up...")
        tracker.cleanup_all()
        print("✓ Cleanup complete")


if __name__ == "__main__":
    import subprocess
    sys.exit(main())
