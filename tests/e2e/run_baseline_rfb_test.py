#!/usr/bin/env python3
"""
P11: Baseline RFB end-to-end test

Tests that the Rust viewer receives regular framebuffer updates in baseline mode:
- No ContentCache
- No PersistentCache
- No ContinuousUpdates
- No Fence
- Only baseline encodings: Raw(0), CopyRect(1), ZRLE(16)

Acceptance criteria:
- Receives ≥20 FBUs within 5 seconds with animated desktop
- No unknown message type errors in logs
- Stable operation without desync
"""

import sys
import time
import argparse
import re
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check, PreflightError, ArtifactManager, 
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT
)


def count_fbus_in_log(log_path):
    """Count FramebufferUpdate messages in Rust viewer log."""
    fbu_count = 0
    unknown_types = []
    
    with open(log_path, 'r') as f:
        for line in f:
            # Count FBU messages (type 0)
            if 'got server message type 0' in line.lower():
                fbu_count += 1
            # Also count via "Decoded FramebufferUpdate with N damaged rects"
            elif 'decoded framebufferupdate' in line.lower():
                fbu_count += 1
            
            # Detect unknown message types
            if 'unexpected server message type' in line.lower():
                match = re.search(r'type (\d+)', line)
                if match:
                    unknown_types.append(int(match.group(1)))
    
    return fbu_count, unknown_types


def run_viewer(viewer_path, port, artifacts, tracker, duration=5):
    """Run Rust viewer for specified duration."""
    import os
    import subprocess
    
    cmd = [
        viewer_path,
        '--shared',
        '-vvv',
        f'127.0.0.1::{port}',
    ]
    
    log_path = artifacts.logs_dir / 'rust_viewer.log'
    
    # Set DISPLAY to nothing (headless)
    env = os.environ.copy()
    env.pop('DISPLAY', None)
    
    print(f"  Starting Rust viewer (headless)...")
    print(f"  Command: {' '.join(cmd)}")
    log_file = open(log_path, 'w')
    
    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env
    )
    
    tracker.register('rust_viewer', proc)
    
    # Let it run for specified duration
    print(f"  Running for {duration} seconds...")
    time.sleep(duration)
    
    # Terminate
    print("  Terminating viewer...")
    tracker.terminate('rust_viewer')
    log_file.close()
    
    return log_path


def main():
    parser = argparse.ArgumentParser(
        description='Baseline RFB test for Rust viewer (P11)'
    )
    parser.add_argument('--display', type=int, default=998,
                       help='Display number for test server (default: 998)')
    parser.add_argument('--port', type=int, default=6898,
                       help='Port for test server (default: 6898)')
    parser.add_argument('--duration', type=int, default=5,
                       help='Test duration in seconds (default: 5)')
    parser.add_argument('--verbose', action='store_true',
                       help='Verbose output')
    
    args = parser.parse_args()
    
    print("=" * 70)
    print("P11: Baseline RFB End-to-End Test")
    print("=" * 70)
    print()
    
    # 1. Create artifact manager
    print("[1/6] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()
    
    # 2. Preflight checks
    print("\n[2/6] Running preflight checks...")
    try:
        binaries = preflight_check(verbose=args.verbose)
    except PreflightError as e:
        print(f"\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1
    
    # Check port/display availability
    if not check_port_available(args.port):
        print(f"\n✗ FAIL: Port {args.port} already in use")
        return 1
    if not check_display_available(args.display):
        print(f"\n✗ FAIL: Display :{args.display} already in use")
        return 1
    
    print("✓ All preflight checks passed")
    
    # 3. Initialize process tracker
    tracker = ProcessTracker()
    
    try:
        # 4. Start VNC server with animated content
        print(f"\n[3/6] Starting VNC server (:{args.display})...")
        server = VNCServer(
            args.display, args.port, "baseline_server",
            artifacts, tracker,
            geometry="800x600",
            log_level="*:stderr:30",
            server_choice='system'  # Use system TigerVNC for stable baseline
        )
        
        if not server.start():
            print("\n✗ FAIL: Could not start VNC server")
            return 1
        
        # 5. Start animated content (xclock)
        print("\n[4/6] Starting animated content (xclock)...")
        import subprocess
        import os
        
        env = os.environ.copy()
        env['DISPLAY'] = f':{args.display}'
        
        xclock_log = artifacts.logs_dir / 'xclock.log'
        xclock_file = open(xclock_log, 'w')
        
        xclock_proc = subprocess.Popen(
            ['xclock', '-update', '1'],
            stdout=xclock_file,
            stderr=subprocess.STDOUT,
            env=env,
            preexec_fn=os.setpgrp
        )
        tracker.register('xclock', xclock_proc)
        time.sleep(1.0)  # Let xclock start
        
        # 6. Run Rust viewer and collect metrics
        print(f"\n[5/6] Running Rust viewer test...")
        rust_viewer = binaries.get('rust_viewer')
        if not rust_viewer:
            print("\n✗ FAIL: Rust viewer not found")
            return 1
        
        log_path = run_viewer(rust_viewer, args.port, artifacts, tracker, args.duration)
        
        # 7. Analyze results
        print("\n[6/6] Analyzing results...")
        fbu_count, unknown_types = count_fbus_in_log(log_path)
        
        print()
        print("=" * 70)
        print("RESULTS")
        print("=" * 70)
        print(f"Duration: {args.duration}s")
        print(f"FBU count: {fbu_count}")
        print(f"FBU rate: {fbu_count / args.duration:.2f} FBU/s")
        print(f"Unknown message types: {unknown_types if unknown_types else 'None'}")
        print()
        
        # Acceptance criteria
        threshold = 20 if args.duration >= 5 else (args.duration * 4)
        passed = True
        
        if fbu_count < threshold:
            print(f"✗ FAIL: FBU count {fbu_count} < threshold {threshold}")
            passed = False
        else:
            print(f"✓ PASS: FBU count {fbu_count} ≥ threshold {threshold}")
        
        if unknown_types:
            print(f"✗ FAIL: Unexpected message types detected: {unknown_types}")
            passed = False
        else:
            print("✓ PASS: No unknown message types")
        
        print()
        print(f"Logs saved to: {artifacts.logs_dir}")
        
        return 0 if passed else 1
        
    finally:
        print("\nCleaning up...")
        tracker.cleanup()


if __name__ == '__main__':
    sys.exit(main())
