#!/usr/bin/env python3
"""
Capture screenshots of LibreOffice Impress at each slide transition
to analyze rendering consistency.
"""

import sys
import time
import subprocess
import os
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from framework import (
    preflight_check_cpp_only, PreflightError, ArtifactManager,
    ProcessTracker, VNCServer, check_port_available, check_display_available,
    PROJECT_ROOT, BUILD_DIR
)

# Default presentation file
DEFAULT_PRESENTATION = "/ncloud/Nick/PMO/UK/Catcher/CGG 4D Processing/05 Presentations from CGG/2019 N-S baseline repro/001_20190812_4dcatch_lcf.pptx"


def start_libreoffice_impress(presentation_path: str, display: int, tracker: ProcessTracker) -> subprocess.Popen:
    """Start LibreOffice Impress in editing mode."""
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    cmd = [
        'libreoffice',
        '--impress',
        '--nofirststartwizard',
        '--nologo',
        '--norestore',
        str(presentation_path)
    ]
    
    proc = subprocess.Popen(
        cmd,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        preexec_fn=os.setpgrp
    )
    
    tracker.register('libreoffice_impress', proc)
    return proc


def wait_for_window(display: int, window_name_patterns: list, timeout: float = 30.0) -> bool:
    """Wait for a window matching any of the patterns to appear."""
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    if isinstance(window_name_patterns, str):
        window_name_patterns = [window_name_patterns]
    
    start = time.time()
    while time.time() - start < timeout:
        for pattern in window_name_patterns:
            result = subprocess.run(
                ['xdotool', 'search', '--name', pattern],
                env=env,
                capture_output=True,
                text=True
            )
            if result.returncode == 0 and result.stdout.strip():
                return True
        time.sleep(0.5)
    
    return False


def capture_screenshot(display: int, output_path: str):
    """Capture a screenshot of the display using import (ImageMagick)."""
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    subprocess.run(
        ['import', '-window', 'root', output_path],
        env=env,
        check=False
    )


def send_key(display: int, key: str):
    """Send a key press to the currently focused window."""
    env = os.environ.copy()
    env['DISPLAY'] = f':{display}'
    
    subprocess.run(
        ['xdotool', 'key', key],
        env=env,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL
    )


def main():
    display = 998
    port = 6898
    presentation_path = Path(DEFAULT_PRESENTATION)
    
    if not presentation_path.exists():
        print(f"✗ Presentation not found: {presentation_path}")
        return 1
    
    print("=" * 70)
    print("Screenshot Capture for Slide Navigation Analysis")
    print("=" * 70)
    
    # Create artifacts directory
    artifacts = ArtifactManager()
    artifacts.create()
    screenshots_dir = artifacts.logs_dir / 'screenshots'
    screenshots_dir.mkdir(exist_ok=True)
    
    print(f"\nScreenshots will be saved to: {screenshots_dir}")
    
    # Preflight
    try:
        binaries = preflight_check_cpp_only(verbose=False)
    except PreflightError as e:
        print(f"✗ Preflight failed: {e}")
        return 1
    
    # Check port/display
    if not check_port_available(port):
        print(f"✗ Port {port} in use")
        return 1
    if not check_display_available(display):
        print(f"✗ Display :{display} in use")
        return 1
    
    tracker = ProcessTracker()
    
    # Determine server mode
    local_server_symlink = BUILD_DIR / 'unix' / 'vncserver' / 'Xnjcvnc'
    local_server_actual = BUILD_DIR / 'unix' / 'xserver' / 'hw' / 'vnc' / 'Xnjcvnc'
    
    server_mode = 'local' if (local_server_symlink.exists() or local_server_actual.exists()) else 'system'
    print(f"Using {server_mode} VNC server")
    
    try:
        # Start VNC server
        print(f"\nStarting VNC server on :{display}...")
        server = VNCServer(
            display, port, "screenshot_server",
            artifacts, tracker,
            geometry="1920x1080",
            log_level="*:stderr:30",
            server_choice=server_mode,
        )
        
        if not server.start():
            print("✗ Could not start VNC server")
            return 1
        if not server.start_session(wm='openbox'):
            print("✗ Could not start session")
            return 1
        print("✓ VNC server ready")
        
        # Start LibreOffice
        print("\nStarting LibreOffice Impress...")
        lo_proc = start_libreoffice_impress(presentation_path, display, tracker)
        
        window_patterns = [
            presentation_path.stem,
            presentation_path.name,
            "Impress",
            "LibreOffice",
        ]
        
        print("Waiting for LibreOffice window...")
        if not wait_for_window(display, window_patterns, timeout=60.0):
            print("✗ LibreOffice window did not appear")
            return 1
        print("✓ LibreOffice started")
        
        # Let it fully render
        time.sleep(5.0)
        
        # Capture initial state
        print("\nCapturing screenshots...")
        capture_screenshot(display, str(screenshots_dir / 'initial.png'))
        print("  Captured: initial.png")
        
        # Navigate through slides and capture at each step
        num_slides = 3
        num_cycles = 2
        delay = 2.5
        
        screenshot_num = 1
        for cycle in range(num_cycles):
            print(f"\n  Cycle {cycle + 1}:")
            
            # Forward navigation using Page Down
            for i in range(num_slides):
                print(f"    Forward: Page Down (to slide {i + 2})...")
                send_key(display, 'Page_Down')
                time.sleep(delay)
                
                filename = f'cycle{cycle + 1}_fwd_slide{i + 2}_{screenshot_num:03d}.png'
                capture_screenshot(display, str(screenshots_dir / filename))
                print(f"    Captured: {filename}")
                screenshot_num += 1
            
            # Backward navigation using Page Up
            for i in range(num_slides):
                print(f"    Backward: Page Up (to slide {num_slides - i})...")
                send_key(display, 'Page_Up')
                time.sleep(delay)
                
                filename = f'cycle{cycle + 1}_back_slide{num_slides - i}_{screenshot_num:03d}.png'
                capture_screenshot(display, str(screenshots_dir / filename))
                print(f"    Captured: {filename}")
                screenshot_num += 1
        
        print(f"\n✓ Captured {screenshot_num} screenshots")
        print(f"\nScreenshots saved to: {screenshots_dir}")
        print("\nTo compare slides at the same position across cycles:")
        print("  - Cycle 1 forward slide 2: cycle1_fwd_slide2_001.png")
        print("  - Cycle 2 forward slide 2: cycle2_fwd_slide2_007.png")
        print("\nUse 'compare' from ImageMagick to diff images:")
        print(f"  compare {screenshots_dir}/cycle1_fwd_slide2_001.png {screenshots_dir}/cycle2_fwd_slide2_007.png diff.png")
        
        return 0
        
    except KeyboardInterrupt:
        print("\nInterrupted")
        return 130
    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return 1
    finally:
        print("\nCleaning up...")
        tracker.cleanup_all()
        print("✓ Done")


if __name__ == '__main__':
    sys.exit(main())
