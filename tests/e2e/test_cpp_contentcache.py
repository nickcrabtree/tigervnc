#!/usr/bin/env python3
"""
End-to-end test: C++ viewer ContentCache functionality.

Validates that the C++ viewer (njcvncviewer) properly utilizes the
session-only "ContentCache" policy when connected to the C++ server
(Xnjcvnc).

IMPORTANT: ContentCache is now implemented as an alias to the unified
GlobalClientPersistentCache engine with disk persistence disabled
(PersistentCache=0). The underlying cache engine is the same for both
ContentCache and PersistentCache; only the policy differs (memory-only
vs disk-backed). Therefore, it's expected to see PersistentCache
initialization in logs even when PersistentCache=0, as the unified
engine constructs the cache with disk writes disabled.

Test validates:
- Cache hits occur (> 20% hit rate confirms functionality)
- Bandwidth reduction occurs
- No crashes or protocol errors

Test content requirements:
- Images must be ≥64×64 pixels (4096 pixels) to pass server threshold
- Recommended: 96×96 (9216 px) or 128×128 (16384 px) for reliable testing
- Smaller content may be subdivided below the 2048 pixel minimum threshold

Note: Hit rates depend heavily on content patterns and rectangle subdivision.
This test confirms the cache is WORKING, not that it achieves maximum efficiency.
Production workloads with repeated UI elements will see much higher hit rates.
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
    PROJECT_ROOT, BUILD_DIR
)
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, compute_metrics


def run_cpp_viewer(viewer_path, port, artifacts, tracker, name, 
                   cache_size_mb=256, display_for_viewer=None):
    """Run C++ viewer with ContentCache enabled."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        f'ContentCacheSize={cache_size_mb}',
        'PersistentCache=0',  # Disable PersistentCache to test ContentCache only
    ]

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name} with ContentCache={cache_size_mb}MB...")
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
        description='Test C++ viewer ContentCache functionality'
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
                        help='Content cache size in MB (default: 256MB)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')
    parser.add_argument('--hit-rate-threshold', type=float, default=20.0,
                        help='Minimum cache hit rate percentage (default: 20)')
    parser.add_argument('--bandwidth-threshold', type=float, default=10.0,
                        help='Minimum bandwidth reduction percentage (default: 10). Set to 0 to disable enforcement.')

    args = parser.parse_args()

    print("=" * 70)
    print("C++ Viewer ContentCache Test")
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
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    
    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = 'local'
        print(f"\nUsing local Xnjcvnc server")
    else:
        server_mode = 'system'
        print(f"\nUsing system Xtigervnc server")

    try:
        # 4. Start content server with ContentCache only
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        print("  Server config: unified cache engine enabled; viewer runs in ContentCache (ephemeral) mode via PersistentCache=0")
        server_content = VNCServer(
            args.display_content, args.port_content, "cpp_cc_content",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
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
            args.display_viewer, args.port_viewer, "cpp_cc_viewerwin",
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

        # 6. Launch C++ viewer with ContentCache
        print(f"\n[5/8] Launching C++ viewer with ContentCache={args.cache_size}MB...")
        test_proc = run_cpp_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'cpp_cc_test_viewer', cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ C++ viewer connected")

        # 7. Run tiled logos scenario
        print(f"\n[6/8] Running tiled logos scenario...")
        print("  Strategy: Display 12 identical logos at different positions")
        print("  Expected: Cache hits after first logo is encoded")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        # Display 12 copies of the TigerVNC logo with delays
        stats = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  Scenario completed: {stats}")
        time.sleep(3.0)

        # Check if viewer is still running
        if test_proc.poll() is not None:
            exit_code = test_proc.returncode
            print(f"\n✗ FAIL: Viewer exited during scenario (exit code: {exit_code})")
            if exit_code < 0:
                import signal
                sig = -exit_code
                sig_name = signal.Signals(sig).name if sig in [s.value for s in signal.Signals] else str(sig)
                print(f"  Viewer was killed by signal {sig} ({sig_name})")
                if sig == signal.SIGSEGV.value:
                    print("  *** SEGMENTATION FAULT detected ***")
            return 1

        # 8. Stop viewer and analyze
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup('cpp_cc_test_viewer')

        log_path = artifacts.logs_dir / 'cpp_cc_test_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        cache_ops = metrics['cache_operations']
        hit_rate = cache_ops['hit_rate']
        bandwidth_reduction = cache_ops.get('bandwidth_reduction_pct', 0.0)

        print(f"\nContentCache Performance:")
        print(f"  Cache lookups: {cache_ops['total_lookups']}")
        print(f"  Cache hits:    {cache_ops['total_hits']} ({hit_rate:.1f}%)")
        print(f"  Cache misses:  {cache_ops['total_misses']}")
        print(f"  Bandwidth reduction: {bandwidth_reduction:.1f}%")

        # Validation
        success = True
        failures = []

        # NOTE: We no longer check for PersistentCache initialization since
        # the unified cache engine (GlobalClientPersistentCache) is used for
        # both ContentCache (session-only) and PersistentCache (disk-backed).
        # When PersistentCache=0, the engine is still constructed but disk
        # persistence is disabled. Seeing PersistentCache init messages is
        # expected and correct behavior.

        if hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(f"Hit rate {hit_rate:.1f}% < {args.hit_rate_threshold}% threshold")

        # Enforce bandwidth reduction when ContentCache was negotiated. If the
        # viewer reports that ContentCache is active, we *require* that a
        # ContentCache bandwidth summary is printed; missing stats are a test
        # failure, not a skip.
        if args.bandwidth_threshold > 0.0:
            if parsed.negotiated_contentcache:
                if bandwidth_reduction <= 0.0:
                    success = False
                    failures.append(
                        "ContentCache statistics were not logged even though ContentCache was negotiated"
                    )
                elif bandwidth_reduction < args.bandwidth_threshold:
                    success = False
                    failures.append(
                        f"Bandwidth reduction {bandwidth_reduction:.1f}% < {args.bandwidth_threshold}% threshold"
                    )
            else:
                # If ContentCache was not negotiated (unexpected for this test),
                # keep the previous soft behaviour.
                if bandwidth_reduction > 0.0 and bandwidth_reduction < args.bandwidth_threshold:
                    success = False
                    failures.append(
                        f"Bandwidth reduction {bandwidth_reduction:.1f}% < {args.bandwidth_threshold}% threshold"
                    )
                elif bandwidth_reduction <= 0.0:
                    print("\nNote: Bandwidth summary not available in viewer log; skipping bandwidth threshold enforcement.")

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")
        print(f"Content server log: {artifacts.logs_dir / f'cpp_cc_content_server_{args.display_content}.log'}")

        if success:
            print("\n✓ TEST PASSED")
            print(f"\nC++ viewer ContentCache working correctly:")
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
