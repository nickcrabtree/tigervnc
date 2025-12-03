#!/usr/bin/env python3
"""
Automated content generation scenarios for VNC ContentCache testing.

Provides scripted desktop interactions that generate repetitive content
patterns to trigger ContentCache hits.
"""

import os
import subprocess
import time
import shutil
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

    def _pick_browser(self) -> Optional[tuple[str, str]]:
        """Pick an installed web browser suitable for automated tests.

        Returns a tuple of (binary_path, kind) where kind is a simple
        classification string (e.g. "firefox" or "chromium"). This allows
        callers to choose invocation flags appropriate for that family while
        still keeping discovery logic in one place.
        """
        candidates: list[tuple[str, str]] = [
            ("firefox", "firefox"),
            ("chromium-browser", "chromium"),
            ("chromium", "chromium"),
            ("google-chrome", "chromium"),
            ("brave-browser", "chromium"),
        ]
        for name, kind in candidates:
            try:
                path = shutil.which(name)
            except Exception:
                path = None
            if path:
                return path, kind
        return None
    
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

    def browser_scroll_bbc(self, duration_sec: float = 60.0) -> dict:
        """Open a browser on bbc.com and continuously scroll the page.

        This is intended as a more "real world" scenario for visual tests,
        with a long, scrolling document and mixed text/image content.
        """
        self.log(f"Starting browser_scroll_bbc scenario (duration={duration_sec}s)")

        stats = {"browser_found": False, "scroll_steps": 0}

        picked = self._pick_browser()
        if not picked:
            self.log("No supported browser found; skipping browser_scroll_bbc")
            wait_idle(duration_sec)
            return stats

        browser, kind = picked
        stats["browser_found"] = True

        env = {**os.environ, "DISPLAY": f":{self.display}"}

        # Use a dedicated, throwaway profile directory for the browser so that
        # the test never reuses or interferes with the user's existing desktop
        # browser instance. For Firefox this avoids remote-control reusing an
        # existing process on another display, and for Chromium-family
        # browsers a separate user-data-dir guarantees a distinct instance.
        profile_base = f"/tmp/tigervnc_e2e_browser_{kind}_{self.display}"
        try:
            os.makedirs(profile_base, exist_ok=True)
        except Exception:
            # Best-effort; if we cannot create the directory we still attempt
            # to launch the browser, but this may fall back to its default
            # profile behaviour.
            pass

        url = "https://www.bbc.com"
        cmd: list[str]
        if kind == "firefox":
            cmd = [
                browser,
                "--no-remote",  # do not talk to an existing Firefox instance
                "-profile",
                profile_base,
                url,
            ]
        else:
            # Chromium-family: isolate via a separate user-data-dir so this
            # instance does not attach to an already running browser.
            cmd = [
                browser,
                f"--user-data-dir={profile_base}",
                "--no-first-run",
                "--no-default-browser-check",
                url,
            ]

        self.log(f"Launching browser {browser} ({kind}) at {url} on :{self.display}")
        try:
            proc = subprocess.Popen(
                cmd,
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            self.pids.append(proc.pid)
        except Exception as exc:
            self.log(f"Failed to launch browser: {exc}")
            wait_idle(5.0)
            return stats
            self.log(f"Failed to launch browser: {exc}")
            wait_idle(5.0)
            return stats

        # Give the browser time to start and render content.
        wait_idle(8.0)

        start = time.time()
        while time.time() - start < duration_sec:
            try:
                # Send Page_Down with cleared modifiers so it works regardless
                # of current modifier state.
                subprocess.run(
                    [
                        "xdotool",
                        "key",
                        "--clearmodifiers",
                        "Page_Down",
                    ],
                    env=env,
                    capture_output=True,
                    timeout=5.0,
                )
                stats["scroll_steps"] += 1
            except Exception:
                # Best-effort; keep going even if a single key event fails.
                pass

            # Small delay between scrolls so the page has time to redraw.
            wait_idle(2.0)

        self.log("browser_scroll_bbc complete, waiting for pipeline flush...")
        wait_idle(3.0)

        # Do not explicitly kill the browser here; cleanup() will handle it.
        self.cleanup()

        self.log(f"browser_scroll_bbc stats: {stats}")
        return stats
    
    def eviction_stress(self, duration_sec: float = 60.0) -> dict:
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

    def typing_stress(self, duration_sec: float = 20.0, delay_ms: int = 80) -> dict:
        """Simulate continuous typing inside an xterm using an internal script.

        Instead of relying on external focus/xdotool, we start xterm with a
        shell pipeline that runs a small Python script. That script prints
        characters with delays and logs per-character timestamps to a file,
        giving us traceability while guaranteeing that the typing is visible
        in the xterm.
        """
        self.log(f"Starting typing_stress scenario (duration={duration_sec}s, delay_ms={delay_ms})")

        stats = {"bursts": 0, "chars_typed": 0}

        # Build a self-contained Python script to run inside the xterm.
        # It prints characters with gaps and logs timestamp + character to
        # /tmp/typing_stress_<display>.log.
        log_path = f"/tmp/typing_stress_{self.display}.log"
        text = (
            "The quick brown fox jumps over the lazy dog. "
            "Typing latency debug line with numbers 1234567890. "
        )

        py_script = (
            "import sys, time, datetime\n"
            f"text = {text!r}\n"
            f"delay = {delay_ms} / 1000.0\n"
            f"end_time = time.time() + {duration_sec}\n"
            f"log = open({log_path!r}, 'a', buffering=1)\n"
            "bursts = 0\n"
            "chars = 0\n"
            "while time.time() < end_time:\n"
            "    for ch in text:\n"
            "        now = time.time()\n"
            "        if now >= end_time:\n"
            "            break\n"
            "        ts = datetime.datetime.now().isoformat()\n"
            "        line = f'{ts} {ch}\\n'\n"
            "        log.write(line)\n"
            "        log.flush()\n"
            "        sys.stdout.write(ch)\n"
            "        sys.stdout.flush()\n"
            "        chars += 1\n"
            "        time.sleep(delay)\n"
            "    bursts += 1\n"
            "log.write(f'BURSTS {bursts} CHARS {chars}\\n')\n"
            "log.flush()\n"
            "log.close()\n"
            "time.sleep(2.0)\n"
        )

        shell_cmd = f"python3 - << 'EOF'\n{py_script}EOF"

        # Launch a single xterm that runs the above script. We rely on the
        # script's own timing and logging rather than controlling it from
        # outside.
        pid = open_xterm_run("typeterm", "80x24+100+100", self.display, shell_cmd)
        if pid:
            self.pids.append(pid)
        else:
            self.log("Failed to start typeterm xterm with typing script; aborting typing_stress")
            return stats

        # Let the script run to completion.
        wait_idle(duration_sec + 3.0)

        # We don't currently parse the log here; just record approximate stats
        # based on expected behaviour.
        stats["bursts"] = int(max(1, duration_sec // ((len(text) * delay_ms / 1000.0) + 0.5)))
        stats["chars_typed"] = stats["bursts"] * len(text)

        # Final quiet period for pipeline flush
        self.log("typing_stress complete, waiting for pipeline flush...")
        wait_idle(1.0)

        # Cleanup
        self.cleanup()

        self.log(f"typing_stress stats: {stats}")
        return stats

    def typing_replay_from_log(self, log_path: str, speed_scale: float = 1.0) -> dict:
        """Replay a captured typing log inside an xterm.

        The log should be produced by tests/e2e/typing_capture.py and contain
        lines of the form:

            <unix_timestamp> <codepoint>

        Comments starting with "#" are ignored. The relative timing between
        keystrokes is preserved (optionally scaled by speed_scale), and the
        characters are printed inside an xterm so the VNC server sees the
        same pattern of updates.
        """
        from pathlib import Path

        self.log(f"Starting typing_replay_from_log: log_path={log_path}, speed_scale={speed_scale}")
        stats = {"events": 0, "duration": 0.0}

        p = Path(log_path).expanduser()
        if not p.exists():
            self.log(f"Log file does not exist: {p}")
            return stats

        # Parse timestamps and codes to determine total duration for scheduling
        times = []
        codes = []
        with p.open("r") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                parts = line.split()
                if len(parts) < 2:
                    continue
                try:
                    t = float(parts[0])
                    c = int(parts[1])
                except ValueError:
                    continue
                times.append(t)
                codes.append(c)

        if not times:
            self.log("No events found in typing log; nothing to replay")
            return stats

        start_ts = times[0]
        end_ts = times[-1]
        duration = max(0.0, (end_ts - start_ts) * speed_scale)
        stats["events"] = len(times)
        stats["duration"] = duration

        # Build a Python script that replays this log from within xterm.
        # We bake the parsed times/codes directly into the script for
        # simplicity.
        import textwrap

        times_repr = repr(times)
        codes_repr = repr(codes)

        py_script = textwrap.dedent(
            f"""
            import sys, time

            times = {times_repr}
            codes = {codes_repr}
            speed = {speed_scale}

            if not times:
                sys.exit(0)

            base = times[0]
            last = base
            for t, c in zip(times, codes):
                target = base + (t - base) * speed
                # Sleep relative to last event
                now = time.time()
                delay = (t - last) * speed
                if delay > 0:
                    time.sleep(delay)
                sys.stdout.write(chr(c))
                sys.stdout.flush()
                last = t

            # Brief pause so the last characters are visible
            time.sleep(2.0)
            """
        )

        shell_cmd = f"python3 - << 'EOF'\n{py_script}EOF"

        pid = open_xterm_run("typereplay", "80x24+100+350", self.display, shell_cmd)
        if pid:
            self.pids.append(pid)
        else:
            self.log("Failed to start typereplay xterm; aborting typing_replay_from_log")
            return stats

        # Wait for the replay to complete plus a small margin.
        wait_idle(duration + 3.0)

        self.log(f"typing_replay_from_log stats: {stats}")
        return stats
