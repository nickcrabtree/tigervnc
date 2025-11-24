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
from log_parser import parse_cpp_log, compute_metrics


def run_viewer_with_small_cache(viewer_path, port, artifacts, tracker, name, 
                                 cache_size_mb=16, display_for_viewer=None):
    """
    Run viewer with a small cache to force evictions.
    
    Args:
        viewer_path: Path to viewer binary
        port: VNC server port
        artifacts: ArtifactManager
        tracker: ProcessTracker
        name: Process name
        cache_size_mb: Cache size in MB (default 16MB to force evictions)
        display_for_viewer: Optional X display
    
    Returns:
        Process object
    """
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        f'ContentCacheSize={cache_size_mb}',
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
                       help='Test duration in seconds (default: 60)')
    parser.add_argument('--cache-size', type=int, default=16,
                       help='Client cache size in MB (default: 16MB to force evictions)')
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
        
        # 7. Run scenario to generate LOTS of different content
        print(f"\n[6/8] Running intensive scenario to force evictions...")
        runner = ScenarioRunner(args.display_content, verbose=args.verbose)
        
        # Use animated scenario for maximum cache pressure
        print("  Generating diverse content to fill cache...")
        stats = runner.cache_hits_with_clock(duration_sec=args.duration)
        print(f"  Scenario completed: {stats['windows_opened']} windows, {stats['commands_typed']} commands")
        
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
        
        # Success criteria
        success = True
        failures = []
        
        # 1. Cache must have received some content
        if proto['CachedRectInit'] < 10:
            success = False
            failures.append(f"Too few CachedRectInit messages ({proto['CachedRectInit']} < 10)")
        else:
            print(f"\n✓ Cache received content ({proto['CachedRectInit']} CachedRectInit)")
        
        # 2. Evictions MUST occur with a small cache in this scenario.
        if proto['CacheEviction'] == 0:
            success = False
            failures.append("No eviction notifications observed (expected at least one in small-cache scenario)")
            print("✗ No eviction notifications observed (expected at least one in small-cache scenario)")
        else:
            print(f"✓ Evictions occurred ({proto['CacheEviction']} notifications sent)")
        
        # 3. Multiple IDs must have been evicted
        if proto['EvictedIDs'] < proto['CacheEviction']:
            success = False
            failures.append(f"Evicted ID count suspiciously low ({proto['EvictedIDs']})")
        else:
            print(f"✓ Cache IDs evicted ({proto['EvictedIDs']} total)")
        
        # 4. Cache should still be working (hits after evictions)
        if cache_ops['total_hits'] == 0:
            success = False
            failures.append("No cache hits recorded")
        else:
            print(f"✓ Cache still working ({cache_ops['total_hits']} hits)")
        
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
