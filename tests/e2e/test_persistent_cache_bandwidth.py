#!/usr/bin/env python3
"""
End-to-end test: PersistentCache bandwidth reduction.

Runs a repeated-content scenario and asserts that the viewer reports
significant (> 80%) PersistentCache bandwidth reduction.
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
    PROJECT_ROOT
)
from scenarios import ScenarioRunner
from log_parser import parse_cpp_log, compute_metrics


def run_viewer_with_persistent_cache(viewer_path, port, artifacts, tracker, name, display_for_viewer=None):
    """Run viewer with PersistentCache enabled (default size)."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'PersistentCache=1',
    ]

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name} with PersistentCache enabled...")
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
        description='Test PersistentCache bandwidth reduction'
    )
    parser.add_argument('--display-content', type=int, default=998,
                        help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                        help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                        help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                        help='Port for viewer window server (default: 6899)')
    parser.add_argument('--duration', type=int, default=90,
                        help='Scenario duration in seconds (default: 90)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')

    args = parser.parse_args()

    print("=" * 70)
    print("PersistentCache Bandwidth Test")
    print("=" * 70)
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
    local_server_symlink = PROJECT_ROOT / 'build' / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = PROJECT_ROOT / 'build' / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'

    print(f"\nUsing server mode: {server_mode}")

    try:
        # 4. Start content server
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "pc_content_bandwidth",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode
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
            args.display_viewer, args.port_viewer, "pc_viewer_window_bandwidth",
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

        # 6. Launch viewer (PersistentCache enabled)
        print(f"\n[5/8] Launching viewer...")
        test_proc = run_viewer_with_persistent_cache(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'pc_bandwidth_test_viewer', display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ Test viewer connected")

        # 7. Run scenario with repeated content (expect high hit rate)
        print(f"\n[6/8] Running repeated-content scenario...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        stats = runner.cache_hits_minimal(duration_sec=args.duration)
        print(f"  Scenario completed: {stats}")
        time.sleep(3.0)

        # 8. Stop viewer and analyze
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup('pc_bandwidth_test_viewer')

        log_path = artifacts.logs_dir / 'pc_bandwidth_test_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        pers = metrics['persistent']
        print(f"PersistentCache Bandwidth Reduction: {pers['bandwidth_reduction_pct']:.1f}%")

        success = True
        failures = []

        if pers['bandwidth_reduction_pct'] < 80.0:
            success = False
            failures.append(f"Insufficient PersistentCache reduction: {pers['bandwidth_reduction_pct']:.1f}% (< 80%)")

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
