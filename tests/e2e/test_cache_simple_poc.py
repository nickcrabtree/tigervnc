#!/usr/bin/env python3
"""
Simple proof-of-concept PersistentCache test.

Uses the same tiled-logo static content scenario as the C++
ContentCache and PersistentCache tests so that, in a cold-cache
run, PersistentCache sees the same pattern of hits as ContentCache
for identical content.

The server and viewer both exercise the unified PersistentCache
engine; "ContentCache" is now just a session-only policy layered on
that engine. This test is expected to remain red when PersistentCache
is not triggered (for example, when only lossless paths are cached or
when the scenario fails to meet caching thresholds) so that it
continues to act as a TDD guard for ensuring real PersistentCache
activity in simple repeated-content scenarios.
"""

import sys
import time
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

def main():
    print("=" * 70)
    print("Simple Cache Proof-of-Concept Test")
    print("=" * 70)
    print("\nThis test uses xclock to generate repeated content")
    print("that should produce cache hits.\n")

    # Setup
    display_content = 998
    port_content = 6898
    display_viewer = 999
    port_viewer = 6899
    
    artifacts = ArtifactManager()
    artifacts.create()
    
    print("[1/6] Preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=False)
    except PreflightError as e:
        print(f"✗ FAIL: {e}")
        return 1
    
    if not check_port_available(port_content) or not check_port_available(port_viewer):
        print("✗ FAIL: Ports already in use")
        return 1
    
    if not check_display_available(display_content) or not check_display_available(display_viewer):
        print("✗ FAIL: Displays already in use")
        return 1
    
    print("✓ Preflight passed")
    
    tracker = ProcessTracker()
    
    # Determine server
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'
    
    try:
        # Start servers (force PersistentCache-only path on the server to
        # ensure that the proof-of-concept actually exercises the
        # PersistentCache protocol rather than falling back to
        # ContentCache-only behaviour.
        print(f"\n[2/6] Starting VNC servers (PersistentCache-focused)...")
        server_content = VNCServer(
            display_content, port_content, "poc_content",
            artifacts, tracker, geometry="800x600",
            log_level="*:stderr:100", server_choice=server_mode,
            # Server-side ContentCache parameters have been removed in the
            # unified cache engine. EnablePersistentCache controls the
            # unified cache; ContentCache vs PersistentCache is now a
            # viewer-side policy only.
            server_params={
                'EnablePersistentCache': '1',     # ensure PersistentCache path is enabled
                # PersistentCacheMinRectSize keeps default unless overridden
            }
        )
        if not server_content.start() or not server_content.start_session(wm='openbox'):
            print("✗ FAIL: Content server failed")
            return 1
        
        server_viewer = VNCServer(
            display_viewer, port_viewer, "poc_viewerwin",
            artifacts, tracker, geometry="800x600",
            log_level="*:stderr:30", server_choice=server_mode
        )
        if not server_viewer.start() or not server_viewer.start_session(wm='openbox'):
            print("✗ FAIL: Viewer server failed")
            return 1
        
        print("✓ Servers ready")
        
        # Start viewer with PersistentCache
        print(f"\n[3/6] Starting viewer with PersistentCache...")
        viewer_log = artifacts.logs_dir / 'poc_viewer.log'
        env = os.environ.copy()
        env['TIGERVNC_VIEWER_DEBUG_LOG'] = '1'
        env['DISPLAY'] = f':{display_viewer}'
        
        # SANDBOXED: Use test-specific cache (not production cache)
        cache_dir = artifacts.get_sandboxed_cache_dir()
        
        with open(viewer_log, 'w') as log_file:
            viewer_proc = subprocess.Popen(
                [binaries['cpp_viewer'], f'127.0.0.1::{port_content}',
                 'Shared=1', 'Log=*:stderr:100', 'PersistentCache=1',
                 f'PersistentCachePath={cache_dir}'],
                stdout=log_file, stderr=subprocess.STDOUT,
                preexec_fn=os.setpgrp, env=env
            )
        tracker.register('poc_viewer', viewer_proc)
        time.sleep(2.0)
        
        if viewer_proc.poll() is not None:
            print("✗ FAIL: Viewer exited")
            return 1
        
        print("✓ Viewer connected")
        
        # Generate repeated static content using the same tiled-logo
        # scenario as the ContentCache and C++ PersistentCache tests.
        print(f"\n[4/6] Generating repeated static content (tiled logos)...")
        runner = StaticScenarioRunner(display_content, verbose=False)
        stats = runner.tiled_logos_test(tiles=12, duration=60.0, delay_between=3.0)
        print(f"  Scenario completed: {stats}")
        
        # Stop and analyze
        print(f"\n[5/6] Analyzing results...")
        tracker.cleanup('poc_viewer')
        time.sleep(1.0)  # Let logs flush
        
        # Parse logs
        server_log = artifacts.logs_dir / f'poc_content_server_{display_content}.log'
        
        print("  Parsing logs...")
        parsed_viewer = parse_cpp_log(viewer_log)
        parsed_server = parse_server_log(server_log, verbose=True)
        
        # Combine
        total_pc_hits = parsed_viewer.persistent_hits + parsed_server.persistent_hits
        total_pc_misses = parsed_viewer.persistent_misses + parsed_server.persistent_misses
        total_lookups = total_pc_hits + total_pc_misses
        
        hit_rate = (100.0 * total_pc_hits / total_lookups) if total_lookups > 0 else 0.0
        
        print("\n[6/6] Results")
        print("=" * 70)
        print(f"PersistentCache Activity:")
        print(f"  Total lookups: {total_lookups}")
        print(f"  Hits:          {total_pc_hits}")
        print(f"  Misses:        {total_pc_misses}")
        print(f"  Hit rate:      {hit_rate:.1f}%")
        
        print(f"\nProtocol messages (viewer):")
        print(f"  CachedRect:     {parsed_viewer.cached_rect_count}")
        print(f"  CachedRectInit: {parsed_viewer.cached_rect_init_count}")
        
        print(f"\nLogs:")
        print(f"  Viewer: {viewer_log}")
        print(f"  Server: {server_log}")
        
        # Success criteria: any cache hits at all proves PersistentCache
        # is active for this scenario. We deliberately fail if the cache
        # is never exercised so that this test can drive regressions.
        if total_pc_hits > 0:
            print("\n✓ SUCCESS: Cache hits detected!")
            print(f"  PersistentCache is working with {total_pc_hits} hits")
            return 0
        else:
            print("\n✗ FAIL: No significant PersistentCache activity (hits=0, lookups=", total_lookups, ")")
            return 1
            
    except KeyboardInterrupt:
        print("\nInterrupted")
        return 130
    except Exception as e:
        print(f"\n✗ FAIL: {e}")
        import traceback
        traceback.print_exc()
        return 1
    finally:
        print("\nCleaning up...")
        tracker.cleanup_all()
        print("✓ Done")


if __name__ == '__main__':
    sys.exit(main())
