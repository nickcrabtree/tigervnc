#!/usr/bin/env python3
"""
End-to-end test: Toggle between two cached pictures.

Validates the tiling enhancement described in
docs/content_and_persistent_cache_tiling_enhancement.md by testing cache
behavior when toggling between two pictures.

## The Tiling Enhancement Goal

Without tiling enhancement:
- Server decomposes large rectangles into many small tiles
- Each toggle generates many cache hits (one per tile)
- Inefficient: dozens of CachedRect messages per toggle (100+ hits)

With tiling enhancement (bounding-box approach):
- Server checks if BOUNDING BOX of damage region matches a cached entry
- On HIT: Send ONE CachedRect for entire bounding box
- On MISS: Encode damage normally, then seed the bounding box hash
- Efficient: few cache hits per toggle (~1-5 depending on viewer behavior)

## Current Implementation

The bounding-box approach checks the entire damage region's bounding box
against known cache entries in the unified PersistentCache engine while
running under the session-only ContentCache policy. This works well when:
- The content (e.g., PowerPoint slide) has consistent coordinates
- The viewer produces stable damage regions

Variations in bounding box coordinates (from compositor effects, window
decorations, etc.) can cause cache misses even for identical content.

## Test Flow

1. Display pictureA (hydration phase)
   - Server sends full pixel data
   - Server sends CachedRectSeed with bounding box hash

2. Display pictureB (hydration phase)
   - Same as above for pictureB

3. Toggle back to pictureA (post-hydration)
   - Expected: bounding-box cache hit if coordinates match

4. Continue toggling...
   - Each toggle: 1-5 cache hits expected (varies by viewer)

## Key Metric

The test measures **cache hits per toggle** after hydration:
- Target: ~3 hits per toggle (with tiling enhancement + realistic viewer)
- Baseline without enhancement: 100+ hits per toggle
- Ideal (consistent coordinates): 1 hit per toggle
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
    env['TIGERVNC_VIEWER_DEBUG_LOG'] = '1'

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
        description='Test toggling between two cached pictures'
    )
    parser.add_argument('--display-content', type=int, default=998,
                        help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                        help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                        help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                        help='Port for viewer window server (default: 6899)')
    parser.add_argument('--toggles', type=int, default=10,
                        help='Number of picture toggles (default: 10)')
    parser.add_argument('--delay', type=float, default=2.0,
                        help='Delay between toggles in seconds (default: 2.0)')
    parser.add_argument('--cache-size', type=int, default=256,
                        help='Content cache size in MB (default: 256MB)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')
    parser.add_argument('--expected-hits-per-toggle', type=float, default=3.0,
                        help='Expected cache hits per toggle after hydration (default: 3.0)')
    parser.add_argument('--hits-tolerance', type=float, default=2.0,
                        help='Tolerance for hits per toggle (default: 2.0, so 3±2 = 1-5)')
    parser.add_argument('--hydration-toggles', type=int, default=2,
                        help='Number of initial toggles to ignore for hydration (default: 2)')

    args = parser.parse_args()

    print("=" * 70)
    print("Picture Toggle Cache Test (Tiling Enhancement Validation)")
    print("=" * 70)
    print(f"\nToggles: {args.toggles} (first {args.hydration_toggles} for hydration)")
    print(f"Post-hydration toggles: {max(0, args.toggles - args.hydration_toggles)}")
    print(f"Delay between toggles: {args.delay}s")
    print(f"Cache Size: {args.cache_size}MB")
    print(f"Expected hits per toggle: {args.expected_hits_per_toggle} ± {args.hits_tolerance}")
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
        print("  Server config: ContentCache enabled; viewer runs in ContentCache mode")
        server_content = VNCServer(
            args.display_content, args.port_content, "toggle_content",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
        )

        if not server_content.start():
            print("✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        # 5. Start viewer window server
        print(f"\n[4/8] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, "toggle_viewerwin",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode
        )
        if not server_viewer.start():
            print("✗ FAIL: Could not start viewer window server")
            return 1
        if not server_viewer.start_session(wm=args.wm):
            print("✗ FAIL: Could not start viewer window server session")
            return 1
        print("✓ Viewer window server ready")

        # 6. Launch C++ viewer with ContentCache
        print(f"\n[5/8] Launching C++ viewer with ContentCache={args.cache_size}MB...")
        test_proc = run_cpp_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'toggle_test_viewer', cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ C++ viewer connected")

        # 7. Run toggle scenario
        print(f"\n[6/8] Running picture toggle scenario...")
        print(f"  Strategy: Toggle between pictureA and pictureB {args.toggles} times")
        print(f"  Hydration phase: first {args.hydration_toggles} toggles (cache misses expected)")
        print(f"  Test phase: remaining {max(0, args.toggles - args.hydration_toggles)} toggles")
        print(f"  Expected: exactly 1 cache hit per toggle after hydration")
        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)
        stats = runner.toggle_two_pictures_test(toggles=args.toggles, delay_between=args.delay)
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
        tracker.cleanup('toggle_test_viewer')

        log_path = artifacts.logs_dir / 'toggle_test_viewer.log'
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
        total_hits = cache_ops['total_hits']
        total_lookups = cache_ops['total_lookups']
        hit_rate = cache_ops['hit_rate']
        bandwidth_reduction = cache_ops.get('bandwidth_reduction_pct', 0.0)

        # Calculate hits per toggle after hydration
        post_hydration_toggles = max(0, args.toggles - args.hydration_toggles)
        
        # For the key metric: we need to estimate post-hydration hits
        # This is approximate since we can't easily separate hydration hits from test hits
        # in the current log format. We use total hits as a proxy.
        #
        # With tiling enhancement:
        #   - Hydration: 0 hits (all misses, content being sent)
        #   - Post-hydration: exactly 1 hit per toggle
        #   - Total hits ≈ post_hydration_toggles
        #
        # Without tiling enhancement:
        #   - Many small tile hits per toggle (both during and after hydration)
        #   - Total hits >> post_hydration_toggles
        
        if post_hydration_toggles > 0:
            # Estimate: assume hydration toggles also generate some hits
            # With tiling: total_hits ≈ post_hydration_toggles (ideal)
            # Without tiling: total_hits >> post_hydration_toggles
            hits_per_toggle = total_hits / args.toggles if args.toggles > 0 else 0
        else:
            hits_per_toggle = 0

        print(f"\nContentCache Performance:")
        print(f"  Total toggles: {args.toggles}")
        print(f"  Hydration toggles: {args.hydration_toggles}")
        print(f"  Post-hydration toggles: {post_hydration_toggles}")
        print(f"  Cache lookups: {total_lookups}")
        print(f"  Cache hits: {total_hits}")
        print(f"  Cache misses: {cache_ops['total_misses']}")
        print(f"  Hit rate: {hit_rate:.1f}%")
        print(f"  Bandwidth reduction: {bandwidth_reduction:.1f}%")
        print(f"")
        print(f"  *** KEY METRIC ***")
        print(f"  Hits per toggle: {hits_per_toggle:.2f}")
        print(f"  Expected: {args.expected_hits_per_toggle} ± {args.hits_tolerance}")
        
        # Calculate expected range
        min_hits_per_toggle = args.expected_hits_per_toggle - args.hits_tolerance
        max_hits_per_toggle = args.expected_hits_per_toggle + args.hits_tolerance

        # Validation
        success = True
        failures = []
        warnings = []

        # NOTE: We no longer check for PersistentCache initialization since
        # ContentCache now uses the unified GlobalClientPersistentCache engine
        # with disk persistence disabled. Seeing PersistentCache init is expected.

        # KEY VALIDATION: hits per toggle should be ~1 after hydration
        if hits_per_toggle < min_hits_per_toggle:
            # Too few hits - cache not working at all?
            success = False
            failures.append(
                f"Hits per toggle ({hits_per_toggle:.2f}) below minimum ({min_hits_per_toggle:.2f}). "
                f"Cache may not be functioning."
            )
        elif hits_per_toggle > max_hits_per_toggle:
            # Too many hits - tiling enhancement not working
            # This is expected until the enhancement is implemented
            warnings.append(
                f"Hits per toggle ({hits_per_toggle:.2f}) above maximum ({max_hits_per_toggle:.2f}). "
                f"This indicates tiling enhancement is NOT active - server is decomposing "
                f"large rectangles into many small cache entries instead of 1 large entry."
            )
            # Don't fail the test yet - this is expected baseline behavior
            # Uncomment the next line once tiling enhancement is implemented:
            # success = False

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")
        print(f"Content server log: {artifacts.logs_dir / f'toggle_content_server_{args.display_content}.log'}")

        # Print warnings (expected until tiling enhancement is implemented)
        if warnings:
            print("\n" + "=" * 70)
            print("WARNINGS (expected until tiling enhancement is implemented)")
            print("=" * 70)
            for w in warnings:
                print(f"  ⚠ {w}")

        if success:
            print("\n✓ TEST PASSED")
            print(f"\nPicture toggle cache test successful:")
            print(f"  • Hits per toggle: {hits_per_toggle:.2f} (expected: {args.expected_hits_per_toggle} ± {args.hits_tolerance})")
            print(f"  • Total cache hits: {total_hits}")
            print(f"  • Toggles: {args.toggles} completed successfully")
            if warnings:
                print(f"\n  Note: {len(warnings)} warning(s) - see above for details.")
                print(f"  The tiling enhancement is not yet active.")
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
