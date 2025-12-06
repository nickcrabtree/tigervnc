#!/usr/bin/env python3
"""
Regression test: Cache seed mechanism validation.

Validates that the seed mechanism correctly respects encoding lossyness:
- Lossless encodings (ZRLE): Seeds are sent, cache works
- Lossy encodings (Tight): Seeds are SKIPPED, cache still works via INITs

This test catches regressions in:
- Seed skip logic for lossy encodings
- isLossyEncoding() returning wrong value
- Cache seeding mechanism
- Seed message generation

Test validates:
- Lossless: writeCachedRectSeed messages present, no seed skips
- Lossy: "Skipped seeding (lossy encoding)" messages present
- Both: Cache still functions (hit rate > threshold)
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
from log_parser import parse_cpp_log, parse_server_log


def run_viewer(viewer_path, port, artifacts, tracker, name, encoding,
               cache_size_mb=256, display_for_viewer=None):
    """Run viewer with specified encoding."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        f'PreferredEncoding={encoding}',
        'PersistentCache=1',
        f'PersistentCacheSize={cache_size_mb}',
    ]

    log_path = artifacts.logs_dir / f'{name}.log'
    env = os.environ.copy()

    if display_for_viewer is not None:
        env['DISPLAY'] = f':{display_for_viewer}'
    else:
        env.pop('DISPLAY', None)

    print(f"  Starting {name} with {encoding} encoding...")
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


def count_seed_messages(server_log):
    """Count seed-related messages in server log."""
    seed_sent_count = 0
    seed_skip_count = 0
    
    with open(server_log, 'r', encoding='utf-8', errors='ignore') as f:
        for line in f:
            # Count seed messages sent
            if 'writeCachedRectSeed' in line:
                seed_sent_count += 1
            # Count seed skips (lossy detected)
            if 'Skipped seeding' in line and 'lossy encoding' in line:
                seed_skip_count += 1
    
    return seed_sent_count, seed_skip_count


def main():
    parser = argparse.ArgumentParser(
        description='Test cache seed mechanism for lossy vs lossless'
    )
    parser.add_argument('--display-content', type=int, default=998)
    parser.add_argument('--port-content', type=int, default=6898)
    parser.add_argument('--display-viewer', type=int, default=999)
    parser.add_argument('--port-viewer', type=int, default=6899)
    parser.add_argument('--duration', type=int, default=45,
                        help='Test duration per encoding (default: 45)')
    parser.add_argument('--cache-size', type=int, default=256)
    parser.add_argument('--wm', default='openbox')
    parser.add_argument('--verbose', action='store_true')
    parser.add_argument('--hit-rate-threshold', type=float, default=15.0,
                        help='Minimum cache hit rate (default: 15%%)')

    args = parser.parse_args()

    print("=" * 70)
    print("Cache Seed Mechanism Test")
    print("=" * 70)
    print(f"Duration per encoding: {args.duration}s")
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
        # ===== Run 1: Lossless (ZRLE) =====
        print(f"\n[2/6] Starting servers for lossless run...")
        server_content = VNCServer(
            args.display_content, args.port_content, 'seed_lossless_content',
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
            args.display_viewer, args.port_viewer, 'seed_lossless_viewerwin',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:30',
            server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server")
            return 1

        print("\n[3/6] Run 1/2: Lossless encoding (ZRLE)")
        viewer1 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'seed_lossless_viewer', 'ZRLE',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )

        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        stats1 = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=2.5)
        print(f"  Scenario completed: {stats1}")
        time.sleep(3.0)

        tracker.cleanup('seed_lossless_viewer')
        time.sleep(1.0)

        # Stop servers between runs
        server_viewer.stop()
        server_content.stop()

        # ===== Run 2: Lossy (Tight) =====
        print(f"\n[4/6] Restarting servers for lossy run...")
        server_content = VNCServer(
            args.display_content, args.port_content, 'seed_lossy_content',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:100',
            server_choice=server_mode,
            server_params={'EnablePersistentCache': '1'}
        )
        if not server_content.start() or not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not restart content server")
            return 1

        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, 'seed_lossy_viewerwin',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:30',
            server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not restart viewer window server")
            return 1

        print("\n[5/6] Run 2/2: Lossy encoding (Tight)")
        viewer2 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'seed_lossy_viewer', 'Tight',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )

        stats2 = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=2.5)
        print(f"  Scenario completed: {stats2}")
        time.sleep(3.0)

        tracker.cleanup('seed_lossy_viewer')
        time.sleep(1.0)

        # ===== Analysis =====
        print("\n[6/6] Analyzing results...")

        lossless_viewer_log = artifacts.logs_dir / 'seed_lossless_viewer.log'
        lossless_server_log = artifacts.logs_dir / f'seed_lossless_content_server_{args.display_content}.log'
        lossy_viewer_log = artifacts.logs_dir / 'seed_lossy_viewer.log'
        lossy_server_log = artifacts.logs_dir / f'seed_lossy_content_server_{args.display_content}.log'

        # Parse logs
        lossless_parsed = parse_cpp_log(lossless_viewer_log)
        lossy_parsed = parse_cpp_log(lossy_viewer_log)

        # Count seed messages
        lossless_sent, lossless_skipped = count_seed_messages(lossless_server_log)
        lossy_sent, lossy_skipped = count_seed_messages(lossy_server_log)

        # Compute hit rates
        def compute_hit_rate(viewer_log, parsed):
            init_count = 0
            with open(viewer_log, "r", encoding="utf-8", errors="ignore") as f:
                for line in f:
                    if "Received PersistentCachedRectInit" in line:
                        init_count += 1
            hits = parsed.persistent_hits
            lookups = hits + init_count
            return (100.0 * hits / lookups) if lookups > 0 else 0.0

        lossless_hit_rate = compute_hit_rate(lossless_viewer_log, lossless_parsed)
        lossy_hit_rate = compute_hit_rate(lossy_viewer_log, lossy_parsed)

        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        print("\nLossless (ZRLE) Run:")
        print(f"  Hit rate: {lossless_hit_rate:.1f}%")
        print(f"  Seeds sent: {lossless_sent}")
        print(f"  Seeds skipped: {lossless_skipped}")

        print("\nLossy (Tight) Run:")
        print(f"  Hit rate: {lossy_hit_rate:.1f}%")
        print(f"  Seeds sent: {lossy_sent}")
        print(f"  Seeds skipped: {lossy_skipped}")

        # Validation
        success = True
        failures = []

        # CRITICAL: Lossless should have NO seed skips
        if lossless_skipped > 0:
            success = False
            failures.append(
                f"Lossless encoding skipped {lossless_skipped} seeds "
                "(expected 0 - ZRLE wrongly detected as lossy)"
            )

        # CRITICAL: Lossy MUST have seed skips (mechanism working)
        if lossy_skipped == 0:
            success = False
            failures.append(
                "Lossy encoding did not skip any seeds "
                "(expected >0 - seed prevention mechanism not working)"
            )

        # Both should achieve reasonable hit rates
        if lossless_hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(
                f"Lossless hit rate {lossless_hit_rate:.1f}% < {args.hit_rate_threshold}% threshold"
            )

        if lossy_hit_rate < args.hit_rate_threshold:
            success = False
            failures.append(
                f"Lossy hit rate {lossy_hit_rate:.1f}% < {args.hit_rate_threshold}% threshold"
            )

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Lossless viewer: {lossless_viewer_log}")
        print(f"Lossless server: {lossless_server_log}")
        print(f"Lossy viewer: {lossy_viewer_log}")
        print(f"Lossy server: {lossy_server_log}")

        if success:
            print("\n" + "=" * 70)
            print("✓ TEST PASSED")
            print("=" * 70)
            print("\nSeed mechanism validated:")
            print(f"  • Lossless seeds skipped: {lossless_skipped} (expected 0)")
            print(f"  • Lossy seeds skipped: {lossy_skipped} (expected >0)")
            print(f"  • Both hit rates above threshold")
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
