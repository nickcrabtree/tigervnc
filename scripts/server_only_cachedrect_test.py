#!/usr/bin/env python3
"""
Server-only CachedRectInit test for cross-host debugging.

Starts the VNC server with ContentCache enabled, runs the scenario to generate
repeated content, then keeps the server alive for an external viewer to connect.
"""

import sys
import time
import signal
from pathlib import Path

# Add the e2e framework to Python path
sys.path.insert(0, str(Path(__file__).parent.parent / "tests" / "e2e"))

from framework import (
    preflight_check, ArtifactManager, ProcessTracker, VNCServer,
    check_port_available, check_display_available
)
from scenarios import ScenarioRunner


class ServerOnlyTest:
    def __init__(self, display_num=998, port_num=6898, duration=60):
        self.display_num = display_num
        self.port_num = port_num 
        self.duration = duration
        self.artifacts = None
        self.tracker = None
        self.server = None
        self.running = True
        
    def setup_signal_handlers(self):
        """Handle Ctrl+C gracefully."""
        def signal_handler(signum, frame):
            print(f"\\nReceived signal {signum}, shutting down...")
            self.running = False
            
        signal.signal(signal.SIGINT, signal_handler)
        signal.signal(signal.SIGTERM, signal_handler)
        
    def run(self):
        self.setup_signal_handlers()
        
        print("=" * 70)
        print("Server-Only CachedRectInit Test")
        print("=" * 70)
        print()
        print(f"Server will run on display :{self.display_num}, port {self.port_num}")
        print(f"Scenario duration: {self.duration}s")
        print("After scenario, server will remain alive for external connections.")
        print("Press Ctrl+C to stop.")
        print()
        
        # Setup
        self.artifacts = ArtifactManager()
        self.artifacts.create()
        print(f"Artifacts: {self.artifacts.base_dir}")
        
        self.tracker = ProcessTracker()
        
        # Safety check: prevent accidental use of production displays
        if self.display_num in (1, 2, 3):
            print("✗ ABORT: Cannot use production displays :1, :2, or :3 (see WARP.md)")
            return False
        
        # Check availability
        if not check_port_available(self.port_num):
            print(f"✗ FAIL: Port {self.port_num} in use")
            return False
        if not check_display_available(self.display_num):
            print(f"✗ FAIL: Display :{self.display_num} in use")
            return False
        
        try:
            # Start VNC server with ContentCache enabled
            print(f"\\n[1/4] Starting VNC server (:{self.display_num})...")
            self.server = VNCServer(
                self.display_num, self.port_num, "contentcache_server_only",
                self.artifacts, self.tracker,
                geometry="1024x768",
                log_level="*:stderr:100",  # Verbose logging
                server_choice="local",  # Use local Xnjcvnc with ContentCache
            )
            
            if not self.server.start():
                print("✗ FAIL: Could not start server")
                return False
            
            # Start window manager
            print("[2/4] Starting window manager...")
            self.server.start_session("openbox")
            time.sleep(2)
            
            print("✓ SERVER_READY - External viewers can now connect")
            print(f"  Connection: quartz.local::{self.port_num}")
            print(f"  Direct IP connection: <quartz-ip>:{self.port_num}")
            print()
            
            # Run initial scenario to populate cache
            print(f"[3/4] Running initial scenario ({self.duration}s)...")
            print("  Generating repeated rectangles to trigger cache activity...")
            runner = ScenarioRunner(self.display_num, verbose=True)
            
            # Use cache_hits_minimal scenario - opens/closes xterm repeatedly
            runner.cache_hits_minimal(duration_sec=self.duration)
            
            print("[4/4] Scenario complete. Server remains running for external connections.")
            print()
            print("=" * 50)
            print("SERVER IS READY FOR EXTERNAL CONNECTIONS")
            print("=" * 50)
            print(f"Connect your viewer to: quartz.local::{self.port_num}")
            print("Press Ctrl+C when done testing.")
            print()
            
            # Keep server alive until interrupted
            while self.running:
                # Periodically run scenario to maintain cache activity
                print(f"Running periodic cache activity scenario... (Press Ctrl+C to stop)")
                try:
                    runner.cache_hits_minimal(duration_sec=30)
                    if self.running:
                        time.sleep(10)  # Short break between scenarios
                except KeyboardInterrupt:
                    break
                    
            return True
            
        except KeyboardInterrupt:
            print("\\nInterrupted by user")
            return True
        finally:
            print("\\nShutting down...")
            if self.server and self.server.proc:
                self.tracker.cleanup(f"vnc_{self.server.name}")
            if self.server and self.server.wm_proc:
                self.tracker.cleanup(f"wm_{self.server.name}")


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description="Server-only CachedRectInit test")
    parser.add_argument('--display', type=int, default=998,
                       help='Test server display number (default: 998)')
    parser.add_argument('--port', type=int, default=6898,
                       help='Test server port (default: 6898)')
    parser.add_argument('--duration', type=int, default=60,
                       help='Initial scenario duration in seconds (default: 60)')
    args = parser.parse_args()
    
    # Run preflight
    try:
        preflight_check(verbose=False)
    except Exception as e:
        print(f"✗ Preflight failed: {e}")
        sys.exit(1)
    
    # Run server-only test
    test = ServerOnlyTest(args.display, args.port, args.duration)
    success = test.run()
    
    sys.exit(0 if success else 1)


if __name__ == '__main__':
    main()