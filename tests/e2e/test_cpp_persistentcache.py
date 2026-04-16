#!/usr/bin/env python3
"""
End-to-end test: C++ viewer PersistentCache functionality (STRICT gate).

This is a strict performance gate for disk-backed PersistentCache persistence across
sessions/connections. It performs a cold populate phase, restarts the viewer using
the same PersistentCachePath, then demands ultra-effective cache performance.

TEMPORARY STRICT GATE (April 2026): This test is intentionally hardened while the
PersistentCache implementation stabilises. Once it is reliably green across
environments, consider relaxing thresholds slightly to reduce flakiness (but do
not revert to weak defaults).
"""

import sys
import time
import argparse
import subprocess
import os
import re
import shutil
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only,
    PreflightError,
    ArtifactManager,
    ProcessTracker,
    VNCServer,
    check_port_available,
    check_display_available,
    BUILD_DIR,
)
from scenarios_static import StaticScenarioRunner
from log_parser import parse_cpp_log, parse_server_log, compute_metrics


def run_cpp_viewer(
    viewer_path,
    port,
    artifacts,
    tracker,
    name,
    cache_size_mb=256,
    disk_cache_mb=4096,
    display_for_viewer=None,
    cache_dir=None,
):
    """Run C++ viewer with PersistentCache enabled (disk-backed)."""
    cmd = [
        viewer_path,
        f"127.0.0.1::{port}",
        "Shared=1",
        "Log=*:stderr:100",
        "PreferredEncoding=ZRLE",  # strict gate: deterministic/lossless
        "PersistentCache=1",
        f"PersistentCacheSize={cache_size_mb}",
        f"PersistentCacheDiskSize={disk_cache_mb}",
    ]

    if cache_dir:
        cmd.append(f"PersistentCachePath={cache_dir}")

    log_path = artifacts.logs_dir / f"{name}.log"
    env = os.environ.copy()
    env["TIGERVNC_VIEWER_DEBUG_LOG"] = "1"

    if display_for_viewer is not None:
        env["DISPLAY"] = f":{display_for_viewer}"
    else:
        env.pop("DISPLAY", None)

    print(f" Starting {name} with PersistentCache mem={cache_size_mb}MB disk={disk_cache_mb}MB...")
    log_file = open(log_path, "w")
    proc = subprocess.Popen(
        cmd,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        preexec_fn=os.setpgrp,
        env=env,
    )
    tracker.register(name, proc)
    time.sleep(2.0)
    return proc


def find_saved_index_entries(viewer_log: Path):
    """Return N from 'PersistentCache: saved vX index with N entries' if present."""
    saved = None
    with open(viewer_log, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            if "PersistentCache: saved v" in line and "index with" in line:
                m = re.search(r"index with\s+(\d+)\s+entries", line)
                if m:
                    saved = int(m.group(1))
    return saved


def viewer_loaded_index(viewer_log: Path) -> bool:
    """True if log shows index.dat being loaded (proof of disk persistence on restart)."""
    with open(viewer_log, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            low = line.lower()
            if "loading index from" in low and "index.dat" in low:
                return True
    return False


def main():
    parser = argparse.ArgumentParser(description="STRICT: C++ PersistentCache cross-session performance gate")
    parser.add_argument("--display-content", type=int, default=998)
    parser.add_argument("--port-content", type=int, default=6898)
    parser.add_argument("--display-viewer", type=int, default=999)
    parser.add_argument("--port-viewer", type=int, default=6899)

    # 10-minute budget friendly defaults: 2 x 240s phases + setup/teardown
    parser.add_argument(
        "--phase1-duration",
        type=int,
        default=240,
        help="Cold populate duration (default: 240s)",
    )
    parser.add_argument(
        "--phase2-duration",
        type=int,
        default=240,
        help="Warm (after restart) duration (default: 240s)",
    )

    parser.add_argument("--cache-size", type=int, default=256)
    parser.add_argument(
        "--disk-cache-mb",
        type=int,
        default=4096,
        help="Disk cache size in MB (>0 required for persistence)",
    )
    parser.add_argument("--wm", default="openbox")
    parser.add_argument("--verbose", action="store_true")

    # STRICT defaults
    parser.add_argument(
        "--hit-rate-threshold",
        type=float,
        default=95.0,
        help="STRICT: minimum warm-phase hit rate percent (default: 95)",
    )
    parser.add_argument(
        "--bandwidth-threshold",
        type=float,
        default=98.0,
        help="STRICT: minimum warm-phase bandwidth reduction percent (default: 98)",
    )
    parser.add_argument(
        "--min-lookups",
        type=int,
        default=200,
        help="STRICT: minimum warm-phase PersistentCache lookups (default: 200)",
    )
    parser.add_argument(
        "--index-min-entries",
        type=int,
        default=1,
        help="STRICT: minimum index entries after cold phase (default: 1; proxy only — strictness is enforced by 0-miss warm phase)",
    )

    args = parser.parse_args()

    print("=" * 70)
    print("C++ Viewer PersistentCache STRICT cross-session gate")
    print("=" * 70)
    print(f"\nCache mem: {args.cache_size}MB")
    print(f"Cache disk: {args.disk_cache_mb}MB")
    print(f"Phase1: {args.phase1_duration}s, Phase2: {args.phase2_duration}s")
    print(f"Warm hit-rate threshold: {args.hit_rate_threshold}%")
    print(f"Warm bandwidth threshold: {args.bandwidth_threshold}%")
    print(f"Min lookups (warm): {args.min_lookups}")
    print(f"Min index entries (cold): {args.index_min_entries}")
    print()

    if args.disk_cache_mb <= 0:
        print("\n✗ FAIL: --disk-cache-mb must be > 0 for persistence across sessions")
        return 1

    # Artifacts
    print("[1/8] Setting up artifacts directory...")
    artifacts = ArtifactManager()
    artifacts.create()

    print("\n[2/8] Running preflight checks...")
    try:
        binaries = preflight_check_cpp_only(verbose=args.verbose)
    except PreflightError as e:
        print("\n✗ FAIL: Preflight checks failed")
        print(f"\n{e}")
        return 1

    # Ports/displays
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

    tracker = ProcessTracker()

    local_server_symlink = BUILD_DIR / "unix" / "vncserver" / "Xnjcvnc"
    local_server_actual = BUILD_DIR / "unix" / "xserver" / "hw" / "vnc" / "Xnjcvnc"
    if local_server_symlink.exists() or local_server_actual.exists():
        server_mode = "local"
        print("\nUsing local Xnjcvnc server")
    else:
        server_mode = "system"
        print("\nUsing system Xtigervnc server")

    try:
        print(f"\n[3/8] Starting content server (:{args.display_content})...")
        print(" Server config: PersistentCache-only (unified cache engine)")
        server_content = VNCServer(
            args.display_content,
            args.port_content,
            "cpp_pc_content",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:100",
            server_choice=server_mode,
            server_params={"EnablePersistentCache": "1"},
        )
        if not server_content.start():
            print("\n✗ FAIL: Could not start content server")
            return 1
        if not server_content.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start content server session")
            return 1
        print("✓ Content server ready")

        print(f"\n[4/8] Starting viewer window server (:{args.display_viewer})...")
        server_viewer = VNCServer(
            args.display_viewer,
            args.port_viewer,
            "cpp_pc_viewerwin",
            artifacts,
            tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        if not server_viewer.start():
            print("\n✗ FAIL: Could not start viewer window server")
            return 1
        if not server_viewer.start_session(wm=args.wm):
            print("\n✗ FAIL: Could not start viewer window server session")
            return 1
        print("✓ Viewer window server ready")

        # Cache dir (sandboxed)
        cache_dir = artifacts.get_sandboxed_cache_dir()
        print(f"\n[5/8] Using sandboxed cache dir: {cache_dir}")

        # Ensure cold start
        if cache_dir.exists():
            shutil.rmtree(cache_dir)
        cache_dir.mkdir(parents=True, exist_ok=True)

        # Phase 1 viewer
        print("\n[6/8] Phase 1: cold populate (disk-backed)...")
        proc1 = run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "cpp_pc_phase1_viewer",
            cache_size_mb=args.cache_size,
            disk_cache_mb=args.disk_cache_mb,
            display_for_viewer=args.display_viewer,
            cache_dir=str(cache_dir),
        )
        if proc1.poll() is not None:
            print("\n✗ FAIL: Phase 1 viewer exited prematurely")
            return 1

        runner = StaticScenarioRunner(args.display_content, verbose=args.verbose)

        # The tiled_logos_test() duration parameter is a final dwell after all logos
        # are displayed, not a repeated-workload budget. To make the strict
        # min-lookups threshold meaningful, drive a larger tiled-logo pass with a
        # short inter-logo delay (same pattern used by test_hash_collision_handling).
        strict_tile_delay = 0.25
        strict_tile_count = max(args.min_lookups + 20, 240)
        strict_final_dwell = 2.0
        print(f"  Strict tiled-logo workload: tiles={strict_tile_count}, delay={strict_tile_delay}s, final_dwell={strict_final_dwell}s")

        stats1 = runner.tiled_logos_test(
            tiles=strict_tile_count,
            duration=strict_final_dwell,
            delay_between=strict_tile_delay,
        )
        print(f" Phase 1 completed: {stats1}")
        time.sleep(3.0)

        tracker.cleanup("cpp_pc_phase1_viewer")
        time.sleep(2.0)

        phase1_log = artifacts.logs_dir / "cpp_pc_phase1_viewer.log"
        if not phase1_log.exists():
            print(f"\n✗ FAIL: Phase 1 viewer log not found: {phase1_log}")
            return 1

        index_path = cache_dir / "index.dat"
        if not index_path.exists():
            print(f"\n✗ FAIL: index.dat not created after cold phase: {index_path}")
            return 1

        saved_entries = find_saved_index_entries(phase1_log)
        if saved_entries is None:
            print("\n✗ FAIL: Phase 1 did not log 'saved ... index with N entries' (cannot prove persistence)")
            print(f" Viewer log: {phase1_log}")
            return 1
        if saved_entries < args.index_min_entries:
            print(f"\n✗ FAIL: Phase 1 index too small: {saved_entries} < {args.index_min_entries}")
            return 1
        print(f"✓ Phase 1 wrote disk index with {saved_entries} entries")

        # Phase 2 viewer (fresh process, same cache dir)
        print("\n[6/8] Phase 2: restart viewer and demand warm hits from disk cache...")
        proc2 = run_cpp_viewer(
            binaries["cpp_viewer"],
            args.port_content,
            artifacts,
            tracker,
            "cpp_pc_phase2_viewer",
            cache_size_mb=args.cache_size,
            disk_cache_mb=args.disk_cache_mb,
            display_for_viewer=args.display_viewer,
            cache_dir=str(cache_dir),
        )
        if proc2.poll() is not None:
            print("\n✗ FAIL: Phase 2 viewer exited prematurely")
            return 1

        stats2 = runner.tiled_logos_test(
            tiles=strict_tile_count,
            duration=strict_final_dwell,
            delay_between=strict_tile_delay,
        )
        print(f" Phase 2 completed: {stats2}")
        time.sleep(3.0)

        tracker.cleanup("cpp_pc_phase2_viewer")
        time.sleep(1.0)

        print("\n[7/8] Analyzing results (warm phase)...")
        viewer_log = artifacts.logs_dir / "cpp_pc_phase2_viewer.log"
        server_log = artifacts.logs_dir / f"cpp_pc_content_server_{args.display_content}.log"
        if not viewer_log.exists():
            print(f"\n✗ FAIL: Phase 2 viewer log not found: {viewer_log}")
            return 1

        if not viewer_loaded_index(viewer_log):
            print("\n✗ FAIL: Phase 2 did not load index.dat (no proof of cross-session persistence)")
            print(f" Viewer log: {viewer_log}")
            return 1
        print("✓ Phase 2 loaded index.dat (persistence proven)")

        parsed_v = parse_cpp_log(viewer_log)
        metrics_v = compute_metrics(parsed_v)
        pers_v = metrics_v["persistent"]

        if pers_v["hits"] == 0 and pers_v["misses"] == 0:
            print("\n✗ FAIL: PersistentCache protocol ops not observed in Phase 2 viewer log")
            print(f" Viewer log: {viewer_log}")
            return 1

        lookups = pers_v["hits"] + pers_v["misses"]
        hit_rate = pers_v["hit_rate"]

        parsed_s = parse_server_log(server_log, verbose=args.verbose)
        # Use server-side bandwidth reduction (viewer doesn't always log it)
        bw = parsed_s.persistent_bandwidth_reduction

        print("\n[8/8] Verification (STRICT)...")
        print("\nPersistentCache (warm phase):")
        print(f" Lookups: {lookups}")
        print(f" Hits: {pers_v['hits']} ({hit_rate:.1f}%)")
        print(f" Misses: {pers_v['misses']}")
        print(f" Bandwidth reduction (server): {bw:.1f}%")

        failures = []

        # STRICT duplicated workload requirement after reconnect:
        # Phase 2 must have zero PersistentCache misses and at least one hit.
        if pers_v["misses"] != 0:
            failures.append(f"PersistentCache warm phase had {pers_v['misses']} misses (expected 0)")
            failures.append("Likely causes: viewer did not advertise disk inventory on reconnect, or server did not emit stable canonical references for identical content")
        if pers_v["hits"] == 0:
            failures.append("PersistentCache warm phase had 0 hits (expected >0)")

        if lookups < args.min_lookups:
            failures.append(f"Too few lookups: {lookups} < {args.min_lookups}")
        if hit_rate < args.hit_rate_threshold:
            failures.append(f"Hit rate too low: {hit_rate:.1f}% < {args.hit_rate_threshold}%")
        if bw <= 0.0:
            failures.append("Bandwidth reduction not computed/logged (expected > 0 under strict gate)")
        elif bw < args.bandwidth_threshold:
            failures.append(f"Bandwidth reduction too low: {bw:.1f}% < {args.bandwidth_threshold}%")

        print("\nARTIFACTS")
        print(f" Logs: {artifacts.logs_dir}")
        print(f" Phase1 viewer log: {phase1_log}")
        print(f" Phase2 viewer log: {viewer_log}")
        print(f" Content server log: {server_log}")
        print(f" Cache dir: {cache_dir}")

        if failures:
            print("\n✗ TEST FAILED (STRICT)")
            for f in failures:
                print(f" • {f}")
            return 1

        print("\n✓ TEST PASSED (STRICT)")
        return 0

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


if __name__ == "__main__":
    sys.exit(main())
