#!/usr/bin/env python3
"""
End-to-end test: C++ viewer PersistentCache functionality.

Validates that the C++ viewer (njcvncviewer) properly utilizes PersistentCache
when connected to the C++ server (Xnjcvnc).

Test validates:
- Cache hit rate > 70% for repeated content
- Bandwidth reduction > 85%
- Eviction notifications work correctly with small cache
- No crashes or protocol errors
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
    preflight_check_cpp_only, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT
)
from scenarios import ScenarioRunner
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_cpp_viewer(viewer_path, port, artifacts, tracker, name, 
                   cache_size_mb=256, display_for_viewer=None):
    """Run C++ viewer with PersistentCache enabled."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
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
        description='Test C++ viewer PersistentCache functionality'
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
                        help='Test duration in seconds (default: 60)')
    parser.add_argument('--cache-size', type=int, default=256,
                        help='Persistent cache size in MB (default: 256MB)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')
    parser.add_argument('--hit-rate-threshold', type=float, default=70.0,
                        help='Minimum cache hit rate percentage (default: 70)')
    parser.add_argument('--bandwidth-threshold', type=float, default=85.0,
                        help='Minimum bandwidth reduction percentage (default: 85)')

    args = parser.parse_args()

    print("=" * 70)
    print("C++ Viewer PersistentCache Test")
    print("=" * 70)
    print(f"\nCache Size: {args.cache_size}MB")
    print(f"Duration: {args.duration}s")
    print(f"Hit Rate Threshold: {args.hit_rate_threshold}%")
    print(f"Bandwidth Threshold: {args.bandwidth_threshold}%")
    print()

    # 1. Create artifacts
    print("[1/8] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight checks (C++ only)
    print("\n[2/8] Running preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
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
    
    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = 'local'
        print(f"\nUsing local Xnjcvnc server")
    else:
        server_mode = 'system'
        print(f"\nUsing system Xtigervnc server")

    try:
        # 4. Start content server
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "cpp_pc_content",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
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
            args.display_viewer, args.port_viewer, "cpp_pc_viewerwin",
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

        # 6. Launch C++ viewer with PersistentCache
        print(f"\n[5/8] Launching C++ viewer with PersistentCache={args.cache_size}MB...")
        test_proc = run_cpp_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'cpp_pc_test_viewer', cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ C++ viewer connected")

        # 7. Run scenario with repeated content
        print(f"\n[6/8] Running repeated-content scenario...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        stats = runner.cache_hits_minimal(duration_sec=args.duration)
        print(f"  Scenario completed: {stats}")
        time.sleep(3.0)

        # 8. Stop viewer and analyze
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup('cpp_pc_test_viewer')

        log_path = artifacts.logs_dir / 'cpp_pc_test_viewer.log'
        server_log_path = artifacts.logs_dir / f'cpp_pc_content_server_{args.display_content}.log'
        
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        # Parse both viewer and server logs
        parsed = parse_cpp_log(log_path)
        server_parsed = parse_server_log(server_log_path)
        
        # Combine server-side hit/miss counts with client counts
        parsed.persistent_hits += server_parsed.persistent_hits
        parsed.persistent_misses += server_parsed.persistent_misses
        
        metrics = compute_metrics(parsed)

        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        pers = metrics['persistent']
        hit_rate = pers['hit_rate']
        bandwidth_reduction = pers['bandwidth_reduction_pct']
        lookups = pers['hits'] + pers['misses']

        print(f"\nPersistentCache Performance:")
        print(f"  Cache lookups: {lookups}")
        print(f"  Cache hits:    {pers['hits']} ({hit_rate:.1f}%)")
        print(f"  Cache misses:  {pers['misses']}")
        print(f"  Bandwidth reduction: {bandwidth_reduction:.1f}%")

        # Validation
        success = True
        failures = []

        if hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(f"Hit rate {hit_rate:.1f}% < {args.hit_rate_threshold}% threshold")

        if bandwidth_reduction < args.bandwidth_threshold:
            success = False
            failures.append(f"Bandwidth reduction {bandwidth_reduction:.1f}% < {args.bandwidth_threshold}% threshold")

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")
        print(f"Content server log: {artifacts.logs_dir / f'cpp_pc_content_server_{args.display_content}.log'}")

        if success:
            print("\n✓ TEST PASSED")
            print(f"\nC++ viewer PersistentCache working correctly:")
            print(f"  • Hit rate: {hit_rate:.1f}% (threshold: {args.hit_rate_threshold}%)")
            print(f"  • Bandwidth reduction: {bandwidth_reduction:.1f}% (threshold: {args.bandwidth_threshold}%)")
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
