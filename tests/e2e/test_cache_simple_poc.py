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

from framework import preflight_check_cpp_only, PreflightError, ArtifactManager, ProcessTracker, VNCServer, check_port_available, check_display_available, BUILD_DIR
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
    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    server_mode = "local" if (local_server_symlink.exists() or local_server_actual.exists()) else "system"

    try:
        # Start servers (force PersistentCache-only path on the server to
        # ensure that the proof-of-concept actually exercises the
        # PersistentCache protocol rather than falling back to
        # ContentCache-only behaviour.
        print("\n[2/6] Starting VNC servers (PersistentCache-focused)...")
        server_content = VNCServer(
            display_content,
            port_content,
            "poc_content",
            artifacts,
            tracker,
            geometry="800x600",
            log_level="*:stderr:100",
            server_choice=server_mode,
            # Server-side ContentCache parameters have been removed in the
            # unified cache engine. EnablePersistentCache controls the
            # unified cache; ContentCache vs PersistentCache is now a
            # viewer-side policy only.
            server_params={
                "EnablePersistentCache": "1",  # ensure PersistentCache path is enabled
                # PersistentCacheMinRectSize keeps default unless overridden
            },
        )
        if not server_content.start() or not server_content.start_session(wm="openbox"):
            print("✗ FAIL: Content server failed")
            return 1

        server_viewer = VNCServer(display_viewer, port_viewer, "poc_viewerwin", artifacts, tracker, geometry="800x600", log_level="*:stderr:30", server_choice=server_mode)
        if not server_viewer.start() or not server_viewer.start_session(wm="openbox"):
            print("✗ FAIL: Viewer server failed")
            return 1

        print("✓ Servers ready")

        # Start viewer with PersistentCache (two-phase: populate then reconnect)
        print("\n[3/7] Starting viewer with PersistentCache (phase 1: populate)...")
        env = os.environ.copy()
        env["TIGERVNC_VIEWER_DEBUG_LOG"] = "1"
        env["DISPLAY"] = f":{display_viewer}"

        cache_dir = artifacts.get_sandboxed_cache_dir()

        def start_viewer(tag: str, viewer_log_path):
            with open(viewer_log_path, "w") as log_file:
                proc = subprocess.Popen(
                    [binaries["cpp_viewer"], f"127.0.0.1::{port_content}", "Shared=1", "Log=*:stderr:100", "PersistentCache=1", f"PersistentCachePath={cache_dir}"],
                    stdout=log_file,
                    stderr=subprocess.STDOUT,
                    preexec_fn=os.setpgrp,
                    env=env,
                )
            tracker.register(tag, proc)
            time.sleep(2.0)
            return proc

        def run_scenario(label: str):
            print(f"\n[4/7] Phase {label}: Generating repeated static content (tiled logos)...")
            runner = StaticScenarioRunner(display_content, verbose=False)
            stats = runner.tiled_logos_test(tiles=12, duration=60.0, delay_between=3.0)
            print(f" Phase {label} scenario completed: {stats}")

        viewer_log_p1 = artifacts.logs_dir / "poc_viewer_phase1.log"
        p1 = start_viewer("poc_viewer_p1", viewer_log_p1)
        if p1.poll() is not None:
            print("✗ FAIL: Viewer exited (phase 1)")
            return 1
        print("✓ Viewer connected (phase 1)")
        run_scenario("1")
        tracker.cleanup("poc_viewer_p1")
        time.sleep(1.0)

        print("\n[5/7] Starting viewer with PersistentCache (phase 2: expect hits)...")
        viewer_log_p2 = artifacts.logs_dir / "poc_viewer_phase2.log"
        p2 = start_viewer("poc_viewer_p2", viewer_log_p2)
        if p2.poll() is not None:
            print("✗ FAIL: Viewer exited (phase 2)")
            return 1
        print("✓ Viewer connected (phase 2)")
        run_scenario("2")
        tracker.cleanup("poc_viewer_p2")
        time.sleep(1.0)

        print("\n[6/7] Analyzing results...")
        server_log = artifacts.logs_dir / f"poc_content_server_{display_content}.log"
        print(" Parsing logs...")
        pv1 = parse_cpp_log(viewer_log_p1)
        pv2 = parse_cpp_log(viewer_log_p2)
        parse_server_log(server_log, verbose=True)

        def pc_stats(parsed):
            hits = parsed.persistent_hits
            misses = parsed.persistent_misses
            lookups = hits + misses
            # Fallback: viewer reports PersistentCache protocol ops as plain Lookups/Hits
            # in the Client-side PersistentCache statistics block.
            if lookups == 0 and getattr(parsed, "negotiated_persistentcache", False) and parsed.total_lookups > 0:
                hits = parsed.total_hits
                lookups = parsed.total_lookups
                misses = max(0, lookups - hits)
            return hits, misses, lookups

        p1_hits, p1_misses, p1_lookups = pc_stats(pv1)
        p2_hits, p2_misses, p2_lookups = pc_stats(pv2)
        p2_hit_rate = (100.0 * p2_hits / p2_lookups) if p2_lookups > 0 else 0.0

        print("\n[7/7] Results")
        print("=" * 70)
        print("PersistentCache Activity (phase 1 - populate):")
        print(f" Lookups: {p1_lookups}")
        print(f" Hits: {p1_hits}")
        print(f" Misses: {p1_misses}")

        print("\nPersistentCache Activity (phase 2 - reconnect):")
        print(f" Lookups: {p2_lookups}")
        print(f" Hits: {p2_hits}")
        print(f" Misses: {p2_misses}")
        print(f" Hit rate: {p2_hit_rate:.1f}%")

        print("\nLogs:")
        print(f" Viewer phase 1: {viewer_log_p1}")
        print(f" Viewer phase 2: {viewer_log_p2}")
        print(f" Server: {server_log}")

        if p2_hits > 0:
            print("\n✓ SUCCESS: Cache hits detected on phase 2!")
            print(f" PersistentCache is working with {p2_hits} hits on reconnect")
            return 0
        else:
            print("\n✗ FAIL: No PersistentCache hits on phase 2 (hits=0, lookups=", p2_lookups, ")")
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


if __name__ == "__main__":
    sys.exit(main())
