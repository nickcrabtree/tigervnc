#!/usr/bin/env python3
"""
Framework for VNC end-to-end testing infrastructure.

Provides:
- VNC server lifecycle management (Xtigervnc)
- Process group tracking and cleanup
- Environment preflight checks
- Artifact directory management
"""

import os
import sys
import time
import socket
import subprocess
import signal
import shutil
from pathlib import Path
from typing import Optional, Dict, List, Tuple
from datetime import datetime

# Project root detection
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent
BUILD_DIR = PROJECT_ROOT / os.environ.get("BUILD_DIR", "build")

# Artifact directories
ARTIFACTS_BASE = Path(__file__).parent / "_artifacts"


class PreflightError(Exception):
    """Raised when preflight checks fail."""
    pass


class ProcessTracker:
    """Track all processes we start for safe cleanup."""
    
    def __init__(self):
        self.processes: Dict[str, subprocess.Popen] = {}
        self.pgids: Dict[str, int] = {}
    
    def register(self, name: str, proc: subprocess.Popen):
        """Register a process we own."""
        self.processes[name] = proc
        try:
            pgid = os.getpgid(proc.pid)
            self.pgids[name] = pgid
        except ProcessLookupError:
            pass  # Process already exited
    
    def cleanup(self, name: str, timeout: float = 5.0):
        """Gracefully terminate a process tree."""
        if name not in self.processes:
            return
        
        proc = self.processes[name]
        if proc.poll() is not None:
            # Already exited
            return
        
        pgid = self.pgids.get(name)
        if pgid:
            try:
                # Send SIGTERM to entire process group
                os.killpg(pgid, signal.SIGTERM)
                try:
                    proc.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    # Force kill if still alive
                    os.killpg(pgid, signal.SIGKILL)
                    proc.wait(timeout=1.0)
            except ProcessLookupError:
                pass  # Already gone
        else:
            # Fallback to single process
            proc.terminate()
            try:
                proc.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=1.0)
    
    def cleanup_all(self):
        """Clean up all tracked processes."""
        for name in list(self.processes.keys()):
            self.cleanup(name)

        # After attempting graceful cleanup, give the system a brief
        # moment to tear down listening sockets before subsequent tests.
        time.sleep(0.5)


def check_binary(name: str, required: bool = True) -> Optional[str]:
    """Check if a binary exists in PATH."""
    path = shutil.which(name)
    if required and path is None:
        raise PreflightError(f"Required binary not found: {name}")
    return path


def check_port_available(port: int) -> bool:
    """Check if a TCP port is available.

    For the dedicated test ports (6898 → :998, 6899 → :999), this will also
    perform a one-shot best-effort cleanup of any orphaned test VNC servers
    before giving up. This helps ensure that a crashed or interrupted test
    run does not permanently block subsequent tests.
    """
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("127.0.0.1", port))
            return True
    except OSError:
        # For known test ports, attempt to clean up any leftover test servers
        # and retry once.
        display = None
        if port == 6898:
            display = 998
        elif port == 6899:
            display = 999

        if display is not None:
            best_effort_cleanup_test_server(display, port, verbose=False)
            time.sleep(0.5)
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    s.bind(("127.0.0.1", port))
                    return True
            except OSError:
                return False

        return False


def check_display_available(display: int) -> bool:
    """Check if an X display number is available.

    Note: We treat a display as unavailable if either the X11 UNIX domain
    socket or the legacy lock file exists. This helps avoid stale X
    artifacts causing false "in use" reports across test runs.
    """
    socket_path = Path(f"/tmp/.X11-unix/X{display}")
    lock_path = Path(f"/tmp/.X{display}-lock")
    return not socket_path.exists() and not lock_path.exists()


def wait_for_tcp_port(host: str, port: int, timeout: float = 10.0) -> bool:
    """Wait for a TCP port to become available."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.settimeout(1.0)
                s.connect((host, port))
                return True
        except (socket.timeout, ConnectionRefusedError, OSError):
            time.sleep(0.2)
    return False


def wait_for_x_display(display: int, timeout: float = 10.0) -> bool:
    """Wait for X display socket to appear."""
    socket_path = Path(f"/tmp/.X11-unix/X{display}")
    deadline = time.time() + timeout
    while time.time() < deadline:
        if socket_path.exists():
            return True
        time.sleep(0.2)
    return False


def best_effort_cleanup_test_server(display: int, port: int, verbose: bool = False) -> None:
    """Best-effort cleanup for orphaned test VNC servers on :998/:999.

    This is used when a test detects that a known test port (e.g. 6898/6899)
    is already in use, which typically indicates a previous test run crashed
    or was interrupted and left an Xtigervnc/Xnjcvnc instance running on one
    of the dedicated test displays.

    Safety:
    - Only touches displays 998 and 999 (reserved for tests).
    - Only considers processes whose command line contains both
      "Xtigervnc"/"Xnjcvnc" and the exact display string ":<display>".
    - Kills by specific PID only, never using pkill/killall.

    Robustness improvements:
    - In addition to scanning "ps aux" output, also inspects the specific
      TCP port (when possible) to discover any Xtigervnc/Xnjcvnc instances
      that may not be visible due to truncated ps output.
    """
    if display not in (998, 999):
        return

    pattern = f":{display}"

    pids_to_kill: List[int] = []

    # 1) Discover candidate PIDs via ps aux (display-based matching).
    try:
        result = subprocess.run(
            ["ps", "aux"],
            capture_output=True,
            text=True,
            timeout=5.0,
        )
    except Exception:
        result = None

    if result is not None and result.returncode == 0:
        for line in result.stdout.splitlines():
            if ("Xtigervnc" in line or "Xnjcvnc" in line) and pattern in line:
                parts = line.split()
                if len(parts) < 2:
                    continue
                try:
                    pid = int(parts[1])
                except ValueError:
                    continue
                if pid not in pids_to_kill:
                    pids_to_kill.append(pid)

    # 2) Optionally, refine with port-based discovery when tools are available.
    #    This catches cases where ps output is truncated and the display
    #    string is not visible even though the process is still bound to the
    #    expected TCP port.
    try:
        import shutil as _shutil

        lsof_path = _shutil.which("lsof")
    except Exception:
        lsof_path = None

    if lsof_path and port:
        try:
            lsof_result = subprocess.run(
                [lsof_path, "-nP", f"-iTCP:{port}", "-sTCP:LISTEN"],
                capture_output=True,
                text=True,
                timeout=5.0,
            )
        except Exception:
            lsof_result = None

        if lsof_result is not None and lsof_result.returncode == 0:
            for line in lsof_result.stdout.splitlines():
                # Skip header lines (they usually start with COMMAND)
                if line.startswith("COMMAND"):
                    continue
                parts = line.split()
                if len(parts) < 2:
                    continue
                try:
                    pid = int(parts[1])
                except ValueError:
                    continue

                # Confirm this PID really is one of our test VNC servers by
                # inspecting its full command line and checking for both the
                # binary name and the expected display string.
                try:
                    ps_result = subprocess.run(
                        ["ps", "-p", str(pid), "-o", "pid,args="],
                        capture_output=True,
                        text=True,
                        timeout=5.0,
                    )
                except Exception:
                    continue

                if ps_result.returncode != 0:
                    continue

                cmdline = ps_result.stdout.strip()
                if not cmdline:
                    continue

                if ("Xtigervnc" in cmdline or "Xnjcvnc" in cmdline) and pattern in cmdline:
                    if pid not in pids_to_kill:
                        pids_to_kill.append(pid)

    if not pids_to_kill:
        return

    if verbose:
        print(f"⚠ Cleaning up orphaned test VNC servers on :{display} (PIDs: {pids_to_kill})")

    for pid in pids_to_kill:
        try:
            # Try to terminate the specific server PID gracefully first.
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
        except PermissionError:
            continue

    # Give them a moment to exit and release the port.
    time.sleep(1.0)

    # Force-kill any stubborn processes.
    for pid in pids_to_kill:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            continue  # already gone
        except PermissionError:
            continue
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            continue
        except PermissionError:
            continue

    # Best-effort cleanup of stale X11 socket/lock files for this display,
    # mirroring the logic used in some individual tests.
    try:
        socket_path = Path(f"/tmp/.X11-unix/X{display}")
        lock_path = Path(f"/tmp/.X{display}-lock")
        if socket_path.exists():
            socket_path.unlink()
        if lock_path.exists():
            lock_path.unlink()
    except Exception:
        # Ignore filesystem cleanup errors; the important part is killing
        # the actual server process so the TCP port is freed.
        pass


def timestamped_log_path(artifacts_dir: Path, role: str, suffix: str) -> Path:
    """Generate timestamped log path."""
    return artifacts_dir / "logs" / f"{role}_{suffix}"


class ArtifactManager:
    """Manage test artifact directories."""
    
    def __init__(self):
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        self.base_dir = ARTIFACTS_BASE / timestamp
        self.logs_dir = self.base_dir / "logs"
        self.screenshots_dir = self.base_dir / "screenshots"
        self.reports_dir = self.base_dir / "reports"
    
    def create(self):
        """Create artifact directory structure."""
        self.base_dir.mkdir(parents=True, exist_ok=True)
        self.logs_dir.mkdir(exist_ok=True)
        self.screenshots_dir.mkdir(exist_ok=True)
        self.reports_dir.mkdir(exist_ok=True)
        print(f"Artifacts will be saved to: {self.base_dir}")


def _run_build_target(target: str, description: str, timeout: float = 600.0, verbose: bool = False) -> None:
    """Run a top-level make target to build a component needed for tests.

    This is a best-effort helper used by preflight checks to avoid failing
    just because the viewer binaries have not been built yet.
    """
    cmd = ["make", target]
    if verbose:
        print(f"Attempting to build {description} via: {' '.join(cmd)}")
    try:
        subprocess.run(
            cmd,
            cwd=str(PROJECT_ROOT),
            check=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        raise PreflightError(
            f"Building {description} timed out after {int(timeout)}s. "
            f"Command: {' '.join(cmd)}"
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise PreflightError(
            f"Failed to build {description} using {' '.join(cmd)}: {exc}"
        )


def _ensure_cpp_viewer(binaries: Dict[str, str], verbose: bool = False) -> None:
    """Ensure the C++ viewer binary exists, building it on demand if needed.

    If the environment variable ``TIGERVNC_VIEWER_BIN`` is set, it takes
    precedence and is used as the viewer under test. This allows the same
    e2e tests to be reused for alternative viewer implementations (for
    example, the Rust viewer) without changing the test code.
    """
    override = os.environ.get("TIGERVNC_VIEWER_BIN")
    if override:
        binaries["cpp_viewer"] = override
        if verbose:
            print(f"✓ C++ viewer (overridden by $TIGERVNC_VIEWER_BIN): {override}")
        return

    cpp_viewer = BUILD_DIR / "vncviewer" / "njcvncviewer"
    if not cpp_viewer.exists():
        _run_build_target("viewer", "C++ viewer", verbose=verbose)
    if not cpp_viewer.exists():
        raise PreflightError(
            f"C++ viewer not found after attempting to build it: {cpp_viewer}\n"
            "Check your build configuration or run 'make viewer' manually."
        )
    binaries["cpp_viewer"] = str(cpp_viewer)
    if verbose:
        print(f"✓ C++ viewer: {cpp_viewer}")


def _ensure_rust_viewer(binaries: Dict[str, str], verbose: bool = False) -> None:
    """Ensure the Rust viewer binary exists, building it on demand if needed.

    If the environment variable ``TIGERVNC_RUST_VIEWER_BIN`` is set, it
    overrides the automatically detected/build output path. This mirrors the
    behaviour of ``TIGERVNC_VIEWER_BIN`` for the C++ viewer and lets higher-
    level harnesses control exactly which Rust binary is exercised.
    """
    override = os.environ.get("TIGERVNC_RUST_VIEWER_BIN")
    if override:
        binaries["rust_viewer"] = override
        if verbose:
            print(f"✓ Rust viewer (overridden by $TIGERVNC_RUST_VIEWER_BIN): {override}")
        return

    rust_viewer_symlink = BUILD_DIR / "vncviewer" / "njcvncviewer-rs"
    rust_viewer_direct = (
        PROJECT_ROOT / "rust-vnc-viewer" / "target" / "release" / "njcvncviewer-rs"
    )

    rust_viewer = None
    if rust_viewer_symlink.exists():
        rust_viewer = rust_viewer_symlink
    elif rust_viewer_direct.exists():
        rust_viewer = rust_viewer_direct
    else:
        _run_build_target("rust_viewer", "Rust viewer", verbose=verbose)
        if rust_viewer_symlink.exists():
            rust_viewer = rust_viewer_symlink
        elif rust_viewer_direct.exists():
            rust_viewer = rust_viewer_direct

    if rust_viewer is None:
        raise PreflightError(
            "Rust viewer not found after attempting to build it. "
            "Ensure 'make rust_viewer' succeeds and that njcvncviewer-rs is available."
        )

    binaries["rust_viewer"] = str(rust_viewer)
    if verbose:
        print(f"✓ Rust viewer: {rust_viewer}")


def _get_latest_git_commit_timestamp() -> Optional[float]:
    """Return the UNIX timestamp of the most recent git commit, or None.

    This is used to decide whether local binaries (like the Xnjcvnc server)
    are stale and should be rebuilt before running tests.
    """
    try:
        result = subprocess.run(
            ["git", "log", "-1", "--format=%ct"],
            cwd=str(PROJECT_ROOT),
            capture_output=True,
            text=True,
            timeout=5.0,
        )
    except OSError:
        return None

    if result.returncode != 0:
        return None

    output = result.stdout.strip()
    if not output:
        return None

    try:
        return float(output)
    except ValueError:
        return None


def _maybe_rebuild_local_server(server_path: Path, verbose: bool = False) -> None:
    """Rebuild local Xnjcvnc server if it is older than the latest git commit.

    This is a best-effort helper. If rebuilding fails, the caller will simply
    proceed with whatever server binary is available (local or system).
    """
    if not server_path.exists():
        # Nothing to compare; respect the fact that the user may not have
        # set up the Xnjcvnc build environment at all.
        return

    head_ts = _get_latest_git_commit_timestamp()
    if head_ts is None:
        return

    try:
        mtime = server_path.stat().st_mtime
    except OSError:
        # If we can't stat the file, don't attempt cleverness here.
        return

    if mtime >= head_ts:
        # Server is at least as new as the latest commit; nothing to do.
        return

    # At this point the server binary appears stale relative to the repo.
    # Try to rebuild it via the top-level Makefile.
    try:
        _run_build_target("server", "Xnjcvnc server", timeout=1200.0, verbose=verbose)
    except PreflightError as exc:
        # Best-effort only: print a warning and continue. Tests that
        # explicitly require the local server can still fail later with
        # a clear error if the binary is unusable.
        if verbose:
            print(f"⚠ Warning: auto-rebuild of local Xnjcvnc server failed: {exc}")


def preflight_check(verbose: bool = False) -> Dict[str, str]:
    """
    Run preflight checks for required dependencies.
    
    Returns dict of binary paths.
    Raises PreflightError if critical requirements missing.
    """
    binaries = {}
    
    # Required binaries
    required = [
        ('Xtigervnc', 'System TigerVNC server (install tigervnc-standalone-server or tigervnc-server)'),
        ('xterm', 'Terminal emulator (install xterm)'),
        ('openbox', 'Window manager (install openbox)'),
        ('xsetroot', 'X11 utilities (install x11-xserver-utils)'),
        ('wmctrl', 'Window manager control (install wmctrl)'),
        ('xdotool', 'X11 automation (install xdotool)'),
    ]
    
    missing = []
    for binary, description in required:
        try:
            path = check_binary(binary, required=True)
            binaries[binary] = path
            if verbose:
                print(f"✓ Found {binary}: {path}")
        except PreflightError as e:
            missing.append(f"  - {binary}: {description}")
    
    if missing:
        msg = "Missing required binaries:\n" + "\n".join(missing)
        raise PreflightError(msg)
    
    # Optional binaries
    optional = ['Xvfb', 'xwd', 'convert', 'vncsnapshot', 'xclock']
    for binary in optional:
        path = check_binary(binary, required=False)
        if path:
            binaries[binary] = path
            if verbose:
                print(f"✓ Found {binary}: {path}")
        elif verbose:
            print(f"⚠ Optional binary not found: {binary}")
    
    # Ensure viewer binaries exist, building them on demand if needed.
    _ensure_cpp_viewer(binaries, verbose=verbose)
    _ensure_rust_viewer(binaries, verbose=verbose)
    
    return binaries


def preflight_check_cpp_only(verbose: bool = False) -> Dict[str, str]:
    """
    Run preflight checks for C++ viewer tests only (no Rust viewer required).
    
    Returns dict of binary paths.
    Raises PreflightError if critical requirements missing.
    """
    binaries = {}
    
    # Required binaries
    required = [
        ('Xtigervnc', 'System TigerVNC server (install tigervnc-standalone-server or tigervnc-server)'),
        ('xterm', 'Terminal emulator (install xterm)'),
        ('openbox', 'Window manager (install openbox)'),
        ('xsetroot', 'X11 utilities (install x11-xserver-utils)'),
        ('wmctrl', 'Window manager control (install wmctrl)'),
        ('xdotool', 'X11 automation (install xdotool)'),
    ]
    
    missing = []
    for binary, description in required:
        try:
            path = check_binary(binary, required=True)
            binaries[binary] = path
            if verbose:
                print(f"✓ Found {binary}: {path}")
        except PreflightError as e:
            missing.append(f"  - {binary}: {description}")
    
    if missing:
        msg = "Missing required binaries:\n" + "\n".join(missing)
        raise PreflightError(msg)
    
    # Optional binaries
    optional = ['Xvfb', 'xwd', 'convert', 'vncsnapshot', 'xclock']
    for binary in optional:
        path = check_binary(binary, required=False)
        if path:
            binaries[binary] = path
            if verbose:
                print(f"✓ Found {binary}: {path}")
        elif verbose:
            print(f"⚠ Optional binary not found: {binary}")
    
    # Ensure C++ viewer exists, building it on demand if needed.
    _ensure_cpp_viewer(binaries, verbose=verbose)
    
    return binaries


class VNCServer:
    """Manage VNC server lifecycle (prefers local Xnjcvnc if available)."""
    
    def __init__(self, display: int, port: int, name: str, 
                 artifacts: ArtifactManager, tracker: ProcessTracker,
                 geometry: str = "1600x1000", depth: int = 24,
                 log_level: str = "*:stderr:100",
                 server_choice: str = 'auto',
                 server_params: Optional[Dict[str, str]] = None):
        self.display = display
        self.port = port
        self.name = name
        self.artifacts = artifacts
        self.tracker = tracker
        self.geometry = geometry
        self.depth = depth
        self.log_level = log_level
        self.server_choice = server_choice  # 'auto' | 'system' | 'local'
        self.server_params = server_params or {}  # Extra server parameters
        self.proc: Optional[subprocess.Popen] = None
        self.wm_proc: Optional[subprocess.Popen] = None
    
    def _select_server_binary(self) -> str:
        """Select server binary based on preference and availability.

        When a local Xnjcvnc binary is available, this will also perform a
        freshness check against the latest git commit and trigger a rebuild
        via `make server` if the binary appears older than HEAD.

        On Linux, if a test explicitly requests the local server
        (server_choice='local'), we require that the custom Xnjcvnc binary
        actually exists and is executable; otherwise we fail fast instead of
        silently falling back to the system Xtigervnc. On macOS, where the
        custom server is not currently supported, we gracefully fall back to
        Xtigervnc when 'local' is requested.
        """
        # Try both symlink and actual binary location
        local_xnjcvnc_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
        local_xnjcvnc_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'

        # If we appear to have a local server binary, ensure it is up-to-date
        # relative to the repository before we decide which binary to run.
        _maybe_rebuild_local_server(local_xnjcvnc_actual, verbose=False)
        
        local_xnjcvnc = None
        if local_xnjcvnc_symlink.exists() and os.access(local_xnjcvnc_symlink, os.X_OK):
            local_xnjcvnc = local_xnjcvnc_symlink
        elif local_xnjcvnc_actual.exists() and os.access(local_xnjcvnc_actual, os.X_OK):
            local_xnjcvnc = local_xnjcvnc_actual
        
        is_macos = (sys.platform == 'darwin')

        if self.server_choice == 'local':
            if local_xnjcvnc is not None:
                return str(local_xnjcvnc)
            # No usable local server binary
            if is_macos:
                # Custom Xnjcvnc server is not expected to build on macOS yet;
                # fall back to the system server so viewer-focused tests can
                # still run.
                print(
                    "⚠ Using system Xtigervnc because local Xnjcvnc server "
                    "is not available on this platform.",
                    file=sys.stderr,
                )
                return 'Xtigervnc'
            raise PreflightError(
                "Local Xnjcvnc server requested (server_choice='local') but "
                "no executable Xnjcvnc binary was found under the build tree. "
                "Run 'make server' first, or use server_choice='system' "
                "if you intentionally want the system Xtigervnc."
            )

        if self.server_choice == 'system':
            return 'Xtigervnc'

        # auto: prefer local server when available; otherwise fall back to
        # system Xtigervnc. Tests that truly require the custom server should
        # pass server_choice='local' so they fail fast if Xnjcvnc is missing.
        if local_xnjcvnc is not None:
            return str(local_xnjcvnc)
        return 'Xtigervnc'
    
    def start(self) -> bool:
        """Start the VNC server."""
        # Check availability
        if not check_display_available(self.display):
            print(f"ERROR: Display :{self.display} already in use", file=sys.stderr)
            return False
        
        if not check_port_available(self.port):
            print(f"ERROR: Port {self.port} already in use", file=sys.stderr)
            return False
        
        # Select server binary
        server_bin = self._select_server_binary()
        
        # Build command
        cmd = [
            server_bin,
            f':{self.display}',
            '-rfbport', str(self.port),
            '-SecurityTypes', 'None',
            '-AlwaysShared=1',
            '-AcceptKeyEvents=1',
            '-AcceptPointerEvents=1',
            '-geometry', self.geometry,
            '-depth', str(self.depth),
            '-Log', self.log_level,
        ]
        
        # Add extra server parameters (e.g., EnableContentCache=0)
        for key, value in self.server_params.items():
            cmd.append(f'-{key}={value}')
        
        log_path = self.artifacts.logs_dir / f"{self.name}_server_{self.display}.log"
        print(f"Starting VNC server :{self.display} (port {self.port}) using {os.path.basename(server_bin)}...")
        
        with open(log_path, 'w') as log_file:
            # Start in its own process group
            self.proc = subprocess.Popen(
                cmd,
                stdout=log_file,
                stderr=subprocess.STDOUT,
                preexec_fn=os.setpgrp,
                env=os.environ.copy()
            )
        
        self.tracker.register(f"vnc_{self.name}", self.proc)
        
        # Wait for server to be ready
        if not wait_for_x_display(self.display, timeout=10.0):
            print(f"ERROR: X display :{self.display} did not start", file=sys.stderr)
            return False
        
        if not wait_for_tcp_port('127.0.0.1', self.port, timeout=10.0):
            print(f"ERROR: VNC port {self.port} not listening", file=sys.stderr)
            return False
        
        print(f"✓ VNC server :{self.display} ready")
        return True
    
    def start_session(self, wm: str = "openbox") -> bool:
        """Start window manager and desktop session."""
        display_env = f":{self.display}"
        
        # Set background
        subprocess.run(['xsetroot', '-solid', '#202020'], 
                      env={**os.environ, 'DISPLAY': display_env},
                      check=False)
        
        # Start window manager
        wm_cmd = [wm, '--sm-disable'] if wm == 'openbox' else [wm]
        log_path = self.artifacts.logs_dir / f"{self.name}_wm.log"
        
        print(f"Starting window manager ({wm}) on :{self.display}...")
        with open(log_path, 'w') as log_file:
            self.wm_proc = subprocess.Popen(
                wm_cmd,
                stdout=log_file,
                stderr=subprocess.STDOUT,
                preexec_fn=os.setpgrp,
                env={**os.environ, 'DISPLAY': display_env}
            )
        
        self.tracker.register(f"wm_{self.name}", self.wm_proc)
        
        # Verify WM started
        time.sleep(1.0)
        result = subprocess.run(['wmctrl', '-m'],
                               env={**os.environ, 'DISPLAY': display_env},
                               capture_output=True,
                               timeout=5.0)
        
        if result.returncode != 0:
            print(f"ERROR: Window manager failed to start", file=sys.stderr)
            return False
        
        print(f"✓ Window manager ready")
        return True
    
    def run_in_display(self, cmd: List[str], name: str, 
                      env_overrides: Optional[Dict[str, str]] = None) -> subprocess.Popen:
        """Run a command in this display."""
        display_env = {**os.environ, 'DISPLAY': f':{self.display}'}
        if env_overrides:
            display_env.update(env_overrides)
        
        log_path = self.artifacts.logs_dir / f"{self.name}_{name}.log"
        
        log_file = open(log_path, 'w')
        proc = subprocess.Popen(
            cmd,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            preexec_fn=os.setpgrp,
            env=display_env
        )
        
        self.tracker.register(f"{self.name}_{name}", proc)
        return proc
    
    def is_alive(self) -> bool:
        """Check if server is still running."""
        return self.proc is not None and self.proc.poll() is None
    
    def stop(self):
        """Stop the VNC server and all associated processes."""
        if self.wm_proc:
            self.tracker.cleanup(f"wm_{self.name}")
        if self.proc:
            self.tracker.cleanup(f"vnc_{self.name}")
