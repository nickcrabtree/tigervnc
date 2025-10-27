# ContentCache End-to-End Tests - Quick Start

## Overview

The ContentCache E2E test framework validates that the Rust VNC viewer exhibits the same ContentCache behavior as the C++ viewer baseline.

**Test Architecture:**
- Two VNC servers: content server (:998) and viewer window server (:999)
- Content server runs automated scenarios (opening terminals, typing commands)
- Internal viewer on :999 connects to :998 to create the VNC-in-VNC setup
- External viewers (C++ then Rust) connect to :999 with verbose logging
- Test compares ContentCache metrics between C++ baseline and Rust candidate

**Key Benefits:**
- No windows appear on user's physical display (all contained in test servers)
- Safe and self-contained (never touches production displays :1-:3)
- Fully automated with detailed metrics and artifact collection
- Graceful cleanup in all scenarios (success, failure, Ctrl-C)

## Prerequisites

```bash
# Required binaries (checked by preflight)
which Xtigervnc      # VNC server
which openbox        # Window manager (or fluxbox/icewm)
which xterm          # Terminal emulator
which xdotool        # X automation tool

# Built viewers in expected locations
ls ~/code/tigervnc/build/vncviewer/njcvncviewer        # C++ viewer
ls ~/code/tigervnc/rust-vnc-viewer/target/release/njcvncviewer-rs  # Rust viewer
```

## Basic Usage

### Run with Defaults

```bash
cd ~/code/tigervnc/tests/e2e
./run_contentcache_test.py
```

**Defaults:**
- Content server: display :998, port 6898
- Viewer window server: display :999, port 6899
- Duration: 90 seconds per viewer
- Window manager: openbox

### Common Options

```bash
# Shorter test (30 seconds per viewer)
./run_contentcache_test.py --duration 30

# Use different window manager
./run_contentcache_test.py --wm fluxbox

# Custom displays/ports (if defaults conflict)
./run_contentcache_test.py --display-content 900 --port-content 6800 \
                            --display-viewer 901 --port-viewer 6801

# Server modes: system Xtigervnc, local Xnjcvnc, or auto (both if available)
./run_contentcache_test.py --server-modes system
./run_contentcache_test.py --server-modes local
./run_contentcache_test.py --server-modes system,local

# Baseline-only (skip Rust)
./run_contentcache_test.py --skip-rust

# Verbose output
./run_contentcache_test.py --verbose
```

## Test Flow

1. **Preflight checks** - Verify all binaries, ports, displays available
2. **Start content server** (:998) - Desktop with automated content
3. **Start viewer window server** (:999) - Desktop for viewer windows
4. **Launch internal viewer** - C++ viewer on :999 → :998
5. **Run C++ baseline** - External C++ viewer on :999, capture logs
6. **Run scenario** - Open terminals, type commands, generate ContentCache activity
7. **Run Rust candidate** - External Rust viewer on :999, capture logs
8. **Replay scenario** - Same actions to generate comparable traffic
9. **Parse logs** - Extract ContentCache metrics from both viewers
10. **Compare metrics** - Validate Rust matches C++ within tolerances
11. **Report results** - Human-readable summary, pass/fail determination
12. **Cleanup** - Terminate all servers and viewers

## Expected Output

```
======================================================================
TigerVNC ContentCache End-to-End Test
======================================================================

[1/10] Setting up artifacts directory...
Artifacts will be saved to: tests/e2e/_artifacts/20251026_165041

[2/10] Running preflight checks...
✓ Found Xtigervnc: /usr/bin/Xtigervnc
✓ Found xterm: /usr/bin/xterm
✓ Found openbox: /usr/bin/openbox
✓ C++ viewer: /home/nickc/code/tigervnc/build/vncviewer/njcvncviewer
✓ Rust viewer: /home/nickc/code/tigervnc/rust-vnc-viewer/target/release/njcvncviewer-rs
✓ All preflight checks passed

[3/10] Starting content server (:998)...
Starting VNC server :998 (port 6898)...
✓ VNC server :998 ready
Starting window manager (openbox) on :998...
✓ Window manager ready
✓ Content server ready

[4/10] Starting nested server (:999)...
Starting VNC server :999 (port 6899)...
✓ VNC server :999 ready
Starting window manager (openbox) on :999...
✓ Window manager ready
✓ Nested server ready

[5/10] Launching internal viewer...
✓ Internal viewer connected

[6/10] Running C++ viewer baseline...
  Starting cpp_viewer...
  Running scenario...
  Scenario completed: 30 windows, 60 commands
  Stopping C++ viewer...
✓ C++ baseline complete

[7/10] Running Rust viewer candidate...
  Starting rust_viewer...
  Running scenario...
  Scenario completed: 30 windows, 60 commands
  Stopping Rust viewer...
✓ Rust candidate complete

[8/10] Parsing logs...
✓ Logs parsed

[9/10] Comparing metrics...

[10/10] Results

======================================================================
BASELINE (C++ Viewer)
======================================================================
ContentCache Metrics:
  Hits: 1234, Misses: 289, Hit Rate: 81.0%
  Stores: 289, Lookups: 1523

Protocol Messages:
  CachedRect: 1234
  CachedRectInit: 289
  RequestCachedData: 0

======================================================================
CANDIDATE (Rust Viewer)
======================================================================
ContentCache Metrics:
  Hits: 1228, Misses: 295, Hit Rate: 80.6%
  Stores: 295, Lookups: 1523

Protocol Messages:
  CachedRect: 1228
  CachedRectInit: 295
  RequestCachedData: 0

======================================================================
COMPARISON
======================================================================
✓ PASS: Rust viewer matches C++ baseline within tolerances

======================================================================
ARTIFACTS
======================================================================
All artifacts saved to: tests/e2e/_artifacts/20251026_165041
  Logs: tests/e2e/_artifacts/20251026_165041/logs
  Screenshots: tests/e2e/_artifacts/20251026_165041/screenshots
  Reports: tests/e2e/_artifacts/20251026_165041/reports

Cleaning up...
✓ Cleanup complete

======================================================================
✓ TEST PASSED
======================================================================
```

## What Gets Tested

1. **VNC-in-VNC Setup**: Two servers (:998, :999) with internal viewer bridge
2. **Content Generation**: Automated xterm operations (open, type, close, repeat)
3. **Cache Behavior**: Both viewers see identical content, should have similar cache hit rates
4. **Protocol Validation**: CachedRect, CachedRectInit message counts match within ±5%
5. **Hit Rate Validation**: Cache hit rates match within ±2%

## Troubleshooting

### Port Already in Use

```bash
# Check what's using the port
lsof -i :6898

# Use different ports
python3 tests/e2e/run_contentcache_test.py --port1 7000 --port2 7001
```

### Missing Dependencies

```bash
# Error: Required binary not found: xdotool
sudo apt-get install xdotool

# Error: Required binary not found: openbox
sudo apt-get install openbox
```

### Viewer Not Built

```bash
# C++ viewer missing
make viewer

# Rust viewer missing
make rust_viewer
```

### Low Hit Rate

If hit rate is < 20%, increase scenario duration:

```bash
python3 tests/e2e/run_contentcache_test.py --duration 180
```

## Customization

```bash
# Different displays (if :998/:999 in use)
python3 tests/e2e/run_contentcache_test.py --display1 900 --display2 901

# Different window manager
python3 tests/e2e/run_contentcache_test.py --wm fluxbox

# All options
python3 tests/e2e/run_contentcache_test.py --help
```

## Understanding Results

### Interpreting Results

**Test passes if all metrics are within tolerance:**
- Cache hits/misses: within ±5% (small differences from timing/scheduling acceptable)
- Hit rate: within ±2 percentage points
- Bandwidth saved: within ±10% (encoding variations acceptable)
- Protocol efficiency: within ±1 percentage point

**Failure scenarios:**
- Large deviation in cache hit rate → Rust cache lookup logic differs
- Missing CachedRect messages → Rust not recognizing protocol extension
- Excessive CachedRectInit messages → Rust cache missing entries C++ found
- Protocol efficiency drop → Rust sending more RequestCachedData messages

## Artifacts

All test artifacts saved to timestamped directory in `/tmp/`:

```
/tmp/tigervnc-e2e-test-YYYYMMDD-HHMMSS/
├── logs/
│   ├── server_content.log        # Content server (:998) log
│   ├── server_viewer_window.log  # Viewer window server (:999) log
│   ├── cpp_viewer.log            # C++ viewer verbose log
│   └── rust_viewer.log           # Rust viewer verbose log
├── screenshots/
│   ├── cpp_*.png                 # C++ viewer screenshots (future)
│   └── rust_*.png                # Rust viewer screenshots (future)
└── reports/
    └── comparison.txt            # Metrics comparison report (future)
```

## Advanced Usage

### Custom Tolerances

Edit `run_contentcache_test.py`:

```python
from comparator import Tolerances

custom_tolerances = Tolerances(
    cache_hit_rate_pp=1.0,      # Tighter: ±1pp instead of ±2pp
    bandwidth_saved_pct=5.0,    # Tighter: ±5% instead of ±10%
    protocol_efficiency_pp=0.5, # Tighter: ±0.5pp instead of ±1pp
    cache_hits_pct=3.0,         # Tighter: ±3% instead of ±5%
    cache_misses_pct=3.0        # Tighter: ±3% instead of ±5%
)

comparison = compare_metrics(cpp_metrics, rust_metrics, tolerances=custom_tolerances)
```

### Custom Scenarios

Edit `scenarios.py` to add new test patterns:

```python
class ScenarioRunner:
    def my_custom_scenario(self, duration_sec=60):
        """Your custom ContentCache scenario."""
        # ... automation commands ...
        return {'custom_metric': value}
```

### Integration with CTest

```cmake
# In tests/e2e/CMakeLists.txt (future)
add_test(
    NAME contentcache_e2e
    COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_contentcache_test.py
            --duration 60
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
)
set_tests_properties(contentcache_e2e PROPERTIES
    TIMEOUT 300
    LABELS "e2e;contentcache"
)
```

## FAQ

**Q: Do test windows appear on my desktop?**  
A: No. All test activity is contained within ephemeral VNC servers on high-numbered displays (:998, :999). Nothing appears on your active display.

**Q: Is it safe to run with production servers on :1-:3?**  
A: Yes. The test never touches displays below :900 and checks for conflicts before starting.

**Q: How long does the test take?**  
A: Default is 90 seconds per viewer (total ~3-4 minutes). Use `--duration 30` for faster iteration.

**Q: Can I run tests in parallel?**  
A: No. Each test needs exclusive access to its displays/ports. Run sequentially or use non-overlapping display ranges.

**Q: What if I don't have openbox installed?**  
A: Use `--wm fluxbox` or install openbox: `sudo apt-get install openbox`

**Q: Can I see what's happening during the test?**  
A: Connect a viewer to the test servers while running:
```bash
# In another terminal
vncviewer localhost::6898  # See content server
vncviewer localhost::6899  # See viewer window server
```

## Next Steps

- Run tests after modifying Rust ContentCache implementation
- Review logs in `/tmp/tigervnc-e2e-test-*/` to debug failures
- Adjust tolerances if needed for your environment
- Add custom scenarios for specific ContentCache patterns
- Integrate with CI/CD for automated validation
- See `README.md` for detailed documentation
- See `IMPLEMENTATION_STATUS.md` for technical details

## Support

For issues or questions:
1. Check artifacts in `/tmp/tigervnc-e2e-test-*/`
2. Review server/viewer logs for error messages
3. Verify all binaries built correctly
4. Consult `README.md` for architecture details
5. Report bugs with full log output
