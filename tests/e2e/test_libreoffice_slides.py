#!/usr/bin/env python3
"""
End-to-end test: LibreOffice Impress slide navigation (editing mode).

Tests the tiling enhancement with a real-world application: LibreOffice Impress
opening a PowerPoint presentation in normal editing mode. The user navigates
between slides using Page Up/Page Down keys.

## Why This Test Matters

This tests a realistic scenario where:
- A large central slide area changes when navigating
- Peripheral UI elements (slide sorter, toolbars) remain mostly static
- The slide content itself may repeat when navigating back to previous slides

The bounding-box cache should:
- Recognize when returning to a previously-seen slide
- Send a single CachedRect for the slide area instead of many small tiles

## Test Flow

1. Start LibreOffice Impress with a PowerPoint file in editing mode
2. Wait for initial rendering (hydration phase)
3. Navigate forward through slides (Page Down)
4. Navigate backward through slides (Page Up) - expect cache hits
5. Repeat navigation cycles

## Key Metric

Cache hits per slide transition after hydration:
- With tiling enhancement: few hits per transition (ideally 1 for slide area)
- Without enhancement: many small tile hits per transition
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
from log_parser import parse_cpp_log, compute_metrics


# Default presentation file
DEFAULT_PRESENTATION = "/ncloud/Nick/PMO/UK/Catcher/CGG 4D Processing/05 Presentations from CGG/2019 N-S baseline repro/001_20190812_4dcatch_lcf.pptx"


def run_cpp_viewer(viewer_path, port, artifacts, tracker, name,
                   cache_size_mb=256, display_for_viewer=None,
                   enable_persistent_cache=True):
    """Run C++ viewer with ContentCache and PersistentCache enabled."""
    cmd = [
        viewer_path,
        f'127.0.0.1::{port}',
        'Shared=1',
        'Log=*:stderr:100',
        f'ContentCacheSize={cache_size_mb}',
        f'PersistentCache={1 if enable_persistent_cache else 0}',
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


def start_libreoffice_impress(presentation_path: str, display: int, tracker: ProcessTracker) -> subprocess.Popen:
    """
    Start LibreOffice Impress in editing mode with the given presentation.
    
    Returns the process handle.
    """
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    # Start LibreOffice Impress in normal editing mode (not presentation mode)
    # --impress opens Impress specifically
    # --nofirststartwizard skips the welcome wizard
    # --nologo skips the splash screen
    # --norestore disables the "recover unsaved documents" dialog
    cmd = [
        'libreoffice',
        '--impress',
        '--nofirststartwizard',
        '--nologo',
        '--norestore',
        str(presentation_path)
    ]
    
    proc = subprocess.Popen(
        cmd,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        preexec_fn=os.setpgrp
    )
    
    tracker.register('libreoffice_impress', proc)
    return proc


def send_key(display: int, key: str, count: int = 1, delay: float = 0.5):
    """
    Send a key press using xdotool.
    
    Args:
        display: X display number
        key: Key name (e.g., 'Page_Down', 'Page_Up', 'Return')
        count: Number of times to press the key
        delay: Delay after each key press
    """
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    for _ in range(count):
        subprocess.run(
            ['xdotool', 'key', key],
            env=env,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL
        )
        time.sleep(delay)


def wait_for_window(display: int, window_name_patterns: list, timeout: float = 30.0, verbose: bool = False) -> bool:
    """
    Wait for a window matching any of the patterns to appear.
    
    Returns True if found within timeout, False otherwise.
    """
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    if isinstance(window_name_patterns, str):
        window_name_patterns = [window_name_patterns]
    
    start = time.time()
    last_debug_time = 0
    while time.time() - start < timeout:
        for pattern in window_name_patterns:
            result = subprocess.run(
                ['xdotool', 'search', '--name', pattern],
                env=env,
                capture_output=True,
                text=True
            )
            if result.returncode == 0 and result.stdout.strip():
                if verbose:
                    print(f"    Found window matching '{pattern}'")
                return True
        
        # Debug: periodically show what windows exist
        if verbose and time.time() - last_debug_time > 5.0:
            list_result = subprocess.run(
                ['wmctrl', '-l'],
                env=env,
                capture_output=True,
                text=True
            )
            if list_result.returncode == 0:
                print(f"    Current windows: {list_result.stdout.strip()[:200]}")
            last_debug_time = time.time()
        
        time.sleep(0.5)
    
    return False


def focus_window(display: int, window_name_pattern: str) -> bool:
    """
    Focus a window matching the pattern.
    
    Returns True if successful.
    """
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    # Find the window
    result = subprocess.run(
        ['xdotool', 'search', '--name', window_name_pattern],
        env=env,
        capture_output=True,
        text=True
    )
    
    if result.returncode != 0 or not result.stdout.strip():
        return False
    
    window_id = result.stdout.strip().split('\n')[0]
    
    # Focus and activate the window
    subprocess.run(
        ['xdotool', 'windowactivate', '--sync', window_id],
        env=env,
        check=False
    )
    
    time.sleep(0.3)
    return True


def send_key(display: int, key: str):
    """
    Send a key press to the currently focused window.
    
    Args:
        display: X display number
        key: Key name (e.g., 'Page_Down', 'Page_Up', 'Escape')
    """
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    subprocess.run(
        ['xdotool', 'key', key],
        env=env,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL
    )


def navigate_slides(display: int, num_slides: int, delay_between: float, verbose: bool = False):
    """
    Navigate through slides using Page Down/Up keyboard navigation.
    
    This is more reliable than clicking on slide thumbnails since the
    Slide Pane may not be visible.
    
    Args:
        display: X display number
        num_slides: Number of slides to navigate (forward then backward)
        delay_between: Delay between slide transitions
        verbose: Print progress
    """
    transitions = 0
    
    # Navigate forward using Page Down
    if verbose:
        print(f"  Navigating forward through {num_slides} slides (Page Down)...")
    for i in range(num_slides):
        if verbose:
            print(f"    Slide transition {transitions + 1}: Page Down")
        send_key(display, 'Page_Down')
        time.sleep(delay_between)
        transitions += 1
    
    # Navigate backward using Page Up
    if verbose:
        print(f"  Navigating backward through {num_slides} slides (Page Up)...")
    for i in range(num_slides):
        if verbose:
            print(f"    Slide transition {transitions + 1}: Page Up")
        send_key(display, 'Page_Up')
        time.sleep(delay_between)
        transitions += 1
    
    return transitions


def main():
    parser = argparse.ArgumentParser(
        description='Test LibreOffice Impress slide navigation caching'
    )
    parser.add_argument('--presentation', type=str, default=DEFAULT_PRESENTATION,
                        help=f'Path to presentation file (default: {DEFAULT_PRESENTATION})')
    parser.add_argument('--display-content', type=int, default=998,
                        help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                        help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                        help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                        help='Port for viewer window server (default: 6899)')
    parser.add_argument('--slides', type=int, default=5,
                        help='Number of slides to navigate (default: 5)')
    parser.add_argument('--cycles', type=int, default=3,
                        help='Number of forward/backward navigation cycles (default: 3)')
    parser.add_argument('--delay', type=float, default=2.0,
                        help='Delay between slide transitions in seconds (default: 2.0)')
    parser.add_argument('--startup-wait', type=float, default=60.0,
                        help='Time to wait for LibreOffice to start (default: 60.0)')
    parser.add_argument('--cache-size', type=int, default=256,
                        help='Content cache size in MB (default: 256MB)')
    parser.add_argument('--wm', default='openbox',
                        help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                        help='Verbose output')
    parser.add_argument('--expected-hits-per-transition', type=float, default=2.0,
                        help='Expected cache hits per slide transition (default: 2.0)')
    parser.add_argument('--hits-tolerance', type=float, default=1.0,
                        help='Tolerance for hits per transition (default: 1.0)')
    parser.add_argument('--hydration-cycles', type=int, default=1,
                        help='Number of initial cycles to ignore for hydration (default: 1)')

    args = parser.parse_args()

    # Validate presentation file exists
    presentation_path = Path(args.presentation)
    if not presentation_path.exists():
        print(f"✗ FAIL: Presentation file not found: {presentation_path}")
        return 1

    print("=" * 70)
    print("LibreOffice Impress Slide Navigation Cache Test")
    print("=" * 70)
    print(f"\nPresentation: {presentation_path.name}")
    print(f"Slides per cycle: {args.slides}")
    print(f"Navigation cycles: {args.cycles} (first {args.hydration_cycles} for hydration)")
    print(f"Delay between transitions: {args.delay}s")
    print(f"Cache Size: {args.cache_size}MB")
    print(f"Expected hits per transition: {args.expected_hits_per_transition} ± {args.hits_tolerance}")
    print()

    # 1. Create artifacts
    print("[1/9] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    # 2. Preflight checks
    print("\n[2/9] Running preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1

    # Check for xdotool
    xdotool_path = subprocess.run(['which', 'xdotool'], capture_output=True, text=True)
    if xdotool_path.returncode != 0:
        print("✗ FAIL: xdotool not found (required for keyboard simulation)")
        return 1
    print("✓ Found xdotool")

    # Check for libreoffice
    lo_path = subprocess.run(['which', 'libreoffice'], capture_output=True, text=True)
    if lo_path.returncode != 0:
        print("✗ FAIL: libreoffice not found")
        return 1
    print("✓ Found libreoffice")

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
        # 4. Start content server
        print(f"\n[3/9] Starting content server (:{args.display_content})...")
        server_content = VNCServer(
            args.display_content, args.port_content, "lo_slides_content",
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
        print(f"\n[4/9] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer, args.port_viewer, "lo_slides_viewerwin",
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

        # 6. Launch C++ viewer
        print(f"\n[5/9] Launching C++ viewer with ContentCache={args.cache_size}MB...")
        test_proc = run_cpp_viewer(
            binaries['cpp_viewer'], args.port_content, artifacts, tracker,
            'lo_slides_viewer', cache_size_mb=args.cache_size,
            display_for_viewer=args.display_viewer
        )
        if test_proc.poll() is not None:
            print("✗ FAIL: Test viewer exited prematurely")
            return 1
        print("✓ C++ viewer connected")

        # 7. Start LibreOffice Impress
        print(f"\n[6/9] Starting LibreOffice Impress...")
        print(f"  Opening: {presentation_path.name}")
        lo_proc = start_libreoffice_impress(presentation_path, args.display_content, tracker)
        
        # Wait for LibreOffice window to appear
        print(f"  Waiting for LibreOffice to start (up to {args.startup_wait}s)...")
        # Look for window with presentation name or "Impress" or "LibreOffice"
        # LibreOffice window titles vary: could be filename, "Impress", "LibreOffice Impress"
        window_patterns = [
            presentation_path.stem,  # Filename without extension
            presentation_path.name,  # Full filename  
            "Impress",
            "LibreOffice",
            "soffice",  # Sometimes the window has internal name
        ]
        if not wait_for_window(args.display_content, window_patterns, 
                               timeout=args.startup_wait, verbose=args.verbose):
            # Show what windows exist for debugging
            env = os.environ.copy()
            env['DISPLAY'] = f':{args.display_content}'
            list_result = subprocess.run(
                ['wmctrl', '-l'],
                env=env,
                capture_output=True,
                text=True
            )
            print(f"  Windows on display: {list_result.stdout.strip() if list_result.returncode == 0 else 'none'}")
            print("✗ FAIL: LibreOffice Impress window did not appear")
            return 1
        
        print("✓ LibreOffice Impress started")
        
        # Give it a moment to fully render
        time.sleep(3.0)
        
        # Focus the window - try several patterns
        focused = False
        for pattern in window_patterns:
            if focus_window(args.display_content, pattern):
                focused = True
                break
        
        if focused:
            print("✓ LibreOffice window focused")
        else:
            print("  (Warning: could not focus window, continuing anyway)")

        # 8. Navigate through slides
        print(f"\n[7/9] Running slide navigation scenario...")
        print(f"  Strategy: Navigate {args.slides} slides forward, then {args.slides} slides back")
        print(f"  Cycles: {args.cycles} (first {args.hydration_cycles} for cache hydration)")
        
        total_transitions = 0
        for cycle in range(args.cycles):
            cycle_type = "hydration" if cycle < args.hydration_cycles else "test"
            print(f"\n  Cycle {cycle + 1}/{args.cycles} ({cycle_type}):")
            transitions = navigate_slides(
                args.display_content,
                args.slides,
                args.delay,
                verbose=args.verbose
            )
            total_transitions += transitions
            print(f"    Completed {transitions} transitions")
        
        print(f"\n  Total transitions: {total_transitions}")
        time.sleep(3.0)

        # Check if viewer is still running
        if test_proc.poll() is not None:
            exit_code = test_proc.returncode
            print(f"\n✗ FAIL: Viewer exited during scenario (exit code: {exit_code})")
            return 1

        # 9. Stop and analyze
        print("\n[8/9] Stopping viewer and analyzing results...")
        tracker.cleanup('lo_slides_viewer')

        log_path = artifacts.logs_dir / 'lo_slides_viewer.log'
        if not log_path.exists():
            print(f"\n✗ FAIL: Log file not found: {log_path}")
            return 1

        parsed = parse_cpp_log(log_path)
        metrics = compute_metrics(parsed)

        print("\n[9/9] Verification...")
        print("\n" + "=" * 70)
        print("TEST RESULTS")
        print("=" * 70)

        cache_ops = metrics['cache_operations']
        total_hits = cache_ops['total_hits']
        total_lookups = cache_ops['total_lookups']
        hit_rate = cache_ops['hit_rate']
        bandwidth_reduction = cache_ops.get('bandwidth_reduction_pct', 0.0)

        # Calculate transitions per cycle and post-hydration transitions
        transitions_per_cycle = args.slides * 2  # forward + backward
        hydration_transitions = args.hydration_cycles * transitions_per_cycle
        post_hydration_transitions = total_transitions - hydration_transitions

        # Calculate hits per transition
        if total_transitions > 0:
            hits_per_transition = total_hits / total_transitions
        else:
            hits_per_transition = 0

        print(f"\nLibreOffice Impress Cache Performance:")
        print(f"  Presentation: {presentation_path.name}")
        print(f"  Slides navigated per cycle: {args.slides}")
        print(f"  Total navigation cycles: {args.cycles}")
        print(f"  Hydration cycles: {args.hydration_cycles}")
        print(f"  Total slide transitions: {total_transitions}")
        print(f"  Post-hydration transitions: {post_hydration_transitions}")
        print(f"  Cache lookups: {total_lookups}")
        print(f"  Cache hits: {total_hits}")
        print(f"  Cache misses: {cache_ops['total_misses']}")
        print(f"  Hit rate: {hit_rate:.1f}%")
        print(f"  Bandwidth reduction: {bandwidth_reduction:.1f}%")
        print(f"")
        print(f"  *** KEY METRIC ***")
        print(f"  Hits per transition: {hits_per_transition:.2f}")
        print(f"  Expected: {args.expected_hits_per_transition} ± {args.hits_tolerance}")

        # Calculate expected range
        min_hits = args.expected_hits_per_transition - args.hits_tolerance
        max_hits = args.expected_hits_per_transition + args.hits_tolerance

        # Validation
        success = True
        failures = []
        warnings = []

        if hits_per_transition < min_hits:
            success = False
            failures.append(
                f"Hits per transition ({hits_per_transition:.2f}) below minimum ({min_hits:.2f}). "
                f"Cache may not be functioning."
            )
        elif hits_per_transition > max_hits:
            warnings.append(
                f"Hits per transition ({hits_per_transition:.2f}) above maximum ({max_hits:.2f}). "
                f"Tiling enhancement may need tuning for LibreOffice damage patterns."
            )

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"Logs: {artifacts.logs_dir}")
        print(f"Viewer log: {log_path}")
        print(f"Content server log: {artifacts.logs_dir / f'lo_slides_content_server_{args.display_content}.log'}")

        if warnings:
            print("\n" + "=" * 70)
            print("WARNINGS")
            print("=" * 70)
            for w in warnings:
                print(f"  ⚠ {w}")

        if success:
            print("\n✓ TEST PASSED")
            print(f"\nLibreOffice slide navigation cache test successful:")
            print(f"  • Hits per transition: {hits_per_transition:.2f}")
            print(f"  • Total cache hits: {total_hits}")
            print(f"  • Transitions: {total_transitions} completed")
            if warnings:
                print(f"\n  Note: {len(warnings)} warning(s) - see above")
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
