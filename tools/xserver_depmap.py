#!/usr/bin/env python3
"""
Maintain and use a static dependency map for the Xorg-based Xnjcvnc server.

This script has two primary subcommands:

  * refresh  - ensure the depmap exists and is not older than a threshold.
               If missing/stale, run a full xserver rebuild with verbose
               output and derive a mapping of source files to object files.

  * sync     - rsync unix/xserver/ into the build tree, then use the depmap
               to delete only the objects corresponding to changed sources
               so that a subsequent `make -C build/unix/xserver` will do an
               incremental rebuild instead of requiring a full clean.

The intent is to replace the previous "sledgehammer" strategy used by the
`server` target in the top-level Makefile with a more targeted incremental
rebuild while still remaining robust in the presence of incomplete autotools
dependency tracking inside the Xorg tree.

The dependency map is stored as JSON in:

    build/unix/xserver/.tigervnc_depmap.json

and maps relative source paths (e.g. "dix/dispatch.c") to a list of relative
object paths (e.g. ["dix/dispatch.o", "dix/.libs/dispatch.o"]).
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List

PROJECT_ROOT = Path(__file__).resolve().parent.parent
BUILD_DIR = PROJECT_ROOT / os.environ.get("BUILD_DIR", "build")
XSERVER_BUILD_DIR = BUILD_DIR / "unix" / "xserver"
XSERVER_SRC_DIR = PROJECT_ROOT / "unix" / "xserver"
DEPMAP_PATH = XSERVER_BUILD_DIR / ".tigervnc_depmap.json"
# Refresh depmap if older than 30 days
DEPMAP_MAX_AGE_SECS = 30 * 24 * 60 * 60


def _run(cmd: List[str], cwd: Path | None = None) -> str:
    """Run a command and return its stdout as text, raising on failure."""
    proc = subprocess.run(
        cmd,
        cwd=str(cwd) if cwd is not None else None,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=True,
    )
    return proc.stdout


def depmap_is_fresh() -> bool:
    """Return True if the depmap exists and is younger than the max age."""
    if not DEPMAP_PATH.exists():
        return False
    try:
        mtime = DEPMAP_PATH.stat().st_mtime
    except OSError:
        return False
    return (time.time() - mtime) <= DEPMAP_MAX_AGE_SECS


def parse_build_log_for_depmap(log_text: str) -> Dict[str, List[str]]:
    """Parse a verbose xserver build log into a source->objects mapping.

    We look for libtool compile lines of the general form:

        libtool: compile:  gcc ... -c foo/bar.c ... -o .libs/bar.o

    or non-libtool compile lines:

        gcc ... -c dix/dispatch.c ... -o dix/dispatch.o

    For each such line we derive a relative source path from the xserver
    build dir and record the corresponding object path(s).
    """
    import shlex

    depmap: Dict[str, List[str]] = {}

    for line in log_text.splitlines():
        line = line.strip()
        if not line:
            continue

        # Fast-path filter: only lines that look like compilation
        if " -c " not in line or ".c" not in line:
            continue

        # Drop a leading "libtool: compile:" prefix if present
        if "libtool: compile:" in line:
            # Split once at the first colon after "libtool"
            parts = line.split("libtool: compile:", 1)
            if len(parts) == 2:
                line = parts[1].strip()

        try:
            args = shlex.split(line)
        except ValueError:
            # Malformed shell quoting; skip conservatively
            continue

        src_path: Path | None = None
        obj_path: Path | None = None

        # Walk args, look for "-c <src>" and "-o <obj>"
        it = iter(range(len(args)))
        for i in it:
            arg = args[i]
            if arg == "-c" and i + 1 < len(args):
                cand = Path(args[i + 1])
                # Only care about C sources under the xserver tree
                if str(cand).endswith(".c"):
                    src_path = cand
                next(it, None)  # skip value index
            elif arg == "-o" and i + 1 < len(args):
                obj_path = Path(args[i + 1])
                next(it, None)

        if src_path is None or obj_path is None:
            continue

        # Normalize to paths relative to XSERVER_BUILD_DIR
        # Compilation usually happens with cwd set to some subdir of
        # XSERVER_BUILD_DIR, so relative paths are interpreted from there.
        if not src_path.is_absolute():
            src_full = (XSERVER_BUILD_DIR / src_path).resolve()
        else:
            src_full = src_path

        try:
            rel_src = src_full.relative_to(XSERVER_BUILD_DIR)
        except ValueError:
            # Not under the build xserver dir; ignore
            continue

        if not obj_path.is_absolute():
            obj_full = (XSERVER_BUILD_DIR / obj_path).resolve()
        else:
            obj_full = obj_path

        try:
            rel_obj = obj_full.relative_to(XSERVER_BUILD_DIR)
        except ValueError:
            continue

        key = str(rel_src).replace(os.sep, "/")
        val = str(rel_obj).replace(os.sep, "/")
        depmap.setdefault(key, [])
        if val not in depmap[key]:
            depmap[key].append(val)

    return depmap


def refresh_depmap(force: bool = False) -> None:
    """Ensure the dependency map exists and is recent.

    If the map is missing or stale (older than DEPMAP_MAX_AGE_SECS) or
    ``force`` is True, we run a full rebuild of the xserver tree with
    verbose output and derive the mapping from the resulting build log.
    """
    if not force and depmap_is_fresh():
        print("[depmap] Existing depmap is fresh; skipping regeneration")
        return

    if not XSERVER_BUILD_DIR.exists():
        raise SystemExit(
            f"X server build dir does not exist: {XSERVER_BUILD_DIR}.\n"
            "Make sure you have set up unix/xserver under build/unix/xserver "
            "per BUILDING.txt."
        )

    print("[depmap] Regenerating Xserver dependency map (full rebuild)...")

    # Clean the xserver tree, but leave the CMake libs alone. The user may
    # still choose to clean the CMake build explicitly via other targets.
    subprocess.run(
        ["make", "-C", str(XSERVER_BUILD_DIR), "clean"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    # Perform a full rebuild with captured output for parsing.
    log_text = _run(["make", "-C", str(XSERVER_BUILD_DIR)], cwd=None)

    depmap = parse_build_log_for_depmap(log_text)
    if not depmap:
        raise SystemExit("[depmap] Failed to derive any dependencies from build log")

    DEPMAP_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(DEPMAP_PATH, "w", encoding="utf-8") as f:
        json.dump(depmap, f, indent=2, sort_keys=True)

    print(f"[depmap] Wrote dependency map for {len(depmap)} sources to {DEPMAP_PATH}")


def load_depmap() -> Dict[str, List[str]]:
    if not DEPMAP_PATH.exists():
        raise SystemExit(
            f"Dependency map not found: {DEPMAP_PATH}.\n"
            "Run `make server` (which will refresh the depmap) or invoke\n"
            "  python3 tools/xserver_depmap.py refresh\n"
            "explicitly first."
        )
    with open(DEPMAP_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


def sync_and_invalidate(verbose: bool = True) -> None:
    """Rsync unix/xserver into the build tree and invalidate stale objects.

    This function:
      1. Runs rsync with --checksum and --itemize-changes from the source
         unix/xserver tree into build/unix/xserver.
      2. Loads the depmap and, for each changed .c or .h file, deletes the
         corresponding object files recorded in the map.

    After this completes, a plain `make -C build/unix/xserver` performs an
    incremental rebuild that is robust against the known dependency gaps.
    """
    if not XSERVER_SRC_DIR.exists():
        raise SystemExit(f"Source xserver dir does not exist: {XSERVER_SRC_DIR}")

    XSERVER_BUILD_DIR.mkdir(parents=True, exist_ok=True)

    # 1. Rsync with itemized changes
    rsync_cmd = [
        "rsync",
        "-a",
        "--checksum",
        "--itemize-changes",
        f"{XSERVER_SRC_DIR}/",
        f"{XSERVER_BUILD_DIR}/",
    ]
    print("[depmap] Syncing unix/xserver → build/unix/xserver (rsync)...")
    proc = subprocess.run(
        rsync_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=True,
    )

    changed_rel_paths: List[str] = []
    for line in proc.stdout.splitlines():
        line = line.rstrip("\n")
        if not line:
            continue
        # rsync --itemize-changes format: "<tag><tag>... path"
        # e.g. ">f+++++++++ dix/dispatch.c"
        parts = line.split(maxsplit=1)
        if len(parts) != 2:
            continue
        tag, rel = parts
        # Only treat lines that modify files (start with '>' or 'c')
        if not tag or tag[0] not in (">", "c"):
            continue
        rel_path = rel.strip()
        if rel_path.endswith(".c") or rel_path.endswith(".h"):
            changed_rel_paths.append(rel_path.replace(os.sep, "/"))

    if verbose:
        if changed_rel_paths:
            print(f"[depmap] Changed xserver sources: {len(changed_rel_paths)}")
        else:
            print("[depmap] No xserver source/header changes detected")

    if not changed_rel_paths:
        return

    depmap = load_depmap()

    removed = 0
    for rel_src in changed_rel_paths:
        objs = depmap.get(rel_src)
        if not objs:
            # Fallback: if this is a header, remove all objects in its directory
            if rel_src.endswith(".h"):
                dir_rel = os.path.dirname(rel_src)
                dir_path = XSERVER_BUILD_DIR / dir_rel
                if dir_path.is_dir():
                    for entry in dir_path.iterdir():
                        if entry.suffix in {".o", ".lo"}:
                            try:
                                entry.unlink()
                                removed += 1
                            except OSError:
                                pass
            continue

        for obj_rel in objs:
            obj_path = XSERVER_BUILD_DIR / obj_rel
            if obj_path.exists():
                try:
                    obj_path.unlink()
                    removed += 1
                except OSError:
                    pass

    if verbose:
        print(f"[depmap] Removed {removed} object files for {len(changed_rel_paths)} changed sources/headers")


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description="Maintain and use Xnjcvnc depmap")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_refresh = sub.add_parser("refresh", help="Refresh dependency map if stale")
    p_refresh.add_argument("--force", action="store_true", help="Force regeneration even if fresh")

    p_sync = sub.add_parser("sync", help="Rsync xserver and invalidate stale objects using depmap")
    p_sync.add_argument("--quiet", action="store_true", help="Reduce logging output")

    args = parser.parse_args(argv)

    if args.cmd == "refresh":
        refresh_depmap(force=args.force)
        return 0
    elif args.cmd == "sync":
        sync_and_invalidate(verbose=not args.quiet)
        return 0

    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
