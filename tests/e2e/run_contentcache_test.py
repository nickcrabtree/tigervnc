#!/usr/bin/env python3
"""
Main orchestrator for ContentCache end-to-end tests.

Runs VNC-in-VNC setup to validate ContentCache behavior between
C++ and Rust viewers.
"""

import sys
import time
import argparse
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check, PreflightError, ArtifactManager, 
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT, BUILD_DIR
)
from scenarios import ScenarioRunner
from log_parser import (
    parse_cpp_log, parse_rust_log, compute_metrics, format_metrics_summary
)
from comparator import compare_metrics, Tolerances, format_comparison_result


def run_external_viewer(viewer_path, port, artifacts, tracker, name, display_for_viewer=None):
    """
    Run external viewer connecting to VNC server.
    
    Args:
        viewer_path: Path to viewer binary
        port: VNC server port to connect to
        artifacts: ArtifactManager instance
        tracker: ProcessTracker instance
        name: Process name for tracking (e.g., 'cpp_viewer', 'rust_viewer')
        display_for_viewer: Optional X display for viewer window (for screenshots)
    
    Returns:
        Process object
    """
    import os
    import subprocess
    from pathlib import Path as _Path

    viewer_basename = _Path(viewer_path).name.lower()
    is_rust = ('rust' in name.lower()) or ('rs' in viewer_basename)

    if is_rust:
        # Rust viewer uses clap-style flags
        cmd = [
            viewer_path,
            '--shared',
            '-vvv',
            f'127.0.0.1::{port}',
        ]
    else:
        # C++ viewer uses TigerVNC parameter syntax
        cmd = [
            viewer_path,
            f'127.0.0.1::{port}',
            'Shared=1',
            'Log=*:stderr:100',
        ]
    
    log_path = artifacts.logs_dir / f'{name}.log'
    
    # Build environment - set DISPLAY to target if provided
    env = os.environ.copy()
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
    time.sleep(2.0)  # Let viewer connect
    
    return proc


def main():
    parser = argparse.ArgumentParser(
        description='ContentCache end-to-end test for C++ and Rust viewers'
    )
    parser.add_argument('--display-content', type=int, default=998,
                       help='Display number for content server (default: 998)')
    parser.add_argument('--port-content', type=int, default=6898,
                       help='Port for content server (default: 6898)')
    parser.add_argument('--display-viewer', type=int, default=999,
                       help='Display number for viewer window (default: 999)')
    parser.add_argument('--port-viewer', type=int, default=6899,
                       help='Port for viewer window server (default: 6899)')
    parser.add_argument('--duration', type=int, default=90,
                       help='Scenario duration in seconds (default: 90)')
    parser.add_argument('--wm', default='openbox',
                       help='Window manager (default: openbox)')
    parser.add_argument('--verbose', action='store_true',
                       help='Verbose output')
    parser.add_argument('--animated', action='store_true',
                        help='Use animated scenario (xclock) to force frequent updates')
    parser.add_argument('--skip-rust', action='store_true',
                       help='Skip Rust viewer run (baseline only)')
    parser.add_argument('--server-modes', default='auto',
                       help='Comma-separated list of server modes to test: system,local,auto (default: auto)')
    
    args = parser.parse_args()
    
    print("=" * 70)
    print("TigerVNC ContentCache End-to-End Test")
    print("=" * 70)
    print()
    
    # 1. Create artifact manager
    print("[1/10] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()
    
    # 2. Preflight checks
    print("\n[2/10] Running preflight checks...")
    try:
        binaries = preflight_check(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1
    
    # Check port/display availability
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
    
    # 3. Initialize process tracker
    tracker = ProcessTracker()

    # Determine server modes to run
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    local_server_exists = local_server_symlink.exists() or local_server_actual.exists()
    requested_modes = [m.strip() for m in args.server_modes.split(',')]
    server_modes = []
    if len(requested_modes) == 1 and requested_modes[0] == 'auto':
        server_modes = ['system'] + (['local'] if local_server_exists else [])
    else:
        for m in requested_modes:
            if m in ('system', 'local'):
                if m == 'local' and not local_server_exists:
                    print("⚠ Skipping 'local' server mode (Xnjcvnc not found)")
                    continue
                server_modes.append(m)
            else:
                print(f"⚠ Ignoring unknown server mode: {m}")
        if not server_modes:
            server_modes = ['system']

    metrics_by = {}

    try:
        for mode in server_modes:
            print("\n" + "#" * 70)
            print(f"SERVER MODE: {mode.upper()}")
            print("#" * 70)

            # 4. Start content server (shows automated desktop content)
            print(f"\n[3/10] Starting content server (:{args.display_content})...")
            server_content = VNCServer(
                args.display_content, args.port_content, f"content_{mode}",
                artifacts, tracker,
                geometry="1600x1000",
                log_level="*:stderr:30",
                server_choice=mode
            )
            
            if not server_content.start():
                print("\n✗ FAIL: Could not start content server")
                return 1
            
            if not server_content.start_session(wm=args.wm):
                print("\n✗ FAIL: Could not start content server session")
                return 1
            
            print("✓ Content server ready")
            
            # 5. Start viewer window server (just provides desktop for viewer window)
            print(f"\n[4/10] Starting viewer window server (:{args.display_viewer})...")
            server_viewer = VNCServer(
                args.display_viewer, args.port_viewer, f"viewer_window_{mode}",
                artifacts, tracker,
                geometry="1600x1000",
                log_level="*:stderr:30",
                server_choice=mode
            )
            
            if not server_viewer.start():
                print("\n✗ FAIL: Could not start viewer window server")
                return 1
            
            if not server_viewer.start_session(wm=args.wm):
                print("\n✗ FAIL: Could not start viewer window server session")
                return 1
            
            print("✓ Viewer window server ready")
            
            # 6. Launch internal viewer (C++) connecting content server to viewer window
            # This viewer runs on :999 display, connects to :998 content server
            print(f"\n[5/10] Launching internal viewer...")
            internal_viewer_cmd = [
                binaries['cpp_viewer'],
                f'127.0.0.1::{args.port_content}',  # Connect to content server
                'ViewOnly=1',
                'Shared=1',
                'Log=*:stderr:0',  # Suppress logs for internal viewer
            ]
            
            internal_proc = server_viewer.run_in_display(
                internal_viewer_cmd, 'internal_viewer'
            )
            
            time.sleep(3.0)  # Let internal viewer connect and stabilize
            
            if internal_proc.poll() is not None:
                print("\n✗ FAIL: Internal viewer exited prematurely")
                return 1
            
            print("✓ Internal viewer connected")
            
            # 7. Initialize scenario runner (runs on content server display)
            print(f"\n[6/10] Running C++ viewer baseline...")
            runner = ScenarioRunner(args.display_content, verbose=args.verbose)
            
            # 8. Run C++ viewer external (baseline)
            # Display window on :999, connect to :998 (content server directly)
            cpp_log_name = f'{mode}_cpp_viewer'
            cpp_proc = run_external_viewer(
                binaries['cpp_viewer'],
                args.port_content,  # Connect to content server
                artifacts,
                tracker,
                cpp_log_name,
                display_for_viewer=args.display_viewer  # Display window in viewer server
            )
            
            if cpp_proc.poll() is not None:
                print("\n✗ FAIL: C++ viewer exited prematurely")
                return 1
            
            print("  Running scenario...")
            
            # 9. Run scenario for C++
            try:
                if args.animated:
                    stats = runner.cache_hits_with_clock(duration_sec=args.duration)
                else:
                    stats = runner.cache_hits_minimal(duration_sec=args.duration)
                print(f"  Scenario completed: {stats['windows_opened']} windows, {stats['commands_typed']} commands")
            except Exception as e:
                print(f"\n✗ FAIL: Scenario execution failed: {e}")
                return 1
            
            time.sleep(3.0)  # Pipeline flush
            
            # 10. Terminate C++ viewer
            print("  Stopping C++ viewer...")
            tracker.cleanup(cpp_log_name)
            
            print("✓ C++ baseline complete")
            
            # Optional: baseline-only mode
            if args.skip_rust:
                pass  # still collect baseline metrics below
            else:
                # 11. Brief pause before Rust run
                print(f"\n[7/10] Running Rust viewer candidate...")
                time.sleep(2.0)
                
                # 12. Run Rust viewer external (candidate)
                rust_log_name = f'{mode}_rust_viewer'
                rust_proc = run_external_viewer(
                    binaries['rust_viewer'],
                    args.port_content,  # Connect to content server
                    artifacts,
                    tracker,
                    rust_log_name,
                    display_for_viewer=args.display_viewer  # Display window in viewer server
                )
                
                if rust_proc.poll() is not None:
                    print("\n✗ FAIL: Rust viewer exited prematurely")
                    # Show tail of log to help debug
                    rust_log = artifacts.logs_dir / f'{rust_log_name}.log'
                    try:
                        with open(rust_log, 'r', errors='replace') as f:
                            tail = f.readlines()[-40:]
                        print("\nLast lines of rust_viewer.log:")
                        print(''.join(tail))
                    except Exception:
                        pass
                    return 1
                
                print("  Running scenario...")
                
                # 13. Replay scenario for Rust
                try:
                    if args.animated:
                        stats = runner.cache_hits_with_clock(duration_sec=args.duration)
                    else:
                        stats = runner.cache_hits_minimal(duration_sec=args.duration)
                    print(f"  Scenario completed: {stats['windows_opened']} windows, {stats['commands_typed']} commands")
                except Exception as e:
                    print(f"\n✗ FAIL: Scenario execution failed: {e}")
                    return 1
                
                time.sleep(3.0)  # Pipeline flush
                
                # 14. Terminate Rust viewer
                print("  Stopping Rust viewer...")
                tracker.cleanup(rust_log_name)
                
                print("✓ Rust candidate complete")

            # 15. Parse logs for this mode
            print(f"\n[8/10] Parsing logs for server mode '{mode}'...")
            metrics_by.setdefault(mode, {})
            cpp_log = artifacts.logs_dir / f'{mode}_cpp_viewer.log'
            if not cpp_log.exists():
                print(f"\n✗ FAIL: C++ viewer log not found: {cpp_log}")
                return 1
            cpp_parsed = parse_cpp_log(cpp_log)
            metrics_by[mode]['cpp'] = compute_metrics(cpp_parsed)

            if not args.skip_rust:
                rust_log = artifacts.logs_dir / f'{mode}_rust_viewer.log'
                if rust_log.exists():
                    rust_parsed = parse_rust_log(rust_log)
                    metrics_by[mode]['rust'] = compute_metrics(rust_parsed)
            
            print("✓ Logs parsed")

            # Teardown this mode before next one
            tracker.cleanup(f"viewer_window_{mode}_internal_viewer")
            server_viewer.stop()
            server_content.stop()

        # After all modes, print results and comparisons
        print("\n" + "=" * 70)
        print("RESULTS BY SERVER MODE")
        print("=" * 70)
        for mode, data in metrics_by.items():
            print(f"\n--- {mode.upper()} SERVER ---")
            if 'cpp' in data:
                print("\nBASELINE (C++ Viewer)")
                print(format_metrics_summary(data['cpp']))
            if 'rust' in data:
                print("\nCANDIDATE (Rust Viewer)")
                print(format_metrics_summary(data['rust']))

        # Cross-server comparisons (if we have both)
        if 'system' in metrics_by and 'local' in metrics_by:
            print("\n" + "=" * 70)
            print("SERVER COMPARISON: system vs local")
            print("=" * 70)
            if 'cpp' in metrics_by['system'] and 'cpp' in metrics_by['local']:
                print("\nC++ Viewer:")
                comparison_cpp = compare_metrics(metrics_by['system']['cpp'], metrics_by['local']['cpp'])
                print(format_comparison_result(comparison_cpp))
            if 'rust' in metrics_by['system'] and 'rust' in metrics_by['local']:
                print("\nRust Viewer:")
                comparison_rust = compare_metrics(metrics_by['system']['rust'], metrics_by['local']['rust'])
                print(format_comparison_result(comparison_rust))

        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"All artifacts saved to: {artifacts.base_dir}")
        print(f"  Logs: {artifacts.logs_dir}")
        print(f"  Screenshots: {artifacts.screenshots_dir}")
        print(f"  Reports: {artifacts.reports_dir}")
        
        # Determine overall pass/fail: if any comparison failed, fail
        exit_code = 0
        if 'system' in metrics_by and 'local' in metrics_by:
            if 'cpp' in metrics_by['system'] and 'cpp' in metrics_by['local']:
                if not compare_metrics(metrics_by['system']['cpp'], metrics_by['local']['cpp']).passed:
                    exit_code = 1
            if not args.skip_rust and 'rust' in metrics_by['system'] and 'rust' in metrics_by['local']:
                if not compare_metrics(metrics_by['system']['rust'], metrics_by['local']['rust']).passed:
                    exit_code = 1
        print("\n" + "=" * 70)
        print("✓ TEST PASSED" if exit_code == 0 else "✗ TEST FAILED")
        print("=" * 70)
        return exit_code
        print(f"\n[8/10] Parsing logs...")
        cpp_log = artifacts.logs_dir / 'cpp_viewer.log'
        rust_log = artifacts.logs_dir / 'rust_viewer.log'
        
        if not cpp_log.exists():
            print(f"\n✗ FAIL: C++ viewer log not found: {cpp_log}")
            return 1
        
        if not rust_log.exists():
            print(f"\n✗ FAIL: Rust viewer log not found: {rust_log}")
            return 1
        
        cpp_parsed = parse_cpp_log(cpp_log)
        rust_parsed = parse_rust_log(rust_log)
        
        cpp_metrics = compute_metrics(cpp_parsed)
        rust_metrics = compute_metrics(rust_parsed)
        
        print("✓ Logs parsed")
        
        # 16. Compare metrics
        print(f"\n[9/10] Comparing metrics...")
        comparison = compare_metrics(cpp_metrics, rust_metrics)
        
        # 17. Print results
        print(f"\n[10/10] Results")
        print("\n" + "=" * 70)
        print("BASELINE (C++ Viewer)")
        print("=" * 70)
        print(format_metrics_summary(cpp_metrics))
        
        print("\n" + "=" * 70)
        print("CANDIDATE (Rust Viewer)")
        print("=" * 70)
        print(format_metrics_summary(rust_metrics))
        
        print("\n" + "=" * 70)
        print("COMPARISON")
        print("=" * 70)
        print(format_comparison_result(comparison))
        
        print("\n" + "=" * 70)
        print("ARTIFACTS")
        print("=" * 70)
        print(f"All artifacts saved to: {artifacts.base_dir}")
        print(f"  Logs: {artifacts.logs_dir}")
        print(f"  Screenshots: {artifacts.screenshots_dir}")
        print(f"  Reports: {artifacts.reports_dir}")
        
        # 18. Return exit code
        if comparison.passed:
            print("\n" + "=" * 70)
            print("✓ TEST PASSED")
            print("=" * 70)
            return 0
        else:
            print("\n" + "=" * 70)
            print("✗ TEST FAILED")
            print("=" * 70)
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
        # 19. Always clean up
        print("\nCleaning up...")
        tracker.cleanup_all()
        print("✓ Cleanup complete")


if __name__ == '__main__':
    sys.exit(main())
