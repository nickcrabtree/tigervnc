"""Pytest wrappers for C++ cache e2e scripts.

The C++ cache e2e tests in this repo are implemented as executable scripts:

    - tests/e2e/test_cpp_persistentcache.py
    - tests/e2e/test_cpp_contentcache.py

Those scripts are intentionally *not* structured as pytest tests, so running
pytest directly on them will collect zero items.

This module provides lightweight pytest entry points that execute the scripts
as subprocesses. They are marked as e2e and are intended to run on quartz/Linux.
"""

from __future__ import annotations

import os
import platform
import subprocess
import sys
from pathlib import Path

import pytest


def _run_script(script: Path, args: list[str], timeout_s: int) -> None:
    env = os.environ.copy()
    # Ensure the scripts can import their local e2e framework modules.
    env["PYTHONPATH"] = str(script.parent) + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
    proc = subprocess.run(
        [sys.executable, str(script), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=env,
        timeout=timeout_s,
    )
    if proc.returncode != 0:
        # Include the full combined output to make quartz failures actionable.
        raise AssertionError(f"e2e script failed: {script.name} rc={proc.returncode}\n\n{proc.stdout}")


@pytest.mark.e2e
def test_cpp_persistentcache_strict_gate() -> None:
    """Run the STRICT cross-session PersistentCache gate (C++ viewer)."""
    if platform.system() != "Linux":
        pytest.skip("e2e tests are intended to run on quartz/Linux")
    script = Path(__file__).with_name("test_cpp_persistentcache.py")
    # Default script runtime is ~10 minutes (2 x 240s phases + setup/teardown).
    _run_script(script, args=[], timeout_s=15 * 60)


@pytest.mark.e2e
def test_cpp_contentcache_unified_cache_path() -> None:
    """Run the historical ContentCache scenario via the unified cache engine."""
    if platform.system() != "Linux":
        pytest.skip("e2e tests are intended to run on quartz/Linux")
    script = Path(__file__).with_name("test_cpp_contentcache.py")
    # Default duration is 60s; allow extra for server/viewer startup.
    _run_script(script, args=[], timeout_s=5 * 60)
