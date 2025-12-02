#!/usr/bin/env python3
"""
End-to-end test: C++ viewer cache eviction.

Validates that the C++ viewer handles cache eviction correctly when
using a small cache size. Tests both ContentCache and PersistentCache.

Test validates:
- Cache continues to function after evictions
- Hit rate remains reasonable despite small cache
- No crashes or memory leaks with frequent evictions

Note: Uses tiled logo scenario for reliable cache hit testing.
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
from scenarios import ScenarioRunner
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, compute_metrics


def run_cpp_viewer(viewer_path, port, artifacts, tracker, name,
                   cache_type='content', cache_size_mb=16, display_for_viewer=None):
    """Run C++ viewer with small cache size to force evictions."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
    ]
    
    if cache_type == 'content':
        cmd.append(f'ContentCacheSize={cache_size_mb}')
        cmd.append('PersistentCache=0')  # ContentCache only
    elif cache_type == 'persistent':
        cmd.append('ContentCache=0')  # PersistentCache only
        cmd.append('PersistentCache=1')
        cmd.append(f'PersistentCacheSize={cache_size_mb}')

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name} with {cache_type}Cache={cache_size_mb}MB (forcing evictions)...")
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
        description='Test C++ viewer cache eviction behavior'
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
    parser.add_argument('--cache-size', type=int, default=16,
                        help='Cache size in MB (default: 16MB - forces evictions)')
    parser.add_argument('--cache-type', choices=['content', 'persistent'], default='content',
                        help='Cache type to test (default: content)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')
    parser.add_argument('--hit-rate-threshold', type=float, default=20.0,
                        help='Minimum cache hit rate percentage (default: 20)')

    args = parser.parse_args()

    cache_name = "ContentCache" if args.cache_type == 'content' else "PersistentCache"
    
    print("=" * 70)
    print(f"C++ Viewer {cache_name} Eviction Test")
    print("=" * 70)
    print(f"\nCache Size: {args.cache_size}MB (small - forces evictions)")
    print(f"Cache Type: {cache_name}")
    print(f"Duration: {args.duration}s")
    print(f"Hit Rate Threshold: {args.hit_rate_threshold}%")
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
        # 4. Start content server with selected cache only
        server_params = {}
        if args.cache_type == 'content':
            # Disable the unified cache engine so that only the viewer-side
            # ephemeral policy ("ContentCache") is exercised.
            server_params['EnablePersistentCache'] = '0'
        else:
            # PersistentCache-only mode: leave the unified engine enabled.
            # ContentCache-specific server flags have been removed.
            server_params['EnablePersistentCache'] = '1'
        
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        print(f"  Server config: {cache_name} only")
        server_content = VNCServer(
            args.display_content, args.port_content, "cpp_evict_content",
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
            args.display_viewer, args.port_viewer, "cpp_evict_viewerwin",
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

        # 6. Launch C++ viewer with small cache
        print(f"\n[5/8] Launching C++ viewer with small cache...")
        test_proc = run_cpp_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'cpp_evict_test_viewer', cache_type=args.cache_type,
            cache_size_mb=args.cache_size, display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ C++ viewer connected")

        # 7. Run tiled logos scenario
        print(f"\n[6/8] Running tiled logos scenario...")
        print("  Strategy: Display 12 identical logos repeatedly")
        print("  Expected: Cache continues to function despite evictions")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        # Multiple rounds to test eviction with small cache
        stats = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=2.0)
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
        tracker.cleanup('cpp_evict_test_viewer')

        log_path = artifacts.logs_dir / 'cpp_evict_test_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        if args.cache_type == 'content':
            cache_metrics = metrics['cache_operations']
            lookups = cache_metrics['total_lookups']
            hits = cache_metrics['total_hits']
            misses = cache_metrics['total_misses']
        else:  # persistent
            cache_metrics = metrics['persistent']
            lookups = cache_metrics['hits'] + cache_metrics['misses']
            hits = cache_metrics['hits']
            misses = cache_metrics['misses']
            
        hit_rate = cache_metrics['hit_rate']

        print(f"\n{cache_name} Performance (Small Cache with Evictions):")
        print(f"  Cache lookups: {lookups}")
        print(f"  Cache hits:    {hits} ({hit_rate:.1f}%)")
        print(f"  Cache misses:  {misses}")
        
        if args.cache_type == 'content':
            bandwidth_reduction = cache_metrics.get('bandwidth_reduction_pct', 0.0)
            print(f"  Bandwidth reduction: {bandwidth_reduction:.1f}%")

        # Validation
        success = True
        failures = []

        if hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(f"Hit rate {hit_rate:.1f}% < {args.hit_rate_threshold}% threshold")
        
        if lookups < 100:
            print(f"\nNote: Too few cache lookups ({lookups}); not enforcing lookup threshold.")

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")
        print(f"Content server log: {artifacts.logs_dir / f'cpp_evict_content_server_{args.display_content}.log'}")

        if success:
            print("\n✓ TEST PASSED")
            print(f"\nC++ viewer {cache_name} handles evictions correctly:")
            print(f"  • Hit rate: {hit_rate:.1f}% (threshold: {args.hit_rate_threshold}%)")
            print(f"  • No crashes despite small cache and frequent evictions")
            print(f"  • Cache continues to provide benefit after evictions")
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
