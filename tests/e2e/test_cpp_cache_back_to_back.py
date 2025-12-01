#!/usr/bin/env python3
"""
Back-to-back comparison of C++ ContentCache vs PersistentCache.

Phase 1 (ContentCache cold):
  - Server: ContentCache enabled (default), PersistentCache disabled.
  - Viewer: ContentCache enabled, PersistentCache=0.
  - Scenario: tiled logos -> establish baseline hit count for ContentCache.

Phase 2 (PersistentCache cold):
  - Server: ContentCache disabled, PersistentCache enabled.
  - Viewer: ContentCache=0, PersistentCache=1, PersistentCachePath set to
    an artifacts-local file to ensure a cold disk cache.
  - Scenario: same tiled logos -> PersistentCache should see at least as
    many hits as ContentCache in the cold case.

Phase 3 (PersistentCache warm):
  - Server: same as Phase 2.
  - Viewer: same PersistentCachePath as Phase 2 (warm disk cache).
  - Scenario: same tiled logos -> hit count should be >= cold PersistentCache.

Assertions:
  - Both ContentCache and PersistentCache (cold) must see non-zero hits.
  - PersistentCache (cold) must have hits >= ContentCache hits.
  - PersistentCache (warm) must have hits >= PersistentCache (cold) hits.
"""

import sys
import time
import argparse
import os
import subprocess
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT,
)
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_cpp_viewer(viewer_path, port, artifacts, tracker, name,
                   extra_params, display_for_viewer=None):
    """Run C++ viewer with given extra parameters (list of key=value strings)."""
    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        "Shared=1",
        "Log=*:stderr:100",
    ] + extra_params

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()

    if display_for_viewer is not None:
        env["DISPLAY"] = f":{display_for_viewer}"
    else:
        env.pop("DISPLAY", None)

    print(f"  Starting {name} with params: {' '.join(extra_params)}")
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
        description="Back-to-back C++ ContentCache vs PersistentCache test",
    )
    parser.add_argument("--display-content", type=int, default=998,
                        help="Display number for content server (default: 998)")
    parser.add_argument("--port-content", type=int, default=6898,
                        help="Port for content server (default: 6898)")
    parser.add_argument("--display-viewer", type=int, default=999,
                        help="Display number for viewer window (default: 999)")
    parser.add_argument("--port-viewer", type=int, default=6899,
                        help="Port for viewer window server (default: 6899)")
    parser.add_argument("--duration", type=int, default=60,
                        help="Scenario duration in seconds (default: 60)")
    parser.add_argument("--cache-size", type=int, default=256,
                        help="Cache size in MB for both caches (default: 256MB)")
    parser.add_argument("--wm", default="openbox",
                        help="Window manager (default: openbox)")
    parser.add_argument("--verbose", action="store_true",
                        help="Verbose output")

    args = parser.parse_args()

    print("=" * 70)
    print("C++ ContentCache vs PersistentCache Back-to-Back Test")
    print("=" * 70)
    print(f"\nCache Size: {args.cache_size}MB")
    print(f"Duration per phase: {args.duration}s")
    print()

    # 1. Create artifacts
    print("[1/9] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight checks
    print("\n[2/9] Running preflight checks (C++ only)...")
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
    local_server_symlink = PROJECT_ROOT / "build" / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = PROJECT_ROOT / "build" / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"

    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = "local"
        print("\nUsing local Xnjcvnc server")
    else:
        server_mode = "system"
        print("\nUsing system Xtigervnc server")

    # Prepare a per-test PersistentCache directory under artifacts
    # v3 uses a directory with index.dat + shard files instead of a single file
    pcache_path = artifacts.logs_dir / "back_to_back_persistentcache"

    try:
        # ==============================
        # Phase 1: ContentCache (cold)
        # ==============================
        print(f"\n[3/9] Phase 1: ContentCache cold run (server :{args.display_content})...")
        print("  Server config: EnablePersistentCache=0 (ContentCache only)")

        server_content_cc = VNCServer(
            args.display_content,
            args.port_content,
            "b2b_cc_content",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params={"EnablePersistentCache": "0"},
        )

        if not server_content_cc.start():
            print("\n✗ FAIL: Could not start ContentCache server")
            return 1
        if not server_content_cc.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start ContentCache server session")
            return 1
        print("✓ Content server ready (ContentCache phase)")

        # Viewer window server
        print(f"\n[4/9] Starting viewer window server for ContentCache phase (:{args.display_viewer})...")
        server_viewer_cc = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "b2b_cc_viewerwin",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer_cc.start():
            print("\n✗ FAIL: Could not start viewer window server (ContentCache phase)")
            return 1
        if not server_viewer_cc.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window session (ContentCache phase)")
            return 1
        print("✓ Viewer window server ready (ContentCache phase)")

        # Launch ContentCache viewer
        print("\n[5/9] Running tiled-logo scenario with ContentCache only...")
        cc_params = [
            f"ContentCacheSize={args.cache_size}",
            "PersistentCache=0",
        ]
        cc_proc, cc_log_path = run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "b2b_cc_viewer",
            extra_params=cc_params,
            display_for_viewer=args.display_viewer,
        )
        if cc_proc.poll() is not None:
            print("\n✗ FAIL: ContentCache viewer exited prematurely")
            return 1

        runner_cc = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        cc_stats = runner_cc.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  ContentCache scenario completed: {cc_stats}")
        time.sleep(2.0)

        tracker.cleanup("b2b_cc_viewer")
        time.sleep(1.0)

        if not cc_log_path.exists():
            print(f"\n✗ FAIL: ContentCache viewer log not found: {cc_log_path}")
            return 1

        cc_parsed = parse_cpp_log(cc_log_path)
        cc_metrics = compute_metrics(cc_parsed)
        cc_hits = cc_metrics["cache_operations"]["total_hits"]
        cc_lookups = cc_metrics["cache_operations"]["total_lookups"]
        cc_hit_rate = cc_metrics["cache_operations"]["hit_rate"]

        print("\nContentCache phase summary:")
        print(f"  Lookups: {cc_lookups}")
        print(f"  Hits:    {cc_hits} ({cc_hit_rate:.1f}%)")

        if cc_hits == 0:
            print("\n✗ FAIL: ContentCache phase produced zero hits; scenario is not valid for comparison")
            return 1

        # Cleanup ContentCache servers before PC phase
        server_viewer_cc.stop()
        server_content_cc.stop()
        time.sleep(2.0)

        # ======================================
        # Phase 2: PersistentCache (cold cache)
        # ======================================
        print(f"\n[6/9] Phase 2: PersistentCache cold run (server :{args.display_content})...")
        print("  Server config: EnableContentCache=0, EnablePersistentCache=1")

        # Ensure cold disk cache by removing any prior directory
        import shutil
        if pcache_path.exists():
            try:
                shutil.rmtree(pcache_path)
            except Exception:
                pass

        server_content_pc = VNCServer(
            args.display_content,
            args.port_content,
            "b2b_pc_content_cold",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            # ContentCache has been unified into the PersistentCache engine;
            # only EnablePersistentCache remains as a server-side toggle.
            server_params={
                "EnablePersistentCache": "1",
            },
        )

        if not server_content_pc.start():
            print("\n✗ FAIL: Could not start PersistentCache server (cold phase)")
            return 1
        if not server_content_pc.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start PersistentCache server session (cold phase)")
            return 1
        print("✓ Content server ready (PersistentCache cold phase)")

        # Viewer window server
        server_viewer_pc = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "b2b_pc_viewerwin_cold",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer_pc.start():
            print("\n✗ FAIL: Could not start viewer window server (PersistentCache cold phase)")
            return 1
        if not server_viewer_pc.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window session (PersistentCache cold phase)")
            return 1
        print("✓ Viewer window server ready (PersistentCache cold phase)")

        pc_cold_params = [
            # Force a lossless encoding so that PersistentCache entries are
            # eligible for on-disk persistence. Using ZRLE here ensures that
            # the cold run populates the disk cache and the warm run can
            # achieve a strictly better hit profile.
            "PreferredEncoding=ZRLE",
            "ContentCache=0",
            "PersistentCache=1",
            f"PersistentCacheSize={args.cache_size}",
            f"PersistentCachePath={pcache_path}",
        ]
        pc_cold_proc, pc_cold_log_path = run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "b2b_pc_viewer_cold",
            extra_params=pc_cold_params,
            display_for_viewer=args.display_viewer,
        )
        if pc_cold_proc.poll() is not None:
            print("\n✗ FAIL: PersistentCache cold viewer exited prematurely")
            return 1

        runner_pc_cold = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        pc_cold_stats = runner_pc_cold.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  PersistentCache cold scenario completed: {pc_cold_stats}")
        time.sleep(2.0)

        tracker.cleanup("b2b_pc_viewer_cold")
        time.sleep(1.0)

        if not pc_cold_log_path.exists():
            print(f"\n✗ FAIL: PersistentCache cold viewer log not found: {pc_cold_log_path}")
            return 1

        pc_cold_server_log = artifacts.logs_dir / f"b2b_pc_content_cold_server_{args.display_content}.log"
        # Fallback to default VNCServer naming if needed
        if not pc_cold_server_log.exists():
            pc_cold_server_log = artifacts.logs_dir / f"b2b_pc_content_cold_server_{args.display_content}.log"

        print("  Parsing PersistentCache cold viewer log...")
        pc_cold_viewer_parsed = parse_cpp_log(pc_cold_log_path)
        print("  Parsing PersistentCache cold server log...")
        pc_cold_server_parsed = parse_server_log(pc_cold_server_log, verbose=args.verbose)

        pc_cold_hits = pc_cold_viewer_parsed.persistent_hits + pc_cold_server_parsed.persistent_hits
        pc_cold_misses = pc_cold_viewer_parsed.persistent_misses + pc_cold_server_parsed.persistent_misses
        pc_cold_lookups = pc_cold_hits + pc_cold_misses

        print("\nPersistentCache cold phase summary:")
        print(f"  Lookups: {pc_cold_lookups}")
        print(f"  Hits:    {pc_cold_hits}")
        print(f"  Misses:  {pc_cold_misses}")

        if pc_cold_hits == 0:
            print("\n✗ FAIL: PersistentCache cold phase produced zero hits; scenario is not valid for comparison")
            return 1

        if pc_cold_hits < cc_hits:
            print("\n✗ FAIL: PersistentCache cold hits are less than ContentCache hits")
            print(f"  ContentCache hits:       {cc_hits}")
            print(f"  PersistentCache (cold): {pc_cold_hits}")
            return 1

        # Cleanup cold PC servers before warm phase
        server_viewer_pc.stop()
        server_content_pc.stop()
        time.sleep(2.0)

        # ======================================
        # Phase 3: PersistentCache (warm cache)
        # ======================================
        print(f"\n[7/9] Phase 3: PersistentCache warm run (server :{args.display_content})...")
        print("  Reusing same PersistentCachePath to exercise warm disk cache.")

        server_content_pc_warm = VNCServer(
            args.display_content,
            args.port_content,
            "b2b_pc_content_warm",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params={
                "EnablePersistentCache": "1",
            },
        )

        if not server_content_pc_warm.start():
            print("\n✗ FAIL: Could not start PersistentCache server (warm phase)")
            return 1
        if not server_content_pc_warm.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start PersistentCache server session (warm phase)")
            return 1
        print("✓ Content server ready (PersistentCache warm phase)")

        server_viewer_pc_warm = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "b2b_pc_viewerwin_warm",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer_pc_warm.start():
            print("\n✗ FAIL: Could not start viewer window server (PersistentCache warm phase)")
            return 1
        if not server_viewer_pc_warm.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window session (PersistentCache warm phase)")
            return 1
        print("✓ Viewer window server ready (PersistentCache warm phase)")

        pc_warm_params = pc_cold_params[:]  # same PC settings, same path (ZRLE, lossless)
        pc_warm_proc, pc_warm_log_path = run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "b2b_pc_viewer_warm",
            extra_params=pc_warm_params,
            display_for_viewer=args.display_viewer,
        )
        if pc_warm_proc.poll() is not None:
            print("\n✗ FAIL: PersistentCache warm viewer exited prematurely")
            return 1

        runner_pc_warm = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        pc_warm_stats = runner_pc_warm.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  PersistentCache warm scenario completed: {pc_warm_stats}")
        time.sleep(2.0)

        tracker.cleanup("b2b_pc_viewer_warm")
        time.sleep(1.0)

        pc_warm_server_log = artifacts.logs_dir / f"b2b_pc_content_warm_server_{args.display_content}.log"
        if not pc_warm_server_log.exists():
            pc_warm_server_log = artifacts.logs_dir / f"b2b_pc_content_warm_server_{args.display_content}.log"

        print("  Parsing PersistentCache warm viewer log...")
        pc_warm_viewer_parsed = parse_cpp_log(pc_warm_log_path)
        print("  Parsing PersistentCache warm server log...")
        pc_warm_server_parsed = parse_server_log(pc_warm_server_log, verbose=args.verbose)

        pc_warm_hits = pc_warm_viewer_parsed.persistent_hits + pc_warm_server_parsed.persistent_hits
        pc_warm_misses = pc_warm_viewer_parsed.persistent_misses + pc_warm_server_parsed.persistent_misses
        pc_warm_lookups = pc_warm_hits + pc_warm_misses

        print("\nPersistentCache warm phase summary:")
        print(f"  Lookups: {pc_warm_lookups}")
        print(f"  Hits:    {pc_warm_hits}")
        print(f"  Misses:  {pc_warm_misses}")

        # TDD expectation: a warm PersistentCache must provide strictly
        # better hit behaviour than a cold cache when driven with the
        # same pixel data. If the warm phase has the same or fewer hits
        # than the cold phase, treat this as a regression.
        if pc_warm_hits <= pc_cold_hits:
            print("\n✗ FAIL: PersistentCache warm hits are not greater than cold hits")
            print(f"  PersistentCache (cold): {pc_cold_hits}")
            print(f"  PersistentCache (warm): {pc_warm_hits}")
            return 1

        # Final cleanup of PC servers
        server_viewer_pc_warm.stop()
        server_content_pc_warm.stop()

        print("\n[8/9] Back-to-back comparison summary:")
        print(f"  ContentCache hits:             {cc_hits}")
        print(f"  PersistentCache cold hits:     {pc_cold_hits}")
        print(f"  PersistentCache warm hits:     {pc_warm_hits}")

        print("\n[9/9] RESULT: ✓ TEST PASSED")
        return 0

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
    sys.exit(main())
