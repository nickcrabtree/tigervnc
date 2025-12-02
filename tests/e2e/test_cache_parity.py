#!/usr/bin/env python3
"""
End-to-end test: Cache parity between ContentCache and PersistentCache.

Runs identical workload twice and compares hit rates between ContentCache
and PersistentCache (< 5 percentage point difference).

Notes:
- On this macOS machine, preflight may fail; run on Linux per README.
- Viewer parameters attempt to steer protocol selection:
  - ContentCache-only run tries PersistentCache=0 (best-effort)
  - PersistentCache run uses PersistentCache=1 (preferred by server code)
- Even if both are advertised, the server prefers PersistentCache. This test
  still captures both CC and PC metrics from the logs and compares hit rates.
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
from log_parser import parse_cpp_log, compute_metrics


def run_viewer(viewer_path, port, artifacts, tracker, name, params=None, display_for_viewer=None):
    cmd = [viewer_path, f'127.0.0.1::{port}', 'Shared=1', 'Log=*:stderr:100']
    if params:
        cmd.extend(params)

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()
    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name}...")
    log_file = open(log_path, 'w')
    proc = subprocess.Popen(cmd, stdout=log_file, stderr=subprocess.STDOUT,
                            preexec_fn=os.setpgrp, env=env)
    tracker.register(name, proc)
    time.sleep(2.0)
    return proc


def main():
    parser = argparse.ArgumentParser(description='Cache parity test: ContentCache vs PersistentCache')
    parser.add_argument('--display-content', type=int, default=998)
    parser.add_argument('--port-content', type=int, default=6898)
    parser.add_argument('--display-viewer', type=int, default=999)
    parser.add_argument('--port-viewer', type=int, default=6899)
    parser.add_argument('--duration', type=int, default=90)
    parser.add_argument('--wm', default='openbox')
    parser.add_argument('--verbose', action='store_true')
    parser.add_argument('--tolerance', type=float, default=0.0,
                        help='Allowed hit rate difference (percentage points). '
                             'With cold caches and identical workload, hit rates must match exactly.')

    args = parser.parse_args()

    print("=" * 70)
    print("Cache Parity Test (ContentCache vs PersistentCache)")
    print("=" * 70)
    print(f"Duration: {args.duration}s  Tolerance: ±{args.tolerance:.1f} pp")

    # 1. Artifacts
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight
    print("\n[Preflight] Checking environment...")
    try:
        binaries = preflight_check(verbose=args.verbose)
    except PreflightError as e:
        print("\n✗ FAIL: Preflight checks failed\n")
        print(e)
        return 1

    if not check_port_available(args.port_content) or not check_port_available(args.port_viewer):
        print("\n✗ FAIL: Required ports are in use")
        return 1
    if not check_display_available(args.display_content) or not check_display_available(args.display_viewer):
        print("\n✗ FAIL: Required displays are in use")
        return 1

    tracker = ProcessTracker()

    # Determine server mode
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'

    try:
        # 3. Start content + viewer display servers
        print(f"\n[Servers] Starting content server :{args.display_content}...")
        server_content = VNCServer(args.display_content, args.port_content, 'pc_parity_content',
                                   artifacts, tracker, geometry='1920x1080',
                                   log_level='*:stderr:30', server_choice=server_mode)
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server")
            return 1

        print(f"[Servers] Starting viewer window server :{args.display_viewer}...")
        server_viewer = VNCServer(args.display_viewer, args.port_viewer, 'pc_parity_viewerwin',
                                  artifacts, tracker, geometry='1920x1080',
                                  log_level='*:stderr:30', server_choice=server_mode)
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server")
            return 1

        # 4. ContentCache run (best effort to disable PersistentCache)
        print("\n[Run 1/2] ContentCache-focused run")
        viewer1 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'parity_cc_viewer', params=['PersistentCache=0', 'ContentCacheSize=256'],
            display_for_viewer=args.display_viewer,
        )
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        runner.cache_hits_minimal(duration_sec=args.duration)
        time.sleep(3.0)
        tracker.cleanup('parity_cc_viewer')
        log_cc = artifacts.logs_dir / 'parity_cc_viewer.log'
        parsed_cc = parse_cpp_log(log_cc)
        metrics_cc = compute_metrics(parsed_cc)

        # Stop servers to guarantee cold server-side state for next phase
        server_viewer.stop()
        server_content.stop()

        # Restart servers for PersistentCache phase
        print(f"\n[Servers] Restarting for PersistentCache phase :{args.display_content} and :{args.display_viewer}...")
        server_content = VNCServer(args.display_content, args.port_content, 'pc_parity_content2',
                                   artifacts, tracker, geometry='1920x1080',
                                   log_level='*:stderr:30', server_choice=server_mode)
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not restart content server")
            return 1
        server_viewer = VNCServer(args.display_viewer, args.port_viewer, 'pc_parity_viewerwin2',
                                  artifacts, tracker, geometry='1920x1080',
                                  log_level='*:stderr:30', server_choice=server_mode)
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not restart viewer window server")
            return 1

        # 5. PersistentCache run (use fresh cache directory for cold start)
        # v3 uses a directory with index.dat + shard files instead of a single file
        print("\n[Run 2/2] PersistentCache-focused run (cold cache)")
        pc_cache_path = artifacts.logs_dir / 'pc_test_cache'
        viewer2 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'parity_pc_viewer', params=[
                'PersistentCache=1',
                'ContentCache=0',
                f'PersistentCachePath={pc_cache_path}',  # Fresh cache path
            ],
            display_for_viewer=args.display_viewer,
        )
        runner.cache_hits_minimal(duration_sec=args.duration)
        time.sleep(3.0)
        tracker.cleanup('parity_pc_viewer')
        log_pc = artifacts.logs_dir / 'parity_pc_viewer.log'
        parsed_pc = parse_cpp_log(log_pc)
        metrics_pc = compute_metrics(parsed_pc)
 
        # Derive PersistentCache hit/miss counts from viewer logs in the
        # same way we do for the back-to-back C++ test: count each
        # PersistentCachedRectInit as a miss-equivalent and each
        # PersistentCache HIT as a hit. This ensures that, for a cold
        # cache and identical workload, the hit/miss profile matches the
        # ContentCache run exactly.
        pc_init_count = 0
        with open(log_pc, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                if "Received PersistentCachedRectInit" in line:
                    pc_init_count += 1
        pc_hits = parsed_pc.persistent_hits
        pc_misses = pc_init_count
        pc_lookups = pc_hits + pc_misses
        pc_hit_rate = (100.0 * pc_hits / pc_lookups) if pc_lookups > 0 else 0.0
 
        # 6. Compare
        print("\n[Compare] Results")
        cc_hit_rate = metrics_cc['cache_operations']['hit_rate']
 
        print(f"  ContentCache hit rate:      {cc_hit_rate:.1f}%")
        print(f"  PersistentCache hit rate:   {pc_hit_rate:.1f}%")
 
        diff = abs(cc_hit_rate - pc_hit_rate)
        print(f"  Difference: {diff:.1f} percentage points")

        # If PersistentCache is not active at all, this is a hard failure: the
        # purpose of this test is to exercise and compare both caches.
        if pc_hit_rate == 0.0:
            print("\n✗ TEST FAILED")
            print("  • PersistentCache activity not observed (hit rate 0.0%)")
            print("  • Cannot compare hit rates between ContentCache and PersistentCache")
            print("\n" + "=" * 70)
            print("ARTIFACTS")
            print("=" * 70)
            print(f"Logs: {artifacts.logs_dir}")
            print(f"CC log: {log_cc}")
            print(f"PC log: {log_pc}")
            return 1

        success = True
        failures = []

        if diff > args.tolerance:
            success = False
            failures.append(f"Hit rates differ by {diff:.1f} pp (> {args.tolerance:.1f} pp)")

        # Also print PC bandwidth reduction for visibility
        pc_bw = metrics_pc['persistent']['bandwidth_reduction_pct']
        print(f"  PersistentCache bandwidth reduction: {pc_bw:.1f}%")

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"CC log: {log_cc}")
        print(f"PC log: {log_pc}")

        if success:
            print("\n✓ TEST PASSED")
            return 0
        else:
            print("\n✗ TEST FAILED")
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


if __name__ == '__main__':
    sys.exit(main())
