# Test Framework Validation Checklist

## Current Status

### ✅ What's Ready
- [x] Test framework implementation complete (~2,220 lines)
- [x] C++ viewer built: `/home/nickc/code/tigervnc/build/vncviewer/njcvncviewer`
- [x] Rust viewer built: `/home/nickc/code/tigervnc/rust-vnc-viewer/target/release/njcvncviewer-rs`
- [x] System TigerVNC server installed: `/usr/bin/Xtigervnc`
- [x] Basic X11 tools: `xterm`, `xsetroot`

### ❌ Missing Dependencies

```bash
sudo apt-get install -y openbox wmctrl xdotool
```

**What each does:**
- `openbox`: Lightweight window manager for test desktops
- `wmctrl`: Window manager control (focus, close, move windows)
- `xdotool`: X11 automation (type text, press keys)

## Validation Steps

### Step 1: Install Dependencies

```bash
sudo apt-get update
sudo apt-get install -y openbox wmctrl xdotool
```

### Step 2: Run Preflight Check

```bash
cd /home/nickc/code/tigervnc
python3 -c "
import sys
sys.path.insert(0, 'tests/e2e')
from framework import preflight_check
try:
    preflight_check(verbose=True)
    print('\n✓ All checks passed!')
except Exception as e:
    print(f'\n✗ Failed: {e}')
"
```

**Expected output:**
```
✓ Found Xtigervnc: /usr/bin/Xtigervnc
✓ Found xterm: /usr/bin/xterm
✓ Found openbox: /usr/bin/openbox
✓ Found xsetroot: /usr/bin/xsetroot
✓ Found wmctrl: /usr/bin/wmctrl
✓ Found xdotool: /usr/bin/xdotool
✓ C++ viewer: /home/nickc/code/tigervnc/build/vncviewer/njcvncviewer
✓ Rust viewer: /home/nickc/code/tigervnc/rust-vnc-viewer/target/release/njcvncviewer-rs

✓ All checks passed!
```

### Step 3: Dry Run Test (Smoke Test)

First, let's verify the basic framework works without running the full test:

```bash
# Test server startup only
python3 -c "
import sys
sys.path.insert(0, 'tests/e2e')
from framework import ArtifactManager, ProcessTracker, VNCServer, preflight_check

print('Testing server startup...')
preflight_check(verbose=False)

artifacts = ArtifactManager()
artifacts.create()
tracker = ProcessTracker()

try:
    server = VNCServer(998, 6898, 'test', artifacts, tracker)
    if server.start():
        print('✓ Server started successfully')
        server.stop()
        print('✓ Server stopped cleanly')
    else:
        print('✗ Server failed to start')
finally:
    tracker.cleanup_all()
"
```

### Step 4: Run Short Test

Run the full test with a short duration (30 seconds) to validate end-to-end:

```bash
python3 tests/e2e/run_contentcache_test.py --duration 30 --verbose
```

**What to expect:**
- Takes ~1-2 minutes total
- Creates two VNC servers on :998 and :999
- Runs automated xterm operations
- Compares C++ vs Rust viewer logs
- Exits with code 0 on success

### Step 5: Full Test Run

Once the short test passes, run the full duration:

```bash
python3 tests/e2e/run_contentcache_test.py --verbose
```

**Expected duration:** ~3-4 minutes
- 90 seconds for C++ viewer scenario
- 90 seconds for Rust viewer scenario
- Plus startup/shutdown overhead

## Troubleshooting

### Displays Already in Use

**Error:**
```
✗ FAIL: Display :998 already in use
```

**Check:**
```bash
ls /tmp/.X11-unix/X998 /tmp/.X11-unix/X999
```

**Fix if stale:**
```bash
rm /tmp/.X11-unix/X998 /tmp/.X11-unix/X999
```

**Or use different displays:**
```bash
python3 tests/e2e/run_contentcache_test.py --display1 900 --display2 901
```

### Ports Already in Use

**Error:**
```
✗ FAIL: Port 6898 already in use
```

**Check:**
```bash
lsof -i :6898
lsof -i :6899
```

**Fix:**
- Kill the process using the port (if it's safe)
- Or use different ports: `--port1 7000 --port2 7001`

### Window Manager Doesn't Start

**Error:**
```
✗ FAIL: Could not start content server session
```

**Debug:**
```bash
# Check logs
ls tests/e2e/_artifacts/*/logs/content_wm.log
tail -50 tests/e2e/_artifacts/*/logs/content_wm.log
```

**Common causes:**
- openbox not installed
- X display not accessible
- Config file errors

### Scenario Failures

**Error:**
```
✗ FAIL: Scenario execution failed
```

**Debug:**
- Check if xterm is spawning: `DISPLAY=:998 xterm -title test &`
- Check if wmctrl works: `DISPLAY=:998 wmctrl -l`
- Check if xdotool works: `DISPLAY=:998 xdotool getactivewindow`

### Log Parsing Issues

If the test reports parsing errors:
1. Check actual log format in `_artifacts/<timestamp>/logs/cpp_viewer.log`
2. The log parser patterns may need adjustment based on actual output
3. This is expected during initial validation - we can iterate on the patterns

## Success Criteria

The test **passes** when you see:

```
======================================================================
✓ TEST PASSED
======================================================================
```

At minimum:
- Both servers start successfully
- Scenarios execute without crashes
- Logs are captured
- Comparison completes (even if hit rates differ initially)

## Next Steps After Validation

1. **Iterate on log patterns**: Adjust `log_parser.py` based on actual log output
2. **Tune scenarios**: Adjust timing/content for better cache hit rates
3. **Add more scenarios**: Extend `scenarios.py` with new patterns
4. **Optional**: Add screenshot comparison
5. **Optional**: Integrate with CTest

## Quick Reference

```bash
# Full test with verbose output
python3 tests/e2e/run_contentcache_test.py --verbose

# Short test (30 seconds)
python3 tests/e2e/run_contentcache_test.py --duration 30

# Different displays/ports
python3 tests/e2e/run_contentcache_test.py --display1 900 --port1 7000 --display2 901 --port2 7001

# Check artifacts
ls -la tests/e2e/_artifacts/

# View logs
tail -f tests/e2e/_artifacts/*/logs/cpp_viewer.log
```

## Getting Help

If you encounter issues:
1. Check `_artifacts/<timestamp>/logs/` for detailed logs
2. Run with `--verbose` for more output
3. See `README.md` for detailed troubleshooting
4. See `QUICKSTART.md` for common issues
