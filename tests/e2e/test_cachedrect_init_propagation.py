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
    check_port_available, check_display_available, PROJECT_ROOT,
    BUILD_DIR, best_effort_cleanup_test_server
)
from scenarios import ScenarioRunner
from scenarios_static import StaticScenarioRunner


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
    # Use findall to get all matches, then take the last one (most recent stats)
    matches = re.findall(r'Lookups:\s*(\d+),\s*References sent:\s*(\d+)', content)
    if matches:
        last_match = matches[-1]  # Get the last occurrence
        stats['cache_lookups'] = int(last_match[0])
        stats['references_sent'] = int(last_match[1])
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
            # Unified cache path also reports PersistentCache messages; these
            # are treated as equivalent to ContentCache for this black-box
            # propagation test.
            'persistent_cached_rect_received': int,
            'persistent_cached_rect_init_received': int,
        }
    """
    stats = {
        'cached_rect_received': 0,
        'cached_rect_init_received': 0,
        'request_cached_data_sent': 0,
        'persistent_cached_rect_received': 0,
        'persistent_cached_rect_init_received': 0,
    }
    
    if not log_path.exists():
        return stats
    
    with open(log_path, 'r', errors='replace') as f:
        for line in f:
            line_lower = line.lower()
            
            # Client receives CachedRect (legacy ContentCache)
            # Exclude 'seed' to avoid counting CachedRectSeed messages
            if 'received cachedrect' in line_lower and 'init' not in line_lower and 'persistent' not in line_lower and 'seed' not in line_lower:
                stats['cached_rect_received'] += 1
            
            # Client receives CachedRectInit (legacy ContentCache)
            elif ('received cachedre ctinit' in line_lower or 'cachedrectinit' in line_lower) \
                    and 'persistent' not in line_lower:
                stats['cached_rect_init_received'] += 1
            
            # Unified cache path: PersistentCachedRect / PersistentCachedRectInit.
            # For the purposes of this propagation test, we treat these as
            # equivalent to ContentCache references and inits.
            #
            # IMPORTANT: Viewer logs can occasionally interleave lines (e.g.
            # PlatformPixelBuffer output) such that the "Received" prefix is
            # not present even though the message arrived. Count any line that
            # contains "PersistentCachedRect:" as a reference.
            elif 'persistentcachedrectinit' in line_lower:
                stats['persistent_cached_rect_init_received'] += 1
            elif re.search(r'\bpersistentcachedrect\s*:', line_lower) and 'init' not in line_lower:
                stats['persistent_cached_rect_received'] += 1
            
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
    
    # Check availability; if the test port is already in use, attempt a
    # best-effort cleanup of any orphaned test servers on the dedicated
    # test displays before giving up.
    if not check_port_available(port_num):
        print(f"⚠ Port {port_num} in use; attempting best-effort cleanup of test server on :{display_num}...")
        best_effort_cleanup_test_server(display_num, port_num, verbose=True)
        time.sleep(1.0)
        if not check_port_available(port_num):
            print(f"✗ FAIL: Port {port_num} in use after cleanup attempt")
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
        server.start_session("openbox")
        time.sleep(2)
        
        # Run scenario with repeated content
        print(f"[3/4] Running scenario ({duration}s)...")
        print("  Generating repeated rectangles to trigger cache hits...")
        runner = ScenarioRunner(display_num, verbose=False)
        
        # Use cache_hits_minimal scenario - opens/closes xterm repeatedly
        # This creates repeated content (same xterm window appearance)
        runner.cache_hits_minimal(duration_sec=duration)
        
        print("[4/4] Analyzing logs...")
        
        # Parse server log (matches VNCServer log naming convention)
        server_log = artifacts.logs_dir / f"contentcache_test_server_{display_num}.log"
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
        if server.proc:
            tracker.cleanup(f"vnc_{server.name}")
        if server.wm_proc:
            tracker.cleanup(f"wm_{server.name}")


def run_test_with_viewer(display_num: int = 998, port_num: int = 6898, 
                         viewer_display_num: int = 999, viewer_port_num: int = 6899,
                         duration: int = 60) -> bool:
    """
    Run test with an actual viewer client to track client-side messages.
    
    Uses VNC-in-VNC architecture per WARP.md:
    - Display :998 (port 6898): Test server with ContentCache
    - Display :999 (port 6899): Viewer window server for FLTK GUI
    - Viewer runs with DISPLAY=:999 and connects to 127.0.0.1::6898
    
    This validates the full protocol flow:
    1. Server has cache hit
    2. Server sends CachedRect OR CachedRectInit
    3. Client receives the message
    
    SAFETY: Only uses test displays :998 and :999. Never touches production :1, :2, :3.
    """
    # Safety check: prevent accidental use of production displays
    if display_num in (1, 2, 3) or viewer_display_num in (1, 2, 3):
        print("✗ ABORT: Cannot use production displays :1, :2, or :3 (see WARP.md)")
        return False
    
    print("=" * 70)
    print("TDD Test: CachedRectInit Propagation (with viewer)")
    print("=" * 70)
    print()
    
    artifacts = ArtifactManager()
    artifacts.create()
    print(f"Artifacts: {artifacts.base_dir}")
    
    tracker = ProcessTracker()
    
    # Proactively clean up any stale test servers on the dedicated displays
    # before we even probe the ports. This is in addition to the best-effort
    # cleanup inside check_port_available and helps recover from interrupted
    # or crashed prior runs of the e2e suite.
    best_effort_cleanup_test_server(display_num, port_num, verbose=True)
    best_effort_cleanup_test_server(viewer_display_num, viewer_port_num, verbose=True)
    time.sleep(0.5)
    
    # Check both test and viewer servers are available
    if not check_port_available(port_num):
        print(f"✗ FAIL: Port {port_num} in use")
        return False
    if not check_port_available(viewer_port_num):
        print(f"✗ FAIL: Port {viewer_port_num} in use")
        return False
    if not check_display_available(display_num):
        print(f"✗ FAIL: Display :{display_num} in use")
        return False
    if not check_display_available(viewer_display_num):
        print(f"✗ FAIL: Display :{viewer_display_num} in use")
        return False
    
    try:
        # Start test server (content source)
        print(f"\n[1/6] Starting test server (:{display_num})...")
        server = VNCServer(
            display_num, port_num, "contentcache_viewer_test",
            artifacts, tracker,
            geometry="1024x768",
            log_level="*:stderr:100",
            server_choice="local",
            # This test is specifically about the ContentCache (CachedRect)
            # protocol. Disable PersistentCache on the server to avoid the
            # default PersistentCache-first behavior masking ContentCache
            # statistics.
            server_params={"EnablePersistentCache": "0"},
        )
        
        if not server.start():
            print("✗ FAIL: Could not start test server")
            return False
        
        server.start_session("openbox")
        time.sleep(2)
        
        # Start viewer window server (for FLTK GUI)
        print(f"[2/6] Starting viewer window server (:{viewer_display_num})...")
        viewer_server = VNCServer(
            viewer_display_num, viewer_port_num, "viewer_window",
            artifacts, tracker,
            geometry="1280x800",
            log_level="*:stderr:30",
            server_choice="local",
        )
        
        if not viewer_server.start():
            print("✗ FAIL: Could not start viewer window server")
            return False
        
        viewer_server.start_session("openbox")
        time.sleep(2)
        
        # Start viewer (GUI on :999, connects to :998)
        print("[3/6] Starting viewer...")
        import subprocess
        import os
        
        # Respect BUILD_DIR so tests work with non-default build locations,
        # but allow an explicit override via TIGERVNC_VIEWER_BIN so the same
        # test harness can be reused for different viewer implementations.
        viewer_env = os.environ.get("TIGERVNC_VIEWER_BIN")
        if viewer_env:
            viewer_path = Path(viewer_env)
            if not viewer_path.exists():
                print(f"✗ FAIL: TIGERVNC_VIEWER_BIN points to missing binary: {viewer_path}")
                return False
        else:
            viewer_path = BUILD_DIR / "vncviewer" / "njcvncviewer"
            if not viewer_path.exists():
                # The server rebuild path (make server) performs a sledgehammer
                # clean of the CMake build tree, which can remove the viewer
                # binary after preflight. Ensure the viewer is present by
                # rebuilding it on demand here.
                try:
                    subprocess.run(
                        [
                            "cmake",
                            "--build",
                            str(BUILD_DIR),
                            "--target",
                            "njcvncviewer",
                        ],
                        cwd=str(PROJECT_ROOT),
                        check=True,
                        timeout=600.0,
                    )
                except Exception as e:
                    print(f"✗ FAIL: Could not build C++ viewer at {viewer_path}: {e}")
                    return False
                if not viewer_path.exists():
                    print(f"✗ FAIL: Viewer binary still missing after build: {viewer_path}")
                    return False

        viewer_log = artifacts.logs_dir / "viewer.log"
        
        cmd = [
            str(viewer_path),
            f'127.0.0.1::{port_num}',
            'Shared=1',
            'Log=*:stderr:100',
            # Disable PersistentCache so that this test focuses on the
            # ContentCache CachedRect/CachedRectInit flow.
            'PersistentCache=0',
        ]
        
        # Set DISPLAY to viewer window server
        env = os.environ.copy()
        env['TIGERVNC_VIEWER_DEBUG_LOG'] = '1'
        env['DISPLAY'] = f':{viewer_display_num}'
        
        log_file = open(viewer_log, 'w')
        viewer_proc = subprocess.Popen(
            cmd,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            preexec_fn=os.setpgrp,
            env=env
        )
        tracker.register('viewer', viewer_proc)
        time.sleep(3)
        
        # Run scenario with high-confidence repeated content using the
        # static tiled-logo scenario. This reliably produces rectangles
        # above the ContentCache MinRectSize threshold and generates
        # cache lookups + hits.
        print(f"[4/6] Running scenario ({duration}s)...")
        runner = StaticScenarioRunner(display_num, verbose=False)
        runner.tiled_logos_test(tiles=12, duration=duration, delay_between=3.0)
        
        # Let viewer finish processing
        time.sleep(2)
        
        # Stop viewer to trigger server stats output
        print("[5/6] Stopping viewer and servers...")
        tracker.cleanup('viewer')
        # Wait longer for viewer logs to flush and server to update stats
        time.sleep(3)
        
        # Stop servers (which writes final statistics to logs)
        if viewer_server.proc:
            tracker.cleanup(f"vnc_{viewer_server.name}")
        if viewer_server.wm_proc:
            tracker.cleanup(f"wm_{viewer_server.name}")
        if server.proc:
            tracker.cleanup(f"vnc_{server.name}")
        if server.wm_proc:
            tracker.cleanup(f"wm_{server.name}")
        
        time.sleep(1)  # Ensure logs are flushed
        
        print("[6/6] Analyzing logs...")
        
        # Parse logs (framework creates logs as: {name}_server_{display}.log)
        server_log = artifacts.logs_dir / f"contentcache_viewer_test_server_{display_num}.log"
        server_stats = parse_server_cache_stats(server_log)
        client_stats = parse_client_cache_messages(viewer_log)
        
        print("\nServer-side statistics:")
        print(f"  Cache lookups: {server_stats['cache_lookups']}")
        print(f"  References sent: {server_stats['references_sent']}")
        
        print("\nClient-side statistics:")
        print(f"  CachedRect received: {client_stats['cached_rect_received']}")
        print(f"  CachedRectInit received: {client_stats['cached_rect_init_received']}")
        print(f"  PersistentCachedRect received: {client_stats['persistent_cached_rect_received']}")
        print(f"  PersistentCachedRectInit received: {client_stats['persistent_cached_rect_init_received']}")
        print(f"  RequestCachedData sent: {client_stats['request_cached_data_sent']}")
        
        # Validation
        print("\nValidating...")
        
        total_client_messages = (
            client_stats['cached_rect_received'] +
            client_stats['cached_rect_init_received'] +
            client_stats['persistent_cached_rect_received'] +
            client_stats['persistent_cached_rect_init_received']
        )
        
        if server_stats['cache_lookups'] == 0:
            print("⚠ WARNING: No cache lookups")
            return False
        
        # Bug condition: server has cache hits but client received no messages
        if server_stats['references_sent'] == 0 and total_client_messages == 0:
            print("\n✗ FAIL: Bug detected!")
            print("  Server had cache lookups but sent no cache references")
            print("  Client received no CachedRect/PersistentCachedRect messages")
            return False
        
        # Validation: server's "References sent" counts cache-reference
        # messages (ContentCache or PersistentCache) and the client should
        # see at least that many corresponding rect-reference messages.
        client_total_refs = (
            client_stats['cached_rect_received'] +
            client_stats['persistent_cached_rect_received']
        )
        
        if client_total_refs != server_stats['references_sent']:
            print(f"\n✗ FAIL: Cache reference mismatch!")
            print(f"  Server sent {server_stats['references_sent']} cache references")
            print(f"  Client received {client_total_refs} cache reference messages")
            print("  These should be equal (counting both ContentCache and PersistentCache)")
            return False
        
        if total_client_messages < server_stats['references_sent']:
            print(f"\n✗ FAIL: Client received fewer messages than server sent!")
            print(f"  Server: {server_stats['references_sent']} references")
            print(f"  Client: {total_client_messages} total cache messages")
            return False
        
        print(f"\n✓ PASS: Protocol flow validated")
        print(f"  Server lookups: {server_stats['cache_lookups']}")
        print(f"  Server sent: {server_stats['references_sent']} cache references")
        print(f"  Client received: {client_total_refs} cache references and "
              f"{total_client_messages} total cache messages (including INITs)")
        print(f"  Cache hit rate: {100.0 * server_stats['references_sent'] / server_stats['cache_lookups']:.1}%")
        return True
        
    finally:
        print("\nFinal cleanup...")
        # Close the viewer log file if it was opened
        try:
            if 'log_file' in locals() and not log_file.closed:
                log_file.close()
        except Exception:
            pass
        
        # Targeted cleanup for any processes we know about explicitly. These
        # calls are idempotent and safe even if the processes have already
        # exited.
        if 'viewer_proc' in locals() and viewer_proc.poll() is None:
            tracker.cleanup('viewer')
        
        if 'viewer_server' in locals():
            if viewer_server.proc and viewer_server.proc.poll() is None:
                tracker.cleanup(f"vnc_{viewer_server.name}")
            if viewer_server.wm_proc and viewer_server.wm_proc.poll() is None:
                tracker.cleanup(f"wm_{viewer_server.name}")
        
        if 'server' in locals():
            if server.proc and server.proc.poll() is None:
                tracker.cleanup(f"vnc_{server.name}")
            if server.wm_proc and server.wm_proc.poll() is None:
                tracker.cleanup(f"wm_{server.name}")
        
        # As a final safety net, ensure that any other processes registered in
        # the tracker (e.g. future helpers) are also terminated so they cannot
        # hold on to the dedicated test ports 6898/6899.
        try:
            tracker.cleanup_all()
        except Exception:
            pass


if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser()
    parser.add_argument('--display', type=int, default=998,
                       help='Test server display number (default: 998)')
    parser.add_argument('--port', type=int, default=6898,
                       help='Test server port (default: 6898)')
    parser.add_argument('--viewer-display', type=int, default=999,
                       help='Viewer window server display (default: 999)')
    parser.add_argument('--viewer-port', type=int, default=6899,
                       help='Viewer window server port (default: 6899)')
    parser.add_argument('--duration', type=int, default=60,
                       help='Scenario duration in seconds (default: 60)')
    parser.add_argument('--with-viewer', action='store_true',
                       help='Run test with actual viewer client')
    args = parser.parse_args()
    
    # Run preflight
    try:
        preflight_check(verbose=False)
    except Exception as e:
        print(f"✗ Preflight failed: {e}")
        sys.exit(1)
    
    # Always run the viewer-backed variant. The server-only path does not
    # exercise the encode pipeline and therefore cannot produce cache
    # statistics. The --with-viewer flag is retained for compatibility but
    # no longer changes behavior.
    success = run_test_with_viewer(
        args.display, args.port,
        args.viewer_display, args.viewer_port,
        args.duration
    )
    
    sys.exit(0 if success else 1)
