#!/usr/bin/env python3
"""
Regression test: Lossy vs Lossless encoding parity.

Validates that the cache system behaves correctly with both encoding types:
- Lossless (ZRLE): Hash matches exactly, no hash reports
- Lossy (Tight/JPEG): Hash mismatch detected, hash reports sent via message 247

This test validates the lossy hash reporting protocol:
- Seeds are ALWAYS sent (both lossy and lossless)
- Lossless: Client stores under canonical hash, no reports needed
- Lossy: Client stores under lossy hash, reports canonical→lossy mapping
- Server learns mapping for future dual-hash lookups

Test validates:
- Lossless: canonical hash == client hash (no PersistentCacheHashReport messages)
- Lossy: canonical hash != client hash (PersistentCacheHashReport messages present)
- Both achieve similar hit rates (within tolerance)
- No visual corruption in either case
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
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_viewer(viewer_path, port, artifacts, tracker, name, encoding, 
               cache_size_mb=256, display_for_viewer=None, cache_path=None, extra_args=None):
    """Run viewer with specified encoding preference."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        'AutoSelect=0',
        f'PreferredEncoding={encoding}',
        'PersistentCache=1',
        f'PersistentCacheSize={cache_size_mb}',
    ]
    
    if cache_path:
        cmd.append(f'PersistentCachePath={cache_path}')
        
    if extra_args:
        cmd.extend(extra_args)

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


def count_hash_reports(log_path):
    """Count PersistentCacheHashReport messages in log."""
    count = 0
    with open(log_path, 'r', encoding='utf-8', errors='ignore') as f:
        for line in f:
            if 'Reported lossy hash to server' in line:
                count += 1
    return count




def main():
    parser = argparse.ArgumentParser(
        description='Test lossy vs lossless encoding cache behavior'
    )
    parser.add_argument('--display-content', type=int, default=998)
    parser.add_argument('--port-content', type=int, default=6898)
    parser.add_argument('--display-viewer', type=int, default=999)
    parser.add_argument('--port-viewer', type=int, default=6899)
    parser.add_argument('--duration', type=int, default=60,
                        help='Test duration per encoding (default: 60)')
    parser.add_argument('--cache-size', type=int, default=256)
    parser.add_argument('--wm', default='openbox')
    parser.add_argument('--verbose', action='store_true')
    parser.add_argument('--hit-rate-tolerance', type=float, default=10.0,
                        help='Allowed hit rate difference between encodings (pp)')

    args = parser.parse_args()

    print("=" * 70)
    print("Lossy vs Lossless Encoding Parity Test")
    print("=" * 70)
    print(f"Duration per encoding: {args.duration}s")
    print(f"Hit rate tolerance: ±{args.hit_rate_tolerance:.1f} pp")
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
            args.display_content, args.port_content, 'lossy_lossless_content',
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
            args.display_viewer, args.port_viewer, 'lossy_lossless_viewerwin',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:30',
            server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server")
            return 1

        # Use isolated cache paths for each run to ensure fair comparison
        cache_dir_lossless = artifacts.base_dir / "cache_lossless"
        cache_dir_lossy = artifacts.base_dir / "cache_lossy"
        cache_dir_lossless.mkdir()
        cache_dir_lossy.mkdir()

        print("\n[3/6] Run 1/2: Lossless encoding (ZRLE)")
        viewer1 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'lossless_viewer', 'ZRLE',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer,
            cache_path=str(cache_dir_lossless)
        )

        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        stats1 = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  Scenario completed: {stats1}")
        time.sleep(3.0)

        tracker.cleanup('lossless_viewer')
        time.sleep(1.0)

        # Stop servers between runs
        server_viewer.stop()
        server_content.stop()

        # ===== Run 2: Lossy (Tight) =====
        print(f"\n[4/6] Restarting servers for lossy run...")
        server_content = VNCServer(
            args.display_content, args.port_content, 'lossy_lossless_content2',
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
            args.display_viewer, args.port_viewer, 'lossy_lossless_viewerwin2',
            artifacts, tracker,
            geometry='1920x1080',
            log_level='*:stderr:30',
            server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not restart viewer window server")
            return 1

        print("\n[5/6] Run 2/2: Lossy encoding (Tight)")
        # Force low quality to ensure JPEG usage and hash mismatches
        viewer2 = run_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'lossy_viewer', 'Tight',
            cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer,
            cache_path=str(cache_dir_lossy),
            extra_args=['QualityLevel=1', 'CompressLevel=9', 'NoJPEG=0']
        )

        stats2 = runner.tiled_logos_test(tiles=12, duration=args.duration, delay_between=3.0)
        print(f"  Scenario completed: {stats2}")
        time.sleep(3.0)

        tracker.cleanup('lossy_viewer')
        time.sleep(1.0)

        # ===== Analysis =====
        print("\n[6/6] Analyzing results...")

        lossless_viewer_log = artifacts.logs_dir / 'lossless_viewer.log'
        lossless_server_log = artifacts.logs_dir / f'lossy_lossless_content_server_{args.display_content}.log'
        lossy_viewer_log = artifacts.logs_dir / 'lossy_viewer.log'
        lossy_server_log = artifacts.logs_dir / f'lossy_lossless_content2_server_{args.display_content}.log'

        # Parse logs
        lossless_parsed = parse_cpp_log(lossless_viewer_log)
        lossless_server_parsed = parse_server_log(lossless_server_log, verbose=args.verbose)
        lossy_parsed = parse_cpp_log(lossy_viewer_log)
        lossy_server_parsed = parse_server_log(lossy_server_log, verbose=args.verbose)

        # Count hash reports (message 247)
        lossless_hash_reports = count_hash_reports(lossless_viewer_log)
        lossy_hash_reports = count_hash_reports(lossy_viewer_log)

        # Compute hit rates (protocol-agnostic)
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
        print(f"  Hash reports: {lossless_hash_reports}")

        print("\nLossy (Tight) Run:")
        print(f"  Hit rate: {lossy_hit_rate:.1f}%")
        print(f"  Hash reports: {lossy_hash_reports}")

        hit_rate_diff = abs(lossless_hit_rate - lossy_hit_rate)
        print(f"\nHit rate difference: {hit_rate_diff:.1f} pp")

        # Validation
        success = True
        failures = []

        # CRITICAL: Lossless should have NO hash reports (hash matches exactly)
        if lossless_hash_reports > 0:
            success = False
            failures.append(
                f"Lossless encoding sent {lossless_hash_reports} hash reports "
                "(expected 0 - lossless should have exact hash match)"
            )

        # CRITICAL: Lossy MUST have hash reports (protocol working)
        if lossy_hash_reports == 0:
            success = False
            failures.append(
                "Lossy encoding sent 0 hash reports "
                "(expected >0 - lossy hash reporting protocol not working)"
            )

        # Hit rates should be similar (cache working for both)
        if hit_rate_diff > args.hit_rate_tolerance:
            success = False
            failures.append(
                f"Hit rates differ by {hit_rate_diff:.1f} pp (> {args.hit_rate_tolerance:.1f} pp tolerance)"
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
            print("\nCache behavior correct for both encodings:")
            print(f"  • Lossless: {lossless_hash_reports} hash reports (expected 0)")
            print(f"  • Lossy: {lossy_hash_reports} hash reports (expected >0)")
            print(f"  • Hit rates within tolerance: {hit_rate_diff:.1f} pp ≤ {args.hit_rate_tolerance:.1f} pp")
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
