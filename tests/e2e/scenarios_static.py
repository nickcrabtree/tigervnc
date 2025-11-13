#!/usr/bin/env python3
"""
Static content generation scenarios for reliable cache hit testing.

Uses simple X11 primitives to generate identical, reproducible content
that guarantees cache hits when repeated.
"""

import os
import subprocess
import time
from pathlib import Path
from typing import Optional


def wait_idle(seconds: float):
    """Sleep to allow encode/decode pipeline to settle."""
    time.sleep(seconds)


class StaticPatternGenerator:
    """Generate static bitmap patterns using X11 primitives."""
    
    def __init__(self, display: int):
        self.display = display
        self.env = {**os.environ, 'DISPLAY': f':{display}'}
    
    def create_pattern_image(self, width: int, height: int, pattern: str) -> str:
        """
        Create a static pattern image file using ImageMagick.
        
        Args:
            width: Image width
            height: Image height  
            pattern: Pattern type ('checkerboard', 'gradient', 'text', 'solid')
        
        Returns:
            Path to created image file
        """
        import tempfile
        
        tmpfile = tempfile.NamedTemporaryFile(mode='w', suffix='.png', delete=False)
        output_path = tmpfile.name
        tmpfile.close()
        
        if pattern == 'checkerboard':
            # Create 32x32 pixel checkerboard pattern
            cmd = [
                'convert', '-size', f'{width}x{height}',
                'pattern:checkerboard', output_path
            ]
        elif pattern == 'gradient':
            # Create gradient from black to white
            cmd = [
                'convert', '-size', f'{width}x{height}',
                'gradient:', output_path
            ]
        elif pattern == 'text':
            # Create image with repeated text
            cmd = [
                'convert', '-size', f'{width}x{height}',
                'xc:white', '-pointsize', '14', '-gravity', 'NorthWest',
                '-annotate', '+10+10', 'Cache Test Pattern\nRepeated Content',
                output_path
            ]
        elif pattern == 'solid':
            # Solid color
            cmd = [
                'convert', '-size', f'{width}x{height}',
                'xc:#4080C0', output_path
            ]
        else:
            # Default: white background with border
            cmd = [
                'convert', '-size', f'{width}x{height}',
                'xc:white', '-bordercolor', 'black', '-border', '2',
                output_path
            ]
        
        subprocess.run(cmd, check=True, env=self.env)
        return output_path
    
    def display_static_window(self, image_path: str, x: int, y: int, 
                             title: str = "static") -> Optional[int]:
        """
        Display a static image in an X window using xloadimage or display.
        
        Args:
            image_path: Path to image file
            x, y: Window position
            title: Window title for identification
        
        Returns:
            Process PID
        """
        # Try display (ImageMagick) first - more reliable for our use case
        cmd = [
            'display',
            '-title', title,
            '-geometry', f'+{x}+{y}',
            '-window', 'root',  # Use root window hints for placement
            image_path
        ]
        
        proc = subprocess.Popen(
            cmd,
            env=self.env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL
        )
        
        wait_idle(0.5)  # Allow window to appear
        return proc.pid
    
    def draw_rectangle(self, x: int, y: int, width: int, height: int, 
                       color: str = "blue"):
        """
        Draw a filled rectangle using xdotool/xwd/convert pipeline.
        More reliable than display for positioning.
        """
        # Use xsetroot to set root window background (simplest approach)
        if x == 0 and y == 0:
            cmd = ['xsetroot', '-solid', color]
            subprocess.run(cmd, env=self.env, check=False)
            wait_idle(0.3)


class StaticScenarioRunner:
    """Execute scenarios with guaranteed identical content."""
    
    def __init__(self, display: int, verbose: bool = False):
        self.display = display
        self.verbose = verbose
        self.generator = StaticPatternGenerator(display)
        self.pids = []
        self.temp_files = []
    
    def log(self, msg: str):
        """Log scenario progress."""
        import datetime
        timestamp = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        if self.verbose:
            print(f"[StaticScenario {timestamp}] {msg}")
    
    def cleanup(self):
        """Clean up spawned processes and temp files."""
        for pid in self.pids:
            try:
                os.kill(pid, 15)  # SIGTERM
            except ProcessLookupError:
                pass
        
        for path in self.temp_files:
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass
    
    def repeated_static_content(self, cycles: int = 10, 
                                duration_sec: Optional[float] = None) -> dict:
        """
        Generate cache hits by setting root window background repeatedly.
        
        Strategy:
        1. Create a static bitmap pattern
        2. Set root window background to pattern
        3. Clear (set to black)
        4. Set SAME pattern again
        5. Repeat -> guaranteed identical pixel content = cache hit
        
        This uses xsetroot -bitmap which directly modifies the root window,
        avoiding issues with window management and process termination.
        
        Args:
            cycles: Number of display/close cycles
            duration_sec: If set, run for approximate duration instead
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting repeated_static_content scenario (cycles={cycles})")
        
        stats = {
            'pattern_changes': 0,
        }
        
        # Define a set of solid colors that create large uniform regions
        # Use contrasting colors to ensure different hash values
        colors = [
            '#3366CC',  # Blue
            '#DC3912',  # Red  
            '#109618',  # Green
            '#FF9900',  # Orange
        ]
        
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        
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
            
            # Cycle through each color
            for color in colors:
                self.log(f"  Setting background to {color}")
                
                # Set root window background color
                cmd = ['xsetroot', '-solid', color]
                subprocess.run(cmd, env=env, check=False)
                stats['pattern_changes'] += 1
                
                # Allow time for VNC encoding/transmission
                wait_idle(2.0)
        
        # Final quiet period for pipeline flush
        self.log("Scenario complete, waiting for pipeline flush...")
        wait_idle(3.0)
        
        # Cleanup
        self.cleanup()
        
        self.log(f"Scenario stats: {stats}")
        return stats
    
    def solid_color_test(self, cycles: int = 15) -> dict:
        """
        Simplest possible test: solid color rectangles via xsetroot.
        
        This uses xsetroot to set the root window background color.
        Guaranteed identical pixels on each repetition.
        
        Args:
            cycles: Number of color change cycles
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting solid_color_test scenario (cycles={cycles})")
        
        stats = {
            'color_changes': 0,
        }
        
        # Cycle through a fixed set of colors
        colors = ['#4080C0', '#C04080', '#80C040']  # Blue, Pink, Green
        
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        
        for cycle in range(cycles):
            self.log(f"Cycle {cycle+1}/{cycles}")
            
            for color_idx, color in enumerate(colors):
                self.log(f"  Setting color: {color}")
                
                # Set root window background
                cmd = ['xsetroot', '-solid', color]
                subprocess.run(cmd, env=env, check=False)
                stats['color_changes'] += 1
                
                # Allow time for VNC to encode/transmit
                wait_idle(2.0)
        
        # Final flush
        self.log("Scenario complete, waiting for pipeline flush...")
        wait_idle(3.0)
        
        self.log(f"Scenario stats: {stats}")
        return stats
    
    def moving_window_test(self, cycles: int = 10) -> dict:
        """
        Test: Create ONE static window, move it to different positions.
        
        The window content is identical, but position changes.
        Tests if cache can handle same content at different locations.
        
        Args:
            cycles: Number of move operations
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting moving_window_test scenario (cycles={cycles})")
        
        stats = {
            'window_moves': 0,
        }
        
        # Create a single static pattern
        image_path = self.generator.create_pattern_image(
            320, 240, 'checkerboard'
        )
        self.temp_files.append(image_path)
        
        # Display window once
        self.log("Displaying static window")
        pid = self.generator.display_static_window(
            image_path, 100, 100, "moving_static"
        )
        if pid:
            self.pids.append(pid)
        
        wait_idle(2.0)
        
        # Move it around using wmctrl
        positions = [
            (200, 150),
            (300, 200),
            (150, 250),
            (100, 100),  # Back to start
        ]
        
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        
        for cycle in range(cycles):
            pos_idx = cycle % len(positions)
            x, y = positions[pos_idx]
            
            self.log(f"Cycle {cycle+1}/{cycles}: Moving to ({x},{y})")
            
            # Move window using wmctrl
            cmd = ['wmctrl', '-r', 'moving_static', '-e', f'0,{x},{y},-1,-1']
            result = subprocess.run(cmd, env=env, capture_output=True, timeout=5.0)
            
            if result.returncode == 0:
                stats['window_moves'] += 1
            
            wait_idle(2.0)
        
        # Cleanup
        self.log("Scenario complete, waiting for pipeline flush...")
        wait_idle(3.0)
        self.cleanup()
        
        self.log(f"Scenario stats: {stats}")
        return stats
    
    def tiled_logos_test(self, logo_path: Optional[str] = None, 
                        tiles: int = 12, duration: float = 20.0,
                        delay_between: float = 3.0) -> dict:
        """
        Display the same logo image sequentially at different positions.
        
        Shows one logo, waits for VNC to encode it, then shows the next logo.
        Each logo is identical content at a different position, creating cache
        hits after the first instance is encoded.
        
        Args:
            logo_path: Path to logo PNG file (defaults to TigerVNC logo)
            tiles: Number of logo copies to display sequentially
            duration: How long to keep final state displayed (seconds)
            delay_between: Delay between showing each logo (seconds)
        
        Returns:
            dict with statistics
        """
        self.log(f"Starting tiled_logos_test scenario (tiles={tiles})")
        
        stats = {
            'logos_displayed': 0,
        }
        
        # Default to TigerVNC logo (prefer larger sizes for better cache testing)
        if logo_path is None:
            # Find the tigervnc repo root (tests/e2e -> tests -> repo_root)
            test_dir = Path(__file__).parent.absolute()  # tests/e2e
            repo_root = test_dir.parent.parent  # Go up two levels
            
            # Try logos in order of preference (larger = better for testing)
            # All are above 2048 pixel threshold (128x128=16384, 96x96=9216, 64x64=4096)
            candidates = [
                repo_root / 'media' / 'icons' / 'tigervnc_128.png',  # 16,384 pixels
                repo_root / 'media' / 'icons' / 'tigervnc_96.png',   # 9,216 pixels
                repo_root / 'media' / 'icons' / 'tigervnc_64.png',   # 4,096 pixels
            ]
            
            logo_path = None
            for candidate in candidates:
                if candidate.exists():
                    logo_path = candidate
                    break
            
            if logo_path is None:
                raise FileNotFoundError(
                    f"No TigerVNC logo found. Tried: {[str(c) for c in candidates]}")
        
        if not Path(logo_path).exists():
            raise FileNotFoundError(f"Logo not found: {logo_path}")
        
        self.log(f"Using logo: {logo_path}")
        
        env = {**os.environ, 'DISPLAY': f':{self.display}'}
        
        # Calculate grid layout to spread across full 1920x1080 screen
        # Using 4 columns x 3 rows for 12 tiles
        cols = 4
        rows = (tiles + cols - 1) // cols
        
        # Spread across most of the screen (leave margins)
        screen_width = 1920
        screen_height = 1080
        margin_x = 100
        margin_y = 100
        
        # Calculate spacing to distribute evenly
        usable_width = screen_width - (2 * margin_x)
        usable_height = screen_height - (2 * margin_y)
        spacing_x = usable_width // (cols - 1) if cols > 1 else 0
        spacing_y = usable_height // (rows - 1) if rows > 1 else 0
        
        # Display all logos and keep them visible
        # Each window shows identical content - cache should hit after first
        for idx in range(tiles):
            row = idx // cols
            col = idx % cols
            x = margin_x + (col * spacing_x)
            y = margin_y + (row * spacing_y)
            
            self.log(f"Displaying logo {idx+1}/{tiles} at ({x},{y})")
            
            # Use display (ImageMagick) to show image
            cmd = [
                'display',
                '-geometry', f'+{x}+{y}',
                '-title', f'logo_{idx}',
                str(logo_path)
            ]
            
            proc = subprocess.Popen(
                cmd,
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL
            )
            
            self.pids.append(proc.pid)
            stats['logos_displayed'] += 1
            
            # Wait for this window to appear and be encoded
            wait_idle(delay_between)
        
        # All logos now visible - keep them displayed
        self.log(f"All {tiles} logos displayed, waiting {duration}s...")
        wait_idle(duration)
        
        # Cleanup
        self.log("Scenario complete, cleaning up...")
        self.cleanup()
        wait_idle(1.0)
        
        self.log(f"Scenario stats: {stats}")
        return stats


# Convenience function for backward compatibility
def run_static_scenario(display: int, scenario: str = 'repeated', 
                       cycles: int = 10, verbose: bool = False,
                       duration: float = 20.0) -> dict:
    """
    Run a static content scenario.
    
    Args:
        display: X display number
        scenario: 'repeated', 'solid', 'moving', or 'tiled_logos'
        cycles: Number of cycles (ignored for tiled_logos)
        duration: Duration in seconds (for tiled_logos)
        verbose: Verbose output
    
    Returns:
        Statistics dict
    """
    runner = StaticScenarioRunner(display, verbose)
    
    try:
        if scenario == 'repeated':
            return runner.repeated_static_content(cycles=cycles)
        elif scenario == 'solid':
            return runner.solid_color_test(cycles=cycles)
        elif scenario == 'moving':
            return runner.moving_window_test(cycles=cycles)
        elif scenario == 'tiled_logos':
            return runner.tiled_logos_test(tiles=12, duration=duration, delay_between=3.0)
        else:
            raise ValueError(f"Unknown scenario: {scenario}")
    finally:
        runner.cleanup()
