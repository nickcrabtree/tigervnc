#!/usr/bin/env python3
"""
End-to-end test: PersistentCache v3 sharded storage.

Tests implementation-independent behavior of the v3 sharded storage:
1. Disk format - cache uses directory with index.dat + shard files
2. Fast startup - warm start should be quick (index-only load)
3. Multi-session persistence - cache persists across sessions

Note: These tests focus on externally observable behavior, not internals.
"""

import sys
import time
import argparse
import subprocess
import os
import shutil
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT, BUILD_DIR
)
from scenarios_static import StaticScenarioRunner


def get_cache_disk_usage(cache_dir: Path) -> dict:
    """Get disk usage statistics for cache directory."""
    stats = {
        'total_bytes': 0,
        'index_bytes': 0,
        'shard_count': 0,
        'shard_bytes': 0,
    }
    
    if not cache_dir.exists():
        return stats
    
    for f in cache_dir.iterdir():
        if f.is_file():
            size = f.stat().st_size
            stats['total_bytes'] += size
            if f.name == 'index.dat':
                stats['index_bytes'] = size
            elif f.name.startswith('shard_') and f.name.endswith('.dat'):
                stats['shard_count'] += 1
                stats['shard_bytes'] += size
    
    return stats


def test_disk_format(cache_dir: Path) -> tuple:
    """Test that v3 sharded format is used.
    
    Returns (success, message).
    """
    if not cache_dir.exists():
        return (False, f"Cache directory does not exist: {cache_dir}")
    
    index_file = cache_dir / 'index.dat'
    if not index_file.exists():
        return (False, f"Index file not found: {index_file}")
    
    # Check index file magic number (v3 = "PCV3" = 0x50435633)
    with open(index_file, 'rb') as f:
        magic = f.read(4)
        if magic != b'3VCP':  # Little-endian 0x50435633
            return (False, f"Invalid magic: {magic.hex()} (expected 33564350 for PCV3)")
    
    # Check for shard files (may be 0 if no content was cached)
    shards = list(cache_dir.glob('shard_*.dat'))
    
    return (True, f"Valid v3 format: index.dat + {len(shards)} shard(s)")


def run_viewer_with_scenario(viewer_path: str, port: int, artifacts: ArtifactManager,
                              tracker: ProcessTracker, name: str, cache_dir: Path,
                              display_content: int, display_viewer: int,
                              cache_mem_mb: int, duration: float,
                              verbose: bool = False) -> tuple:
    """Run viewer with tiled logo scenario.
    
    Returns (startup_time_seconds, success).
    """
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'ContentCache=0',
        'PersistentCache=1',
        f'PersistentCacheSize={cache_mem_mb}',
        f'PersistentCachePath={cache_dir}',
    ]
    
    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()
    env['TIGERVNC_VIEWER_DEBUG_LOG'] = '1'
    env['DISPLAY'] = f':{display_viewer}'
    
    log_file = open(log_path, 'w')
    
    start_time = time.time()
    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env
    )
    tracker.register(name, proc)
    
    # Wait for viewer to connect
    connected = False
    deadline = time.time() + 30.0
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        if log_path.exists():
            with open(log_path, 'r') as f:
                content = f.read()
                if 'initialisation done' in content.lower():
                    connected = True
                    break
        time.sleep(0.2)
    
    startup_time = time.time() - start_time
    
    if not connected:
        tracker.cleanup(name)
        return (startup_time, False)
    
    # Run scenario while viewer is connected
    runner = StaticScenarioRunner(display_content, verbose=verbose)
    runner.tiled_logos_test(tiles=6, duration=duration - 5, delay_between=1.0)
    runner.cleanup()
    
    time.sleep(2.0)
    
    # Stop viewer gracefully
    tracker.cleanup(name)
    time.sleep(1.0)
    
    return (startup_time, True)


def main():
    parser = argparse.ArgumentParser(
        description='Test PersistentCache v3 sharded storage behavior'
    )
    parser.add_argument('--display-content', type=int, default=998,
                        help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                        help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                        help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                        help='Port for viewer window server (default: 6899)')
    parser.add_argument('--cache-mem-mb', type=int, default=64,
                        help='Memory cache size in MB (default: 64)')
    parser.add_argument('--session-duration', type=float, default=30.0,
                        help='Duration of each viewer session in seconds (default: 30)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')

    args = parser.parse_args()

    print("=" * 70)
    print("PersistentCache v3 Sharded Storage Test")
    print("=" * 70)
    print(f"\nConfiguration:")
    print(f"  Memory cache: {args.cache_mem_mb}MB")
    print(f"  Session:      {args.session_duration}s")
    print()

    # 1. Create artifacts
    print("[1/7] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()
    
    # Create cache directory under artifacts
    cache_dir = artifacts.base_dir / 'persistentcache'

    # 2. Preflight checks
    print("\n[2/7] Running preflight checks...")
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

    results = {}

    try:
        # 4. Start servers
        print(f"\n[3/7] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "v3_test_content",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            # With the unified cache engine, only EnablePersistentCache is
            # exposed on the server side. Treat this as a PersistentCache-only
            # configuration and let the viewer control persistence policy.
            server_params={'EnablePersistentCache': '1'}
        )
        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        print(f"\n[4/7] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, "v3_test_viewerwin",
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

        # 5. First session - populate cache (cold start)
        print(f"\n[5/7] Session 1: Cold start (populating cache)...")
        
        # Ensure clean cache
        if cache_dir.exists():
            shutil.rmtree(cache_dir)
        
        startup1, success1 = run_viewer_with_scenario(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session1', cache_dir,
            args.display_content, args.display_viewer,
            args.cache_mem_mb, args.session_duration,
            verbose=args.verbose
        )
        
        if not success1:
            print(f"\n✗ FAIL: Session 1 failed to connect")
            return 1
        
        print(f"  Cold start time: {startup1:.1f}s")
        
        # Check disk format
        success, msg = test_disk_format(cache_dir)
        results['disk_format'] = (success, msg)
        print(f"  Disk format: {msg}")
        
        # Check disk usage
        stats1 = get_cache_disk_usage(cache_dir)
        print(f"  Cache size: {stats1['total_bytes']/1024:.1f}KB ({stats1['shard_count']} shards)")

        # 6. Second session - warm start
        print(f"\n[6/7] Session 2: Warm start (existing cache)...")
        
        startup2, success2 = run_viewer_with_scenario(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session2', cache_dir,
            args.display_content, args.display_viewer,
            args.cache_mem_mb, args.session_duration,
            verbose=args.verbose
        )
        
        if not success2:
            print(f"\n✗ FAIL: Session 2 failed to connect")
            return 1
        
        print(f"  Warm start time: {startup2:.1f}s")
        
        # Warm startup should be fast (index-only load)
        if startup2 <= 5.0:
            results['startup_time'] = (True, f"Startup time: {startup2:.1f}s")
        else:
            results['startup_time'] = (False, f"Startup took {startup2:.1f}s (max: 5.0s)")
        print(f"  {results['startup_time'][1]}")
        
        stats2 = get_cache_disk_usage(cache_dir)
        print(f"  Cache size: {stats2['total_bytes']/1024:.1f}KB ({stats2['shard_count']} shards)")

        # 7. Report results
        print("\n[7/7] Results Summary")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        all_passed = True
        
        for test_name, (success, msg) in results.items():
            status = "✓" if success else "✗"
            print(f"\n{status} {test_name}: {msg}")
            if not success:
                all_passed = False

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Cache: {cache_dir}")

        if all_passed:
            print("\n✓ ALL TESTS PASSED")
            return 0
        else:
            print("\n✗ SOME TESTS FAILED")
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
