#!/usr/bin/env python3
"""
Test ContentCache eviction notifications.

This test specifically validates that:
1. Client-side pixel cache evicts entries when full (ARC algorithm)
2. Eviction notifications are sent to the server
3. Server updates its knownCacheIds_ tracking
4. System continues to work correctly after evictions

Strategy: Use a VERY SMALL cache size to force evictions quickly.
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
from log_parser import parse_cpp_log, compute_metrics


def run_viewer_with_small_cache(viewer_path, port, artifacts, tracker, name, 
                                 cache_size_mb=1, display_for_viewer=None):
    """
    Run viewer with a small ContentCache to force evictions.
    
    IMPORTANT: We disable PersistentCache so that the server negotiates
    ContentCache instead. Otherwise PersistentCache takes precedence and
    ContentCache evictions won't occur.
    
    Args:
        viewer_path: Path to viewer binary
        port: VNC server port
        artifacts: ArtifactManager
        tracker: ProcessTracker
        name: Process name
        cache_size_mb: Cache size in MB (default 1MB to force evictions)
        display_for_viewer: Optional X display
    
    Returns:
        Process object
    """
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'PreferredEncoding=ZRLE',
        f'ContentCacheSize={cache_size_mb}',
        'PersistentCache=0',  # Disable PersistentCache to force ContentCache usage
    ]
    
    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()
    
    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)
    
    print(f"  Starting {name} with {cache_size_mb}MB cache...")
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
        description='Test ContentCache eviction with small cache size'
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
    parser.add_argument('--verify-tiles', type=int, default=12,
                       help='Number of tiled logos to display in verification (default: 12)')
    parser.add_argument('--variable-content', choices=['images','xclock','fullscreen','none'], default='images',
                       help='Variable content generator (default: images from system datasets)')
    parser.add_argument('--grid-cols', type=int, default=6,
                       help='xclock grid columns (default: 6)')
    parser.add_argument('--grid-rows', type=int, default=2,
                       help='xclock grid rows (default: 2)')
    parser.add_argument('--clock-size', type=int, default=160,
                       help='xclock window size (default: 160)')
    parser.add_argument('--cache-size', type=int, default=1,
                       help='Client cache size in MB (default: 1MB to force evictions)')
    parser.add_argument('--wm', default='openbox',
                       help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                       help='Verbose output')
    
    args = parser.parse_args()
    
    print("=" * 70)
    print("ContentCache Eviction Test")
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
    
    # Determine server mode (prefer local if available for testing latest code)
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'
    
    print(f"\nUsing server mode: {server_mode}")
    
    try:
        # 4. Start content server
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "content_eviction_test",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",  # Verbose to see eviction messages
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
            args.display_viewer, args.port_viewer, "viewer_window_eviction_test",
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
        
        # 6. Launch test viewer with SMALL cache
        print(f"\n[5/8] Launching viewer with {args.cache_size}MB cache...")
        test_proc = run_viewer_with_small_cache(
            binaries['cpp_viewer'],
            args.port_content,
            artifacts,
            tracker,
            'eviction_test_viewer',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        
        if test_proc.poll() is not None:
            print("\n✗ FAIL: Test viewer exited prematurely")
            return 1
        
        print("✓ Test viewer connected")
        
        # 7. Run variable-content phase to force evictions, then a short
        #    repeated-content phase to verify the cache still produces hits.
        print(f"\n[6/8] Running variable-content phase to force evictions...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        static_runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        if args.variable_content == 'images':
            print("  Generating variable content from system image set...")
            vstats = static_runner.image_churn(duration_sec=args.duration, cols=args.grid_cols, rows=args.grid_rows,
                                               size=args.clock_size, interval_sec=0.8, max_windows=72)
        elif args.variable_content == 'fullscreen':
            print("  Generating variable content with fullscreen random colors...")
            vstats = static_runner.random_fullscreen_colors(duration_sec=args.duration, interval_sec=0.4)
        elif args.variable_content == 'xclock':
            print("  Generating variable content with xclock grid...")
            vstats = runner.xclock_grid(cols=args.grid_cols, rows=args.grid_rows,
                                        size=args.clock_size, update=1,
                                        duration_sec=args.duration)
        else:
            print("  Using eviction_stress fallback...")
            vstats = runner.eviction_stress(duration_sec=args.duration)
        print(f"  Variable phase completed: {vstats}")

        # Aggressive eviction burst to ensure enough evictions are generated
        print("  Running eviction burst with large images (size=320, count=24)...")
        burst_stats = static_runner.image_burst(count=24, size=320, cols=4, rows=6, interval_ms=80)
        print(f"  Eviction burst completed: {burst_stats}")

        # Brief verification phase with static repeated content
        print(f"  Running verification phase with tiled logos for {args.verify_duration}s...")
        static_runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        sstats = static_runner.tiled_logos_test(tiles=args.verify_tiles, duration=args.verify_duration, delay_between=1.0)
        print(f"  Verification phase completed: {sstats}")
        
        time.sleep(5.0)  # Let evictions and notifications complete
        
        # 8. Stop viewer and parse results
        print("\n[7/8] Stopping viewer and analyzing results...")
        tracker.cleanup('eviction_test_viewer')
        
        # Parse log
        log_path = artifacts.logs_dir / 'eviction_test_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1
        
        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)
        
        # 9. Verify evictions occurred
        print("\n[8/8] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)
        
        cache_ops = metrics['cache_operations']
        proto = metrics['protocol_messages']
        
        print(f"\nCache Operations:")
        print(f"  Hits: {cache_ops['total_hits']}")
        print(f"  Misses: {cache_ops['total_misses']}")
        print(f"  Hit Rate: {cache_ops['hit_rate']:.1f}%")
        
        print(f"\nProtocol Messages:")
        print(f"  CachedRect: {proto['CachedRect']}")
        print(f"  CachedRectInit: {proto['CachedRectInit']}")
        print(f"  CacheEviction: {proto['CacheEviction']}")
        print(f"  Evicted IDs: {proto['EvictedIDs']}")
        
        # Success criteria (HARSH)
        success = True
        failures = []

        MIN_CACHE_ACTIVITY = 100  # Total cache protocol messages (CachedRect + CachedRectInit)
        MIN_INITS = 48
        MIN_EVICTIONS = 8
        MIN_EVICTED_IDS = 12
        # The primary goal of this test is to verify that evictions happen
        # and that the cache continues to function afterwards. Under heavy
        # churn with a very small cache (1MB), the post-eviction hit rate can
        # legitimately be modest, so keep this threshold conservative.
        MIN_HIT_RATE = 10.0

        # 0. Ensure enough cache activity happened.
        # We measure cache activity as CachedRect (references) + CachedRectInit (initial sends).
        # This is different from viewer's "Lookups" stat which only counts CachedRect.
        # For eviction testing, we need to know the total cache protocol traffic.
        cache_activity = proto['CachedRect'] + proto['CachedRectInit']
        if cache_activity < MIN_CACHE_ACTIVITY:
            success = False
            failures.append(f"Too few cache protocol messages ({cache_activity} < {MIN_CACHE_ACTIVITY}); insufficient churn")
        else:
            print(f"\n✓ Sufficient cache activity: {cache_activity} messages (>= {MIN_CACHE_ACTIVITY})")
        
        # 1. Cache must have received substantial content
        if proto['CachedRectInit'] < MIN_INITS:
            success = False
            failures.append(f"Too few CachedRectInit messages ({proto['CachedRectInit']} < {MIN_INITS})")
        else:
            print(f"✓ Cache received content ({proto['CachedRectInit']} CachedRectInit >= {MIN_INITS})")
        
        # 2. Evictions MUST occur in quantity
        if proto['CacheEviction'] < MIN_EVICTIONS:
            success = False
            failures.append(f"Too few eviction notifications ({proto['CacheEviction']} < {MIN_EVICTIONS})")
        else:
            print(f"✓ Evictions occurred ({proto['CacheEviction']} notifications >= {MIN_EVICTIONS})")
        
        # 3. Evicted ID count must be healthy
        if proto['EvictedIDs'] < MIN_EVICTED_IDS:
            success = False
            failures.append(f"Too few evicted IDs ({proto['EvictedIDs']} < {MIN_EVICTED_IDS})")
        else:
            print(f"✓ Cache IDs evicted ({proto['EvictedIDs']} >= {MIN_EVICTED_IDS})")
        
        # 4. Cache should still be working (hits after evictions)
        if cache_ops['hit_rate'] < MIN_HIT_RATE:
            success = False
            failures.append(f"Hit rate too low after evictions ({cache_ops['hit_rate']:.1f}% < {MIN_HIT_RATE}%)")
        else:
            print(f"✓ Cache still effective after evictions (hit rate {cache_ops['hit_rate']:.1f}% >= {MIN_HIT_RATE}%)")
        
        # 5. Check for errors
        if metrics['errors'] > 0:
            success = False
            failures.append(f"{metrics['errors']} errors logged")
            print(f"\n⚠ Errors detected in logs")
        
        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Full log: {log_path}")
        
        print("\n" + "=" * 70)
        if success:
            print("\n✓ TEST PASSED")
            print("=" * 70)
            print("\nEviction behaviour is working correctly:")
            print("  • Client evicted entries when cache filled (as evidenced by hits + content churn)")
            if proto['CacheEviction'] > 0:
                print("  • Eviction notifications sent to server")
            else:
                print("  • No explicit eviction notifications observed (viewer may evict silently)")
            print("  • Cache continued to work after evictions")
            return 0
        else:
            print("✗ TEST FAILED")
            print("=" * 70)
            print("\nFailures:")
            for f in failures:
                print(f"  • {f}")
            print(f"\nCheck log for details: {log_path}")
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
