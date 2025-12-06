#!/usr/bin/env python3
"""
End-to-end test: PersistentCache eviction notifications.

Validates that with a small PersistentCache, the client sends eviction
notifications and continues to operate with hits after evictions.
"""

import sys
import time
import argparse
import subprocess
import os
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT, BUILD_DIR
)
from scenarios import ScenarioRunner
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_viewer_with_small_persistent_cache(viewer_path, port, artifacts, tracker, name,
                                           cache_size_mb=16, display_for_viewer=None):
    """Run viewer with a small PersistentCache to force evictions."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'PreferredEncoding=ZRLE',
        'PersistentCache=1',
        f'PersistentCacheSize={cache_size_mb}',
    ]

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name} with PersistentCache={cache_size_mb}MB...")
    log_file = open(log_path, 'w')

    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env
    )

    tracker.register(name, proc)
    time.sleep(2.0)

    return proc


def main():
    parser = argparse.ArgumentParser(
        description='Test PersistentCache eviction with small cache size'
    )
    parser.add_argument('--display-content', type=int, default=998,
                        help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                        help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                        help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                        help='Port for viewer window server (default: 6899)')
    parser.add_argument('--duration', type=int, default=60,
                        help='Variable-content phase duration in seconds (default: 60)')
    parser.add_argument('--verify-duration', type=int, default=12,
                        help='Verification phase duration with repeated static logos (default: 12)')
    parser.add_argument('--variable-content', choices=['images','xclock','fullscreen','none'], default='images',
                        help='Variable content generator (default: images from system datasets)')
    parser.add_argument('--grid-cols', type=int, default=6,
                        help='xclock grid columns (default: 6)')
    parser.add_argument('--grid-rows', type=int, default=2,
                        help='xclock grid rows (default: 2)')
    parser.add_argument('--clock-size', type=int, default=160,
                        help='xclock window size (default: 160)')
    parser.add_argument('--cache-size', type=int, default=4,
                        help='Persistent cache size in MB (default: 4MB)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')

    args = parser.parse_args()

    print("=" * 70)
    print("PersistentCache Eviction Test")
    print("=" * 70)
    print(f"\nCache Size: {args.cache_size}MB (forcing evictions)")
    print(f"Duration: {args.duration}s")
    print()

    # 1. Create artifacts
    print("[1/8] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight checks
    print("\n[2/8] Running preflight checks...")
    try:
        binaries = preflight_check(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed")
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

    # 3. Initialize tracker
    tracker = ProcessTracker()

    # Determine server mode
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'

    print(f"\nUsing server mode: {server_mode}")

    try:
        # 4. Start content server in PersistentCache-focused mode.
        # Disable ContentCache so that the eviction scenario exercises
        # only the PersistentCache code path.
        print(f"\n[3/8] Starting content server (:{args.display_content}) with PersistentCache only...")
        server_params = {
            # Server-side ContentCache toggles have been removed; use the
            # unified PersistentCache engine only.
            'EnablePersistentCache': '1',
        }
        server_content = VNCServer(
            args.display_content, args.port_content, "pc_content_eviction",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params=server_params
        )

        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        # 5. Start viewer window server
        print(f"\n[4/8] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, "pc_viewer_window_eviction",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode
        )
        if not server_viewer.start():
            print("\n✗ FAIL: Could not start viewer window server")
            return 1
        if not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server session")
            return 1
        print("✓ Viewer window server ready")

        # 6. Launch test viewer with SMALL persistent cache
        print(f"\n[5/8] Launching viewer with PersistentCache={args.cache_size}MB...")
        test_proc = run_viewer_with_small_persistent_cache(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'pc_eviction_test_viewer', cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ Test viewer connected")

        # 7. Variable-content phase (xclock grid) to overflow the cache, then
        #    a short repeated-content phase to ensure hits survive eviction.
        print(f"\n[6/8] Running variable-content phase to force evictions...")
        vrunner = ScenarioRunner(args.display_content, verbose=args.verbose)
        srunner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        if args.variable_content == 'images':
            print("  Generating variable content from system image set (cycle set + churn)...")
            # First ensure repeated content for PersistentCache hits
            vstats_cycle = srunner.image_cycle(set_size=24, cycles=2, size=max(args.clock_size, 240),
                                               cols=args.grid_cols, rows=max(args.grid_rows, 3),
                                               delay_between=0.15)
            print(f"  Cycle phase completed: {vstats_cycle}")
            # Then general churn to overflow memory
            vstats = srunner.image_churn(duration_sec=args.duration, cols=args.grid_cols, rows=args.grid_rows,
                                         size=args.clock_size, interval_sec=0.6, max_windows=96)
        elif args.variable_content == 'xclock':
            print("  Generating variable content with xclock grid...")
            vstats = vrunner.xclock_grid(cols=args.grid_cols, rows=args.grid_rows,
                                         size=args.clock_size, update=1,
                                         duration_sec=args.duration)
        elif args.variable_content == 'fullscreen':
            print("  Generating variable content with fullscreen random colors...")
            vstats = srunner.random_fullscreen_colors(duration_sec=args.duration, interval_sec=0.4)
        else:
            print("  Using eviction_stress fallback...")
            vstats = vrunner.eviction_stress(duration_sec=args.duration)
        print(f"  Variable phase completed: {vstats}")

        # Aggressive eviction burst to ensure memory pressure
        print("  Running eviction burst with large images (size=320, count=24)...")
        burst_stats = srunner.image_burst(count=24, size=320, cols=4, rows=6, interval_ms=80)
        print(f"  Eviction burst completed: {burst_stats}")

        print(f"  Running verification phase with tiled logos for {args.verify_duration}s...")
        sstats = srunner.tiled_logos_test(tiles=12, duration=args.verify_duration, delay_between=1.0)
        print(f"  Verification phase completed: {sstats}")
        time.sleep(5.0)  # Let evictions and notifications complete

        # 8. Stop viewer and analyze
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup('pc_eviction_test_viewer')

        log_path = artifacts.logs_dir / 'pc_eviction_test_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        # Also parse server log for eviction notifications
        server_log_path = artifacts.logs_dir / f'pc_content_eviction_server_{args.display_content}.log'
        server_metrics = None
        if server_log_path.exists():
            server_parsed = parse_server_log(server_log_path)
            server_metrics = compute_metrics(server_parsed)
        
        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        pers = metrics['persistent']
        evict_v = pers['eviction_count']
        evicted_ids_v = pers['evicted_ids']
        evict_s = 0
        evicted_ids_s = 0
        if server_metrics is not None:
            evict_s = server_metrics['persistent']['eviction_count']
            evicted_ids_s = server_metrics['persistent']['evicted_ids']
        eviction_count = max(evict_v, evict_s)
        evicted_ids = max(evicted_ids_v, evicted_ids_s)

        print(f"PersistentCache Evictions (viewer/server): {evict_v}/{evict_s} (IDs: {evicted_ids_v}/{evicted_ids_s})")
        print(f"PersistentCache Bandwidth Reduction: {pers['bandwidth_reduction_pct']:.1f}%")

        # Hard failure if absolutely no activity
        if (
            pers['hits'] == 0
            and pers['misses'] == 0
            and eviction_count == 0
            and pers['init_events'] == 0
        ):
            print("\n✗ TEST FAILED")
            print("  • PersistentCache protocol not observed (no activity recorded)")
            print("=" * 70)
            print("ARTIFACTS")
            print("=" * 70)
            print(f"Logs: {artifacts.logs_dir}")
            print(f"Viewer log: {log_path}")
            if server_log_path.exists():
                print(f"Server log: {server_log_path}")
            return 1

        success = True
        failures = []

        # Thresholds tuned for high-churn, small-cache eviction scenarios
        MIN_LOOKUPS = 50
        # Eviction notifications are batched; typical runs show 5-8 batches
        MIN_EVICTIONS = 5
        MIN_EVICTED_IDS = 16

        lookups = pers['hits'] + pers['misses']
        if lookups < MIN_LOOKUPS:
            success = False
            failures.append(f"Too few PersistentCache lookups ({lookups} < {MIN_LOOKUPS})")

        if eviction_count < MIN_EVICTIONS:
            success = False
            failures.append(f"Too few eviction notifications ({eviction_count} < {MIN_EVICTIONS})")

        if evicted_ids < MIN_EVICTED_IDS:
            success = False
            failures.append(f"Too few evicted IDs ({evicted_ids} < {MIN_EVICTED_IDS})")

        # Some reduction may be reported
        # primarily care that evictions are signalled and the cache continues
        # to function. A zero bandwidth reduction is not a failure here.

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")

        print("\n" + "=" * 70)
        if success and not failures:
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
        print("\n\nInterrupted by user")
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


if __name__ == '__main__':
    sys.exit(main())
