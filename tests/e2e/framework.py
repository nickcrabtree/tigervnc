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
BUILD_DIR = PROJECT_ROOT / "build"

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


def check_binary(name: str, required: bool = True) -> Optional[str]:
    """Check if a binary exists in PATH."""
    path = shutil.which(name)
    if required and path is None:
        raise PreflightError(f"Required binary not found: {name}")
    return path


def check_port_available(port: int) -> bool:
    """Check if a TCP port is available."""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(('127.0.0.1', port))
            return True
    except OSError:
        return False


def check_display_available(display: int) -> bool:
    """Check if an X display number is available."""
    socket_path = Path(f"/tmp/.X11-unix/X{display}")
    return not socket_path.exists()


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
    
    # Check our viewer binaries
    cpp_viewer = BUILD_DIR / "vncviewer" / "njcvncviewer"
    rust_viewer_symlink = BUILD_DIR / "vncviewer" / "njcvncviewer-rs"
    rust_viewer_direct = PROJECT_ROOT / "rust-vnc-viewer" / "target" / "release" / "njcvncviewer-rs"
    
    if not cpp_viewer.exists():
        raise PreflightError(f"C++ viewer not found: {cpp_viewer}\nRun 'make viewer' to build")
    binaries['cpp_viewer'] = str(cpp_viewer)
    
    rust_viewer = None
    if rust_viewer_symlink.exists():
        rust_viewer = rust_viewer_symlink
    elif rust_viewer_direct.exists():
        rust_viewer = rust_viewer_direct
    
    if rust_viewer is None:
        raise PreflightError(f"Rust viewer not found\nRun 'make rust_viewer' to build")
    binaries['rust_viewer'] = str(rust_viewer)
    
    if verbose:
        print(f"✓ C++ viewer: {binaries['cpp_viewer']}")
        print(f"✓ Rust viewer: {binaries['rust_viewer']}")
    
    return binaries


class VNCServer:
    """Manage VNC server lifecycle (prefers local Xnjcvnc if available)."""
    
    def __init__(self, display: int, port: int, name: str, 
                 artifacts: ArtifactManager, tracker: ProcessTracker,
                 geometry: str = "1600x1000", depth: int = 24,
                 log_level: str = "*:stderr:100",
                 server_choice: str = 'auto'):
        self.display = display
        self.port = port
        self.name = name
        self.artifacts = artifacts
        self.tracker = tracker
        self.geometry = geometry
        self.depth = depth
        self.log_level = log_level
        self.server_choice = server_choice  # 'auto' | 'system' | 'local'
        self.proc: Optional[subprocess.Popen] = None
        self.wm_proc: Optional[subprocess.Popen] = None
    
    def _select_server_binary(self) -> str:
        """Select server binary based on preference and availability."""
        # Try both symlink and actual binary location
        local_xnjcvnc_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
        local_xnjcvnc_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
        
        local_xnjcvnc = None
        if local_xnjcvnc_symlink.exists() and os.access(local_xnjcvnc_symlink, os.X_OK):
            local_xnjcvnc = local_xnjcvnc_symlink
        elif local_xnjcvnc_actual.exists() and os.access(local_xnjcvnc_actual, os.X_OK):
            local_xnjcvnc = local_xnjcvnc_actual
        
        if self.server_choice == 'local':
            return str(local_xnjcvnc) if local_xnjcvnc else 'Xtigervnc'
        if self.server_choice == 'system':
            return 'Xtigervnc'
        # auto
        if local_xnjcvnc:
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
