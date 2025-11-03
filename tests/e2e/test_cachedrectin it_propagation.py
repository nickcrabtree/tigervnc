#!/usr/bin/env python3
"""
TDD Test: CachedRectInit Propagation

Tests that when the server has a ContentCache hit, it properly sends either:
1. A CachedRect reference (if client knows the cacheId), OR
2. A CachedRectInit message (if client doesn't know the cacheId yet)

Bug identified 2025-11-03: Server logs show cache hits but 0 references sent.
This indicates CachedRectInit messages are queued but never transmitted.

Test Strategy:
- Generate repeated content (same rectangle appears multiple times)
- Track server-side cache hits
- Track client-side CachedRect and CachedRectInit messages received
- Verify: server_cache_hits <= (client_cached_rect + client_cached_rect_init)
"""

import sys
import time
import re
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check, ArtifactManager, ProcessTracker, VNCServer,
    check_port_available, check_display_available, PROJECT_ROOT
)
from scenarios import ScenarioRunner


def parse_server_cache_stats(log_path: Path) -> dict:
    """
    Parse server-side ContentCache statistics from Xnjcvnc log.
    
    Returns:
        {
            'cache_lookups': int,
            'cache_hits': int,  # This is "References sent" in server log
            'references_sent': int,  # Same as cache_hits
        }
    """
    stats = {
        'cache_lookups': 0,
        'cache_hits': 0,
        'references_sent': 0,
    }
    
    if not log_path.exists():
        return stats
    
    with open(log_path, 'r', errors='replace') as f:
        content = f.read()
    
    # Server logs: "Lookups: 251, References sent: 0 (0.0%)"
    match = re.search(r'Lookups:\s*(\d+),\s*References sent:\s*(\d+)', content)
    if match:
        stats['cache_lookups'] = int(match.group(1))
        stats['references_sent'] = int(match.group(2))
        stats['cache_hits'] = stats['references_sent']
    
    return stats


def parse_client_cache_messages(log_path: Path) -> dict:
    """
    Parse client-side ContentCache protocol messages.
    
    Returns:
        {
            'cached_rect_received': int,
            'cached_rect_init_received': int,
            'request_cached_data_sent': int,
        }
    """
    stats = {
        'cached_rect_received': 0,
        'cached_rect_init_received': 0,
        'request_cached_data_sent': 0,
    }
    
    if not log_path.exists():
        return stats
    
    with open(log_path, 'r', errors='replace') as f:
        for line in f:
            line_lower = line.lower()
            
            # Client receives CachedRect
            if 'received cachedrect' in line_lower and 'init' not in line_lower:
                stats['cached_rect_received'] += 1
            
            # Client receives CachedRectInit
            elif 'received cachedre ctinit' in line_lower or 'cachedrectinit' in line_lower:
                stats['cached_rect_init_received'] += 1
            
            # Client sends RequestCachedData
            elif 'requestcacheddata' in line_lower or 'requesting from server' in line_lower:
                stats['request_cached_data_sent'] += 1
    
    return stats


def run_test(display_num: int = 998, port_num: int = 6898, duration: int = 60) -> bool:
    """
    Run the CachedRectInit propagation test.
    
    Returns:
        True if test passes, False if test fails
    """
    print("=" * 70)
    print("TDD Test: CachedRectInit Propagation")
    print("=" * 70)
    print()
    print("Bug: Server cache hits don't result in CachedRect references")
    print("Expected: server_cache_hits <= (CachedRect + CachedRectInit) received")
    print()
    
    # Setup
    artifacts = ArtifactManager()
    artifacts.create()
    print(f"Artifacts: {artifacts.base_dir}")
    
    tracker = ProcessTracker()
    
    # Check availability
    if not check_port_available(port_num):
        print(f"✗ FAIL: Port {port_num} in use")
        return False
    if not check_display_available(display_num):
        print(f"✗ FAIL: Display :{display_num} in use")
        return False
    
    try:
        # Start VNC server with ContentCache enabled
        print(f"\n[1/4] Starting VNC server (:{display_num})...")
        server = VNCServer(
            display_num, port_num, "contentcache_test",
            artifacts, tracker,
            geometry="1024x768",
            log_level="*:stderr:100",  # Verbose logging
            server_choice="local",  # Use local Xnjcvnc with ContentCache
        )
        
        if not server.start():
            print("✗ FAIL: Could not start server")
            return False
        
        # Start window manager
        print("[2/4] Starting window manager...")
        server.start_wm("openbox")
        time.sleep(2)
        
        # Run scenario with repeated content
        print(f"[3/4] Running scenario ({duration}s)...")
        print("  Generating repeated rectangles to trigger cache hits...")
        runner = ScenarioRunner(display_num, verbose=False)
        
        # Use xterm_cycles scenario - opens/closes xterm repeatedly
        # This creates repeated content (same xterm window appearance)
        runner.xterm_cycles(duration_sec=duration, cycle_count=10)
        
        print("[4/4] Analyzing logs...")
        
        # Parse server log
        server_log = artifacts.logs_dir / "contentcache_test.log"
        server_stats = parse_server_cache_stats(server_log)
        
        print("\nServer-side ContentCache statistics:")
        print(f"  Cache lookups: {server_stats['cache_lookups']}")
        print(f"  References sent: {server_stats['references_sent']}")
        
        # For this test, we expect the server to have cache hits
        # but we're checking if those hits were communicated to clients
        # Since we're running without a viewer client, we check the server's
        # internal accounting
        
        # The bug: server has cache hits but references_sent = 0
        # Expected: if cache_lookups > 0, then eventually references_sent > 0
        
        if server_stats['cache_lookups'] == 0:
            print("\n⚠ WARNING: No cache lookups performed")
            print("  (May indicate ContentCache is disabled)")
            return False
        
        # Check for the bug condition
        if server_stats['references_sent'] == 0:
            print("\n✗ FAIL: Bug detected!")
            print(f"  Server performed {server_stats['cache_lookups']} cache lookups")
            print("  but sent 0 CachedRect references to clients")
            print("\nRoot cause: CachedRectInit messages not being propagated")
            print("  - Server queues CachedRectInit when client doesn't know cacheId")
            print("  - But queued inits are only sent if client supports LastRect encoding")
            print("  - Or queued inits are never getting transmitted properly")
            return False
        
        print(f"\n✓ PASS: Server sent {server_stats['references_sent']} references")
        print(f"  ({100.0 * server_stats['references_sent'] / server_stats['cache_lookups']:.1f}% of lookups)")
        return True
        
    finally:
        print("\nCleaning up...")
        tracker.cleanup()


def run_test_with_viewer(display_num: int = 998, port_num: int = 6898, duration: int = 60) -> bool:
    """
    Run test with an actual viewer client to track client-side messages.
    
    This validates the full protocol flow:
    1. Server has cache hit
    2. Server sends CachedRect OR CachedRectInit
    3. Client receives the message
    """
    print("=" * 70)
    print("TDD Test: CachedRectInit Propagation (with viewer)")
    print("=" * 70)
    print()
    
    artifacts = ArtifactManager()
    artifacts.create()
    print(f"Artifacts: {artifacts.base_dir}")
    
    tracker = ProcessTracker()
    
    if not check_port_available(port_num):
        print(f"✗ FAIL: Port {port_num} in use")
        return False
    if not check_display_available(display_num):
        print(f"✗ FAIL: Display :{display_num} in use")
        return False
    
    try:
        # Start server
        print(f"\n[1/5] Starting VNC server (:{display_num})...")
        server = VNCServer(
            display_num, port_num, "contentcache_viewer_test",
            artifacts, tracker,
            geometry="1024x768",
            log_level="*:stderr:100",
            server_choice="local",
        )
        
        if not server.start():
            print("✗ FAIL: Could not start server")
            return False
        
        server.start_wm("openbox")
        time.sleep(2)
        
        # Start viewer
        print("[2/5] Starting viewer...")
        import subprocess
        import os
        
        viewer_path = PROJECT_ROOT / "build" / "vncviewer" / "njcvncviewer"
        viewer_log = artifacts.logs_dir / "viewer.log"
        
        cmd = [
            str(viewer_path),
            f'127.0.0.1::{port_num}',
            'Shared=1',
            'Log=*:stderr:100',
        ]
        
        log_file = open(viewer_log, 'w')
        viewer_proc = subprocess.Popen(
            cmd,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            preexec_fn=os.setpgrp,
            env={**os.environ, 'DISPLAY': ''}  # Headless
        )
        tracker.register('viewer', viewer_proc)
        time.sleep(3)
        
        # Run scenario
        print(f"[3/5] Running scenario ({duration}s)...")
        runner = ScenarioRunner(display_num, verbose=False)
        runner.xterm_cycles(duration_sec=duration, cycle_count=10)
        
        # Let viewer finish processing
        time.sleep(2)
        
        print("[4/5] Analyzing logs...")
        
        # Parse logs
        server_log = artifacts.logs_dir / "contentcache_viewer_test.log"
        server_stats = parse_server_cache_stats(server_log)
        client_stats = parse_client_cache_messages(viewer_log)
        
        print("\nServer-side statistics:")
        print(f"  Cache lookups: {server_stats['cache_lookups']}")
        print(f"  References sent: {server_stats['references_sent']}")
        
        print("\nClient-side statistics:")
        print(f"  CachedRect received: {client_stats['cached_rect_received']}")
        print(f"  CachedRectInit received: {client_stats['cached_rect_init_received']}")
        print(f"  RequestCachedData sent: {client_stats['request_cached_data_sent']}")
        
        # Validation
        print("\n[5/5] Validating...")
        
        total_client_messages = (client_stats['cached_rect_received'] + 
                                 client_stats['cached_rect_init_received'])
        
        if server_stats['cache_lookups'] == 0:
            print("⚠ WARNING: No cache lookups")
            return False
        
        # Bug condition: server has cache hits but client received no messages
        if server_stats['references_sent'] == 0 and total_client_messages == 0:
            print("\n✗ FAIL: Bug detected!")
            print("  Server had cache lookups but sent no cache references")
            print("  Client received no CachedRect or CachedRectInit messages")
            return False
        
        # Expected: references sent should match client messages received
        if server_stats['references_sent'] != total_client_messages:
            print(f"\n✗ FAIL: Mismatch!")
            print(f"  Server sent {server_stats['references_sent']} references")
            print(f"  Client received {total_client_messages} messages")
            print("  These should be equal")
            return False
        
        print(f"\n✓ PASS: Protocol flow validated")
        print(f"  Server sent {server_stats['references_sent']} references")
        print(f"  Client received {total_client_messages} cache messages")
        return True
        
    finally:
        print("\nCleaning up...")
        log_file.close()
        tracker.cleanup()


if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser()
    parser.add_argument('--display', type=int, default=998)
    parser.add_argument('--port', type=int, default=6898)
    parser.add_argument('--duration', type=int, default=60)
    parser.add_argument('--with-viewer', action='store_true',
                       help='Run test with actual viewer client')
    args = parser.parse_args()
    
    # Run preflight
    try:
        preflight_check(verbose=False)
    except Exception as e:
        print(f"✗ Preflight failed: {e}")
        sys.exit(1)
    
    # Run appropriate test
    if args.with_viewer:
        success = run_test_with_viewer(args.display, args.port, args.duration)
    else:
        success = run_test(args.display, args.port, args.duration)
    
    sys.exit(0 if success else 1)
