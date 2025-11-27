#!/usr/bin/env python3
"""
Automated content generation scenarios for VNC ContentCache testing.

Provides scripted desktop interactions that generate repetitive content
patterns to trigger ContentCache hits.
"""

import os
import subprocess
import time
from typing import Optional


def wait_idle(seconds: float):
    """Sleep to allow encode/decode pipeline to settle."""
    time.sleep(seconds)


def open_xterm(title: str, geom: str, display: int) -> Optional[int]:
    """
    Open xterm with specific title and geometry.
    
    Args:
        title: Window title
        geom: Geometry string like "80x24+100+100"
        display: X display number
    
    Returns:
        Process PID or None on failure
    """
    cmd = [
        'xterm',
        '-title', title,
        '-name', title,  # set WM_CLASS resource name for reliable matching
        '-geometry', geom,
        '-hold',  # Keep window open
    ]
    
    env = {**os.environ, 'DISPLAY': f':{display}'}
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    
    # Wait for window to appear
    wait_idle(0.5)
    return proc.pid


def open_xterm_run(title: str, geom: str, display: int, shell_cmd: str) -> Optional[int]:
    """
    Open xterm that runs a shell command, then exits.
    Keeps window open briefly so framebuffer captures content.
    """
    cmd = [
        'xterm',
        '-title', title,
        '-name', title,
        '-geometry', geom,
        '-e', 'bash', '-lc', shell_cmd,
    ]
    env = {**os.environ, 'DISPLAY': f':{display}'}
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_idle(0.3)
    return proc.pid


def close_window_by_title(title: str, display: int) -> bool:
    """Close a window by title using wmctrl, with xdotool fallback."""
    env = {**os.environ, 'DISPLAY': f':{display}'}
    cmd = ['wmctrl', '-c', title]
    result = subprocess.run(cmd, env=env, capture_output=True, timeout=5.0)
    if result.returncode == 0:
        wait_idle(0.3)
        return True
    # Fallback: xdotool windowkill
    try:
        for search_args in (['--class', title], ['--name', title]):
            search = subprocess.run(['xdotool', 'search', *search_args], env=env, capture_output=True, timeout=5.0)
            if search.returncode == 0:
                wid = search.stdout.decode().strip().splitlines()[0]
                subprocess.run(['xdotool', 'windowkill', wid], env=env, capture_output=True, timeout=5.0)
                wait_idle(0.3)
                return True
    except Exception:
        pass
    return False


def type_into_window(title: str, text: str, display: int, delay_ms: int = 50) -> bool:
    """
    Type text into a window by title.
    
    Args:
        title: Window title to focus
        text: Text to type (escape special chars with backslashes)
        display: X display number
        delay_ms: Delay between keystrokes in milliseconds
    
    Returns:
        True on success
    """
    env = {**os.environ, 'DISPLAY': f':{display}'}
    
    # Focus window (wmctrl, fallback to xdotool search)
    focus_cmd = ['wmctrl', '-a', title]
    result = subprocess.run(focus_cmd, env=env, capture_output=True, timeout=5.0)
    if result.returncode != 0:
        # Fallback: activate via xdotool
        try:
            # Try by class first (set via -name), then by title
            for search_args in (['--class', title], ['--name', title]):
                search = subprocess.run(['xdotool', 'search', *search_args], env=env, capture_output=True, timeout=5.0)
                if search.returncode == 0:
                    wid = search.stdout.decode().strip().splitlines()[0]
                    subprocess.run(['xdotool', 'windowactivate', '--sync', wid], env=env, capture_output=True, timeout=5.0)
                    break
            else:
                return False
        except Exception:
            return False
    
    wait_idle(0.2)
    
    # Type text
    type_cmd = ['xdotool', 'type', '--delay', str(delay_ms), text]
    result = subprocess.run(type_cmd, env=env, capture_output=True, timeout=60.0)
    
    wait_idle(0.5)
    return result.returncode == 0


def move_resize_window(title: str, x: int, y: int, w: int, h: int, display: int) -> bool:
    """Move and resize a window by title, with xdotool fallback."""
    env = {**os.environ, 'DISPLAY': f':{display}'}
    cmd = ['wmctrl', '-r', title, '-e', f'0,{x},{y},{w},{h}']
    result = subprocess.run(cmd, env=env, capture_output=True, timeout=5.0)
    if result.returncode == 0:
        wait_idle(0.3)
        return True
    # Fallback: xdotool
    try:
        for search_args in (['--class', title], ['--name', title]):
            search = subprocess.run(['xdotool', 'search', *search_args], env=env, capture_output=True, timeout=5.0)
            if search.returncode == 0:
                wid = search.stdout.decode().strip().splitlines()[0]
                subprocess.run(['xdotool', 'windowmove', wid, str(x), str(y)], env=env, capture_output=True, timeout=5.0)
                subprocess.run(['xdotool', 'windowsize', wid, str(w), str(h)], env=env, capture_output=True, timeout=5.0)
                wait_idle(0.3)
                return True
    except Exception:
        pass
    return False


def run_command_in_window(title: str, command: str, display: int) -> bool:
    """Run a command by typing it and pressing Enter."""
    env = {**os.environ, 'DISPLAY': f':{display}'}
    
    # Focus window
    focus_cmd = ['wmctrl', '-a', title]
    result = subprocess.run(focus_cmd, env=env, capture_output=True, timeout=5.0)
    if result.returncode != 0:
        return False
    
    wait_idle(0.2)
    
    # Type command
    type_cmd = ['xdotool', 'type', '--delay', '30', command]
    subprocess.run(type_cmd, env=env, capture_output=True, timeout=10.0)
    
    # Press Enter
    enter_cmd = ['xdotool', 'key', 'Return']
    subprocess.run(enter_cmd, env=env, capture_output=True, timeout=5.0)
    
    wait_idle(0.5)
    return True


class ScenarioRunner:
    """Execute repeatable content generation scenarios."""
    
    def __init__(self, display: int, verbose: bool = False):
        self.display = display
        self.verbose = verbose
        self.pids = []
    
    def log(self, msg: str):
        """Log scenario progress."""
        if self.verbose:
            print(f"[Scenario] {msg}")
    
    def cleanup(self):
        """Clean up any spawned processes."""
        for pid in self.pids:
            try:
                os.kill(pid, 15)  # SIGTERM
            except ProcessLookupError:
                pass
    
    # --- Variable-content helpers (xclock grid) ---
    def _spawn_xclock(self, x: int, y: int, size: int = 160, update: int = 1, analog: bool = True) -> int:
        """Spawn a single xclock at x,y with given size and update interval.
        Returns the PID (or raises if xclock missing)."""
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        args = ['xclock', '-geometry', f'{size}x{size}+{x}+{y}', '-update', str(update)]
        if analog:
            args.insert(1, '-analog')
        proc = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.pids.append(proc.pid)
        return proc.pid
    
    def xclock_grid(self, cols: int = 6, rows: int = 2, size: int = 160, update: int = 1,
                    duration_sec: float = 60.0) -> dict:
        """Start a grid of ticking xclocks to generate variable pixel content.
        Falls back to eviction_stress if xclock is not available."""
        self.log(f"Starting xclock grid: {cols}x{rows}, size={size}, update={update}s, duration={duration_sec}s")
        stats = {'clocks_started': 0, 'fallback_used': False}
        try:
            # Quickly verify availability
            if subprocess.call(['bash', '-lc', 'command -v xclock >/dev/null 2>&1']) != 0:
                raise FileNotFoundError('xclock not found')
            # Launch grid
            spacing_x = size + 10
            spacing_y = size + 10
            start_x, start_y = 40, 40
            for r in range(rows):
                for c in range(cols):
                    x = start_x + c * spacing_x
                    y = start_y + r * spacing_y
                    try:
                        self._spawn_xclock(x, y, size=size, update=update, analog=True)
                        stats['clocks_started'] += 1
                    except Exception:
                        # If any spawn fails, attempt to continue launching others
                        continue
            # Let them tick for duration
            wait_idle(duration_sec)
        except FileNotFoundError:
            # Fallback to the existing eviction_stress scenario
            self.log("xclock not available; falling back to eviction_stress")
            stats['fallback_used'] = True
            stats.update(self.eviction_stress(duration_sec=duration_sec))
        finally:
            # Do not cleanup here; caller decides when to clean up (we keep PIDs)
            pass
        self.log(f"xclock grid stats: {stats}")
        return stats
    
    def cache_hits_minimal(self, cycles: int = 15, duration_sec: Optional[float] = None) -> dict:
        """
        Generate ContentCache hits through repetitive window operations without relying on XTEST.
        Uses xterm -e to render identical content, then closes and reopens at same geometry.
        
        Args:
            cycles: Number of open/close/reopen cycles
            duration_sec: If set, override cycles to run for approximate duration
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting cache_hits_minimal scenario (cycles={cycles})")
        
        stats = {
            'windows_opened': 0,
            'windows_closed': 0,
            'commands_typed': 0,
            'moves_resizes': 0,
        }
        
        # Fixed content repeated each cycle
        banner_lines = [
            "========================================",
            "TigerVNC ContentCache Test Scenario",
            "========================================",
            "This content will repeat to generate",
            "cache hits in the ContentCache system.",
            "========================================",
        ]
        banner_cmd = "; ".join([f"echo '{line}'" for line in banner_lines])
        command_sequence = "ls -l /bin | head -n 20"
        
        # Start time tracking for duration-based execution
        start_time = time.time()
        
        cycle = 0
        while True:
            if duration_sec:
                if time.time() - start_time >= duration_sec:
                    break
            else:
                if cycle >= cycles:
                    break
            
            cycle += 1
            self.log(f"Cycle {cycle}/{cycles if not duration_sec else '~'}")
            
            # Phase 1: Open xterm at fixed position with fixed geometry, render content, pause
            self.log("  Opening + rendering content...")
            shell_cmd = f"{banner_cmd}; {command_sequence}; sleep 1.5"
            pid = open_xterm_run("cacheterm", "80x24+100+100", self.display, shell_cmd)
            if pid:
                self.pids.append(pid)
                stats['windows_opened'] += 1
            
            # Allow time for rendering
            wait_idle(2.0)
            
            # Phase 2: Reopen at SAME position/size with identical content (expect cache hits)
            self.log("  Reopen + render same content (expect hits)...")
            pid = open_xterm_run("cacheterm", "80x24+100+100", self.display, shell_cmd)
            if pid:
                self.pids.append(pid)
                stats['windows_opened'] += 1
            
            wait_idle(2.0)
            
            # Optional: slight move to vary blit positions every few cycles
            if cycle % 3 == 0:
                positions = [
                    (200, 150, 640, 480),
                    (300, 200, 640, 480),
                    (100, 100, 640, 480),
                ]
                for x, y, w, h in positions:
                    self.log(f"  Moving window to ({x},{y})...")
                    if move_resize_window("cacheterm", x, y, w, h, self.display):
                        stats['moves_resizes'] += 1
                    wait_idle(0.6)
        
        # Final quiet period for pipeline flush
        self.log("Scenario complete, waiting for pipeline flush...")
        wait_idle(2.5)
        
        # Cleanup
        self.cleanup()
        
        self.log(f"Scenario stats: {stats}")
        return stats
    
    def cache_hits_with_clock(self, duration_sec: float = 60.0) -> dict:
        """
        Generate cache hits with an xclock providing animated updates.
        
        Args:
            duration_sec: How long to run scenario
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting cache_hits_with_clock scenario (duration={duration_sec}s)")
        
        stats = {'windows_opened': 0, 'windows_closed': 0}
        
        # Start xclock if available
        try:
            env = {**os.environ, 'DISPLAY': f':{self.display}'}
            clock_proc = subprocess.Popen(
                ['xclock', '-analog', '-update', '1', '-geometry', '200x200+50+50'],
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL
            )
            self.pids.append(clock_proc.pid)
            self.log("Started xclock")
            wait_idle(1.0)
        except FileNotFoundError:
            self.log("xclock not available, skipping")
        
        # Run cache_hits_minimal with duration
        stats.update(self.cache_hits_minimal(duration_sec=duration_sec))
        
        return stats
    
    def eviction_stress(self, duration_sec: float = 60.0) -> dict:
        """
        Generate many unique rectangles to stress cache eviction.
        
        Opens xterm windows at many different positions with unique content
        to ensure the cache fills up and evictions occur.
        
        Args:
            duration_sec: How long to run scenario
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting eviction_stress scenario (duration={duration_sec}s)")
        
        stats = {'windows_opened': 0, 'unique_positions': 0, 'commands_typed': 0}
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        
        start_time = time.time()
        position_counter = 0
        
        # Generate many windows at different positions with unique content
        while time.time() - start_time < duration_sec:
            # Use varying positions to generate unique cached rectangles
            x = 50 + (position_counter * 47) % 800
            y = 50 + (position_counter * 31) % 500
            geom = f"60x10+{x}+{y}"
            
            # Unique content per window to ensure unique cache entries
            unique_cmd = f"echo 'Window {position_counter} at {x},{y}'; date; uname -a; sleep 0.5"
            
            self.log(f"Opening window {position_counter} at {x},{y}")
            
            pid = open_xterm_run(f"evict{position_counter}", geom, self.display, unique_cmd)
            if pid:
                self.pids.append(pid)
                stats['windows_opened'] += 1
            
            position_counter += 1
            stats['unique_positions'] = position_counter
            
            # Shorter wait to generate more content faster
            wait_idle(0.8)
            
            # Every 5 windows, generate some large content
            if position_counter % 5 == 0:
                large_cmd = f"cat /etc/passwd; ls -la /usr/bin | head -50; echo 'Iteration {position_counter}'; sleep 0.5"
                lg_geom = f"80x25+{(x + 100) % 800}+{(y + 100) % 500}"
                pid = open_xterm_run(f"evictlarge{position_counter}", lg_geom, self.display, large_cmd)
                if pid:
                    self.pids.append(pid)
                    stats['windows_opened'] += 1
                wait_idle(1.0)
        
        # Final quiet period for pipeline flush
        self.log("Scenario complete, waiting for pipeline flush...")
        wait_idle(2.5)
        
        # Cleanup
        self.cleanup()
        
        self.log(f"Scenario stats: {stats}")
        return stats
