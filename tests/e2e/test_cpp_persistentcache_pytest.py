"""Pytest wrapper for the C++ PersistentCache strict e2e gate.

The underlying implementation lives in tests/e2e/test_cpp_persistentcache.py as a
script-style driver (argparse + main()). Pytest will not collect it because it
defines no test_* functions.

This wrapper makes the strict gate discoverable and runnable under pytest.

By default it is skipped on non-Linux platforms because the C++ e2e stack is
expected to run on quartz/Linux (Xvfb/Openbox, local X server build paths, etc.).
"""

from __future__ import annotations

import os
import sys
import subprocess
from pathlib import Path

import pytest


def _env_int(name: str, default: int) -> int:
    val = os.environ.get(name)
    if val is None or val == "":
        return default
    try:
        return int(val)
    except ValueError:
        return default


@pytest.mark.e2e
def test_cpp_persistentcache_strict_gate() -> None:
    if sys.platform != "linux":
        pytest.skip("C++ PersistentCache e2e gate is run on quartz/Linux")

    script = Path(__file__).with_name("test_cpp_persistentcache.py")
    assert script.exists(), f"missing script: {script}"

    phase1 = _env_int("PC_PHASE1_DURATION", 240)
    phase2 = _env_int("PC_PHASE2_DURATION", 240)

    cmd = [
        sys.executable,
        str(script),
        "--phase1-duration",
        str(phase1),
        "--phase2-duration",
        str(phase2),
    ]

    # Optional overrides for parallelism / local debugging.
    for opt, env in [
        ("--display-content", "PC_DISPLAY_CONTENT"),
        ("--port-content", "PC_PORT_CONTENT"),
        ("--display-viewer", "PC_DISPLAY_VIEWER"),
        ("--port-viewer", "PC_PORT_VIEWER"),
        ("--cache-size", "PC_CACHE_SIZE_MB"),
        ("--disk-cache-mb", "PC_DISK_CACHE_MB"),
    ]:
        if os.environ.get(env):
            cmd.extend([opt, os.environ[env]])

    proc = subprocess.run(cmd)
    assert proc.returncode == 0, f"strict gate failed with rc={proc.returncode}"
