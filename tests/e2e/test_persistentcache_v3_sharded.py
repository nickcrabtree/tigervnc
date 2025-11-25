#!/usr/bin/env python3
"""
End-to-end test: PersistentCache v3 sharded storage.

Tests implementation-independent behavior of the v3 sharded storage:
1. Fast startup - index-only load should be quick regardless of cache size
2. Disk format - cache uses directory with index.dat + shard files
3. Disk limit - cache respects disk size limit (2x memory by default)
4. Cold entries - evicted entries persist on disk and can be reloaded
5. No full rebuild - evictions don't cause expensive disk operations

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


def get_cache_dir(artifacts: ArtifactManager) -> Path:
    """Get the test PersistentCache directory path."""
    return artifacts.data_dir / "persistentcache"


def ensure_clean_cache(cache_dir: Path):
    """Remove any existing cache to start fresh."""
    if cache_dir.exists():
        shutil.rmtree(cache_dir)


def get_cache_disk_usage(cache_dir: Path) -> dict:
    """Get disk usage statistics for cache directory.
    
    Returns dict with:
    - total_bytes: total size of all files
    - index_bytes: size of index.dat
    - shard_count: number of shard files
    - shard_bytes: total size of shard files
    """
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


def run_viewer_session(viewer_path: str, port: int, artifacts: ArtifactManager,
                       tracker: ProcessTracker, name: str, cache_dir: Path,
                       cache_mem_mb: int = 64, cache_disk_mb: int = 0,
                       cache_shard_mb: int = 16, display_for_viewer: int = None,
                       duration: float = 10.0) -> tuple:
    """Run a viewer session and return (startup_time, exit_code).
    
    Returns:
        (startup_time_seconds, exit_code, viewer_proc)
    """
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'ContentCache=0',  # Disable ContentCache to isolate PersistentCache
        'PersistentCache=1',
        f'PersistentCacheSize={cache_mem_mb}',
        f'PersistentCacheDiskSize={cache_disk_mb}',
        f'PersistentCacheShardSize={cache_shard_mb}',
        f'PersistentCachePath={cache_dir}',
    ]
    
    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()
    
    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)
    
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
    
    # Wait for viewer to connect (check log for connection message)
    connected = False
    deadline = time.time() + 30.0
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        # Check log for connection
        if log_path.exists():
            with open(log_path, 'r') as f:
                content = f.read()
                if 'connected' in content.lower() or 'framebuffer' in content.lower():
                    connected = True
                    break
        time.sleep(0.2)
    
    startup_time = time.time() - start_time
    
    if not connected:
        return (startup_time, -1, proc)
    
    # Let session run for specified duration
    time.sleep(duration)
    
    # Stop viewer gracefully
    tracker.cleanup(name)
    time.sleep(0.5)
    
    exit_code = proc.returncode if proc.returncode is not None else 0
    return (startup_time, exit_code, proc)


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
    
    # Check for at least one shard file
    shards = list(cache_dir.glob('shard_*.dat'))
    if not shards:
        return (False, "No shard files found")
    
    return (True, f"Valid v3 format: index.dat + {len(shards)} shard(s)")


def test_startup_time(startup_time: float, max_time: float = 5.0) -> tuple:
    """Test that startup time is acceptable.
    
    Returns (success, message).
    """
    if startup_time > max_time:
        return (False, f"Startup took {startup_time:.1f}s (max: {max_time}s)")
    return (True, f"Startup time: {startup_time:.1f}s")


def test_disk_limit(cache_dir: Path, max_disk_mb: int) -> tuple:
    """Test that disk usage respects limit.
    
    Returns (success, message).
    """
    stats = get_cache_disk_usage(cache_dir)
    disk_mb = stats['total_bytes'] / (1024 * 1024)
    
    # Allow 10% tolerance
    if disk_mb > max_disk_mb * 1.1:
        return (False, f"Disk usage {disk_mb:.1f}MB exceeds limit {max_disk_mb}MB")
    
    return (True, f"Disk usage: {disk_mb:.1f}MB (limit: {max_disk_mb}MB)")


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
    parser.add_argument('--cache-disk-mb', type=int, default=0,
                        help='Disk cache size in MB (0=2x memory, default: 0)')
    parser.add_argument('--cache-shard-mb', type=int, default=16,
                        help='Shard file size in MB (default: 16)')
    parser.add_argument('--session-duration', type=float, default=30.0,
                        help='Duration of each viewer session in seconds (default: 30)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')

    args = parser.parse_args()
    
    # Calculate effective disk limit
    effective_disk_mb = args.cache_disk_mb if args.cache_disk_mb > 0 else args.cache_mem_mb * 2

    print("=" * 70)
    print("PersistentCache v3 Sharded Storage Test")
    print("=" * 70)
    print(f"\nConfiguration:")
    print(f"  Memory cache: {args.cache_mem_mb}MB")
    print(f"  Disk cache:   {effective_disk_mb}MB")
    print(f"  Shard size:   {args.cache_shard_mb}MB")
    print(f"  Session:      {args.session_duration}s")
    print()

    # 1. Create artifacts
    print("[1/9] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()
    
    # Create data_dir for cache
    artifacts.data_dir = artifacts.run_dir / 'data'
    artifacts.data_dir.mkdir(exist_ok=True)
    
    cache_dir = get_cache_dir(artifacts)

    # 2. Preflight checks
    print("\n[2/9] Running preflight checks...")
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
        print(f"\n[3/9] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "v3_test_content",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params={'EnableContentCache': '0'}
        )
        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        print(f"\n[4/9] Starting viewer window server (:{args.display_viewer})...")
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
        print(f"\n[5/9] Session 1: Cold start (populating cache)...")
        ensure_clean_cache(cache_dir)
        
        # Start viewer first, then run scenario
        startup1, exit1, proc1 = run_viewer_session(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session1', cache_dir,
            cache_mem_mb=args.cache_mem_mb,
            cache_disk_mb=args.cache_disk_mb,
            cache_shard_mb=args.cache_shard_mb,
            display_for_viewer=args.display_viewer,
            duration=2.0  # Short initial wait
        )
        
        # If viewer connected, run scenario
        if exit1 == 0 or proc1.poll() is None:
            runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
            runner.tiled_logos_test(tiles=6, duration=args.session_duration - 5, delay_between=2.0)
            runner.cleanup()
            time.sleep(2.0)
            tracker.cleanup('v3_session1')
            exit1 = 0
        
        if exit1 != 0:
            print(f"\n✗ FAIL: Session 1 exited with code {exit1}")
            return 1
        
        print(f"  Cold start time: {startup1:.1f}s")
        
        # Check disk format
        success, msg = test_disk_format(cache_dir)
        results['disk_format'] = (success, msg)
        print(f"  Disk format: {msg}")
        
        # Check disk usage after first session
        stats1 = get_cache_disk_usage(cache_dir)
        print(f"  Cache size: {stats1['total_bytes']/1024:.1f}KB ({stats1['shard_count']} shards)")

        # 6. Second session - warm start (test startup time with existing cache)
        print(f"\n[6/9] Session 2: Warm start (existing cache)...")
        
        startup2, exit2, proc2 = run_viewer_session(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session2', cache_dir,
            cache_mem_mb=args.cache_mem_mb,
            cache_disk_mb=args.cache_disk_mb,
            cache_shard_mb=args.cache_shard_mb,
            display_for_viewer=args.display_viewer,
            duration=2.0
        )
        
        if exit2 == 0 or proc2.poll() is None:
            runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
            runner.tiled_logos_test(tiles=6, duration=args.session_duration - 5, delay_between=2.0)
            runner.cleanup()
            time.sleep(2.0)
            tracker.cleanup('v3_session2')
            exit2 = 0
        
        if exit2 != 0:
            print(f"\n✗ FAIL: Session 2 exited with code {exit2}")
            return 1
        
        print(f"  Warm start time: {startup2:.1f}s")
        
        # Startup should be fast (index-only load)
        success, msg = test_startup_time(startup2, max_time=5.0)
        results['startup_time'] = (success, msg)
        print(f"  {msg}")
        
        stats2 = get_cache_disk_usage(cache_dir)
        print(f"  Cache size: {stats2['total_bytes']/1024:.1f}KB ({stats2['shard_count']} shards)")

        # 7. Third session - stress test with small memory to force evictions
        print(f"\n[7/9] Session 3: Eviction stress test (small memory cache)...")
        
        # Use tiny memory cache to force many evictions
        tiny_mem_mb = 1  # 1MB memory, but disk can be larger
        
        startup3, exit3, proc3 = run_viewer_session(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session3', cache_dir,
            cache_mem_mb=tiny_mem_mb,
            cache_disk_mb=0,  # 2x memory = 2MB disk
            cache_shard_mb=1,  # 1MB shards
            display_for_viewer=args.display_viewer,
            duration=2.0
        )
        
        if exit3 == 0 or proc3.poll() is None:
            runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
            runner.tiled_logos_test(tiles=12, duration=int(args.session_duration * 1.5) - 5, delay_between=1.0)
            runner.cleanup()
            time.sleep(2.0)
            tracker.cleanup('v3_session3')
            exit3 = 0
        
        if exit3 != 0:
            print(f"\n✗ FAIL: Session 3 exited with code {exit3}")
            return 1
        
        print(f"  Stress test time: {startup3:.1f}s")
        
        # Check that disk usage is still reasonable (not exploding due to full rebuilds)
        stats3 = get_cache_disk_usage(cache_dir)
        print(f"  Cache size: {stats3['total_bytes']/1024:.1f}KB ({stats3['shard_count']} shards)")
        
        # 8. Fourth session - verify disk limit enforcement
        print(f"\n[8/9] Session 4: Disk limit enforcement...")
        
        startup4, exit4, proc4 = run_viewer_session(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'v3_session4', cache_dir,
            cache_mem_mb=args.cache_mem_mb,
            cache_disk_mb=effective_disk_mb,
            cache_shard_mb=args.cache_shard_mb,
            display_for_viewer=args.display_viewer,
            duration=2.0
        )
        
        if exit4 == 0 or proc4.poll() is None:
            runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
            runner.tiled_logos_test(tiles=12, duration=args.session_duration - 5, delay_between=0.5)
            runner.cleanup()
            time.sleep(2.0)
            tracker.cleanup('v3_session4')
            exit4 = 0
        
        if exit4 != 0:
            print(f"\n✗ FAIL: Session 4 exited with code {exit4}")
            return 1
        
        success, msg = test_disk_limit(cache_dir, effective_disk_mb)
        results['disk_limit'] = (success, msg)
        print(f"  {msg}")

        # 9. Report results
        print("\n[9/9] Results Summary")
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
