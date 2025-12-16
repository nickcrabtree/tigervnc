#!/usr/bin/env python3
"""
Regression test: Large rectangle caching strategies.

Validates that large rectangle caching logic is working:
- Bordered region detection and caching
- Bounding box caching for large rects
- Tiling subdivision for oversized content

This test catches regressions in:
- tryPersistentCacheLookup bordered logic
- Bounding box computation and caching
- Large rect subdivision thresholds
- Tiling mechanism

Test validates:
- Server logs contain "BORDERED:" or "TILING:" messages
- Cache hits occur for large content (not just small tiles)
- Hit rate > threshold for large-rect scenarios
"""

import sys
import time
import argparse
import subprocess
import os
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT, BUILD_DIR
)
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_viewer(viewer_path, port, artifacts, tracker, name,
               cache_size_mb=256, display_for_viewer=None, cache_path=None):
    """Run viewer with PersistentCache enabled (lossless + sandboxed cache)."""
    if cache_path is None:
        cache_path = artifacts.get_sandboxed_cache_dir() / 'large_rect_strategy'
        cache_path.mkdir(parents=True, exist_ok=True)

    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'AutoSelect=0',
        'PreferredEncoding=ZRLE',
        'NoJPEG=1',
        'PersistentCache=1',
        f'PersistentCacheSize={cache_size_mb}',
        f'PersistentCachePath={cache_path}',
    ]

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()
    env['TIGERVNC_VIEWER_DEBUG_LOG'] = '1'

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name}...")
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


def count_large_rect_messages(log_path):
    """Count bordered region, tiling, and bounding box messages."""
    bordered_count = 0
    tiling_count = 0
    bbox_count = 0
    
    with open(log_path, 'r', encoding='utf-8', errors='ignore') as f:
        for line in f:
            line_lower = line.lower()
            if 'bordered:' in line_lower:
                bordered_count += 1
            if 'tiling:' in line_lower:
                tiling_count += 1
            if 'bounding-box' in line_lower or 'bbox' in line_lower:
                bbox_count += 1
    
    return bordered_count, tiling_count, bbox_count


def main():
    parser = argparse.ArgumentParser(
        description='Test large rectangle caching strategies'
    )
    parser.add_argument('--display-content', type=int, default=998)
    parser.add_argument('--port-content', type=int, default=6898)
    parser.add_argument('--display-viewer', type=int, default=999)
    parser.add_argument('--port-viewer', type=int, default=6899)
    parser.add_argument('--duration', type=int, default=30,
                        help='Test duration (default: 30)')
    parser.add_argument('--cache-size', type=int, default=256)
    parser.add_argument('--wm', default='openbox')
    parser.add_argument('--verbose', action='store_true')
    parser.add_argument('--hit-rate-threshold', type=float, default=15.0,
                        help='Minimum cache hit rate (default: 15%%)')

    args = parser.parse_args()

    print("=" * 70)
    print("Large Rectangle Cache Strategy Test")
    print("=" * 70)
    print(f"Duration: {args.duration}s")
    print(f"Hit rate threshold: {args.hit_rate_threshold}%")
    print()

    # 1. Artifacts
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight
    print("[1/6] Running preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed\n{e}")
        return 1

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

    print("✓ Preflight checks passed")

    tracker = ProcessTracker()

    # Determine server mode
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'

    try:
        # 3. Start servers
        print(f"\n[2/6] Starting servers...")
        server_content = VNCServer(
            args.display_content, args.port_content, 'large_rect_content',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:100',
            server_choice=server_mode,
            server_params={'EnablePersistentCache': '1'}
        )
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server")
            return 1

        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, 'large_rect_viewerwin',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:30',
            server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server")
            return 1

        # 4. Launch viewer
        print(f"\n[3/6] Launching viewer with PersistentCache...")
        viewer = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'large_rect_viewer',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )

        # 5. Run large content scenario
        print(f"\n[4/6] Running large-content scenarios...")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        
        # Deterministic large-rect workload using repo-provided images.
        # Toggle between two large pictures fullscreen; after initial hydration
        # we should see persistent cache hits on repeats.
        toggles = max(8, min(20, int(args.duration / 2)))
        delay_between = 1.0
        print(f"  Running toggle_two_pictures_test (toggles={toggles}, delay={delay_between}s)...")
        stats = runner.toggle_two_pictures_test(toggles=toggles, delay_between=delay_between)
        print(f"  Toggle completed: {stats}")
        time.sleep(3.0)

        tracker.cleanup('large_rect_viewer')
        time.sleep(1.0)

        # 6. Analysis
        print("\n[5/6] Analyzing results...")

        viewer_log = artifacts.logs_dir / 'large_rect_viewer.log'
        server_log = artifacts.logs_dir / f'large_rect_content_server_{args.display_content}.log'

        # Parse logs
        viewer_parsed = parse_cpp_log(viewer_log)
        server_parsed = parse_server_log(server_log, verbose=args.verbose)

        # Count large rect strategy messages
        bordered, tiling, bbox = count_large_rect_messages(server_log)

        # Compute hit rate
        init_count = 0
        with open(viewer_log, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                if "Received PersistentCachedRectInit" in line:
                    init_count += 1
        hits = viewer_parsed.persistent_hits
        lookups = hits + init_count
        hit_rate = (100.0 * hits / lookups) if lookups > 0 else 0.0

        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        print(f"\nCache Performance:")
        print(f"  Hit rate: {hit_rate:.1f}%")
        print(f"  Lookups: {lookups}")
        print(f"  Hits: {hits}")

        print(f"\nLarge Rectangle Strategy Messages:")
        print(f"  BORDERED: {bordered}")
        print(f"  TILING: {tiling}")
        print(f"  Bounding-box: {bbox}")

        # Validation
        success = True
        failures = []

        # CRITICAL: At least one large-rect strategy must be exercised
        if bordered == 0 and tiling == 0 and bbox == 0:
            success = False
            failures.append(
                "No large rectangle strategy messages found "
                "(expected BORDERED, TILING, or bbox messages - large rect logic not exercised)"
            )

        # Hit rate should be reasonable for repeated large content
        if hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(
                f"Hit rate {hit_rate:.1f}% < {args.hit_rate_threshold}% threshold"
            )

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer: {viewer_log}")
        print(f"Server: {server_log}")

        if success:
            print("\n" + "=" * 70)
            print("✓ TEST PASSED")
            print("=" * 70)
            print("\nLarge rectangle caching strategies validated:")
            print(f"  • BORDERED messages: {bordered}")
            print(f"  • TILING messages: {tiling}")
            print(f"  • Bounding-box messages: {bbox}")
            print(f"  • Hit rate: {hit_rate:.1f}% (threshold: {args.hit_rate_threshold}%)")
            return 0
        else:
            print("\n" + "=" * 70)
            print("✗ TEST FAILED")
            print("=" * 70)
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
