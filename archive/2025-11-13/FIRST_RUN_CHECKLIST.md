# First Run Checklist

Quick reference for running the ContentCache E2E tests for the first time.

## Before Running

### 1. Verify System Dependencies

```bash
# Check all required binaries exist
which Xtigervnc    # VNC server
which xterm        # Terminal emulator  
which xdotool      # X automation
which openbox      # Window manager

# If missing, install:
# sudo apt-get install tigervnc-standalone-server xterm xdotool openbox
```

### 2. Build Both Viewers

```bash
cd ~/code/tigervnc

# Build C++ viewer
make viewer
ls build/vncviewer/njcvncviewer  # Should exist

# Build Rust viewer
make rust_viewer
ls rust-vnc-viewer/target/release/njcvncviewer-rs  # Should exist
```

### 3. Check Test Ports/Displays Available

```bash
# Check if test ports free
lsof -i :6898  # Should show nothing
lsof -i :6899  # Should show nothing

# Check if test displays free  
ls /tmp/.X11-unix/X998  # Should not exist
ls /tmp/.X11-unix/X999  # Should not exist

# If in use, use different displays:
# ./run_contentcache_test.py --display-content 900 --display-viewer 901
```

## Running the Test

### Quick Test (30 seconds per viewer)

```bash
cd ~/code/tigervnc/tests/e2e
./run_contentcache_test.py --duration 30 --verbose
```

### Full Test (90 seconds per viewer, default)

```bash
cd ~/code/tigervnc/tests/e2e
./run_contentcache_test.py
```

## What to Expect

### Normal Progress
```
[1/10] Setting up artifacts directory...
[2/10] Running preflight checks...
[3/10] Starting content server...
[4/10] Starting viewer window server...
[5/10] Launching internal viewer...
[6/10] Running C++ viewer baseline...
[7/10] Running Rust viewer candidate...
[8/10] Parsing logs...
[9/10] Comparing metrics...
[10/10] Results
```

### Test Duration
- **Quick test (--duration 30)**: ~2 minutes total
- **Full test (default 90)**: ~4 minutes total

### Artifacts Location
```
/tmp/tigervnc-e2e-test-YYYYMMDD-HHMMSS/
├── logs/
│   ├── server_content.log
│   ├── server_viewer_window.log
│   ├── cpp_viewer.log
│   └── rust_viewer.log
├── screenshots/
└── reports/
```

## If Something Goes Wrong

### Test Hangs

```bash
# Ctrl-C to interrupt (cleanup happens automatically)

# If processes stuck, manually clean up:
ps aux | grep -E 'Xtigervnc.*:99[89]|njcvncviewer'
# Kill specific PIDs (NOT with pattern matching!)
kill -TERM <PID>

# Clean up X sockets if needed
rm -f /tmp/.X11-unix/X998 /tmp/.X11-unix/X999
```

### Missing Dependencies

```bash
# Error: Required binary not found: <binary>
sudo apt-get install <binary>

# Common packages:
sudo apt-get install xterm xdotool openbox tigervnc-standalone-server
```

### Port/Display Conflicts

```bash
# Use different ports/displays
./run_contentcache_test.py \
    --display-content 900 --port-content 6800 \
    --display-viewer 901 --port-viewer 6801
```

### Viewer Not Built

```bash
cd ~/code/tigervnc
make viewer        # C++ viewer
make rust_viewer   # Rust viewer
```

## After Running

### Review Results

Check exit code:
- `0` = Test passed (Rust matches C++ within tolerances)
- `1` = Test failed or error occurred

### Check Artifacts

```bash
# Find latest test run
ls -lt /tmp/ | grep tigervnc-e2e-test | head -n1

# Review logs
cd /tmp/tigervnc-e2e-test-<timestamp>
ls -lh logs/

# Look for errors
grep -i error logs/*.log
grep -i fail logs/*.log
```

### Understand Metrics

Example successful output:
```
BASELINE (C++ Viewer)
  Cache hits: 778, misses: 456, hit rate: 63.0%

CANDIDATE (Rust Viewer)  
  Cache hits: 772, misses: 458, hit rate: 62.8%

COMPARISON
  ✓ Hit rate: 62.8% vs 63.0% (diff: -0.2pp, tolerance: ±2.0pp)
  ✓ TEST PASSED
```

Small differences (<5%) are acceptable due to timing variations.

## Common First-Run Issues

### 1. Server Won't Start
**Symptom**: "Could not start content server"  
**Fix**: Check logs in `/tmp/tigervnc-e2e-test-*/logs/server_content.log`

### 2. Internal Viewer Fails
**Symptom**: "Internal viewer exited prematurely"  
**Fix**: Check if C++ viewer built correctly, try running manually:
```bash
DISPLAY=:999 ~/code/tigervnc/build/vncviewer/njcvncviewer localhost::6898
```

### 3. No Content Generated
**Symptom**: Zero cache hits/misses  
**Fix**: Check xdotool works on test display:
```bash
DISPLAY=:998 xterm &
DISPLAY=:998 xdotool search --class xterm
```

### 4. Log Parsing Fails
**Symptom**: "Could not parse logs" or all metrics zero  
**Fix**: Check log format manually:
```bash
grep -i cache /tmp/tigervnc-e2e-test-*/logs/cpp_viewer.log | head -n20
```

### 5. Comparison Fails
**Symptom**: Test fails but metrics look similar  
**Fix**: Check tolerances in `comparator.py`, may need adjustment for your system

## Next Steps After Successful Run

1. **Review metrics** - Understand what's being measured
2. **Try longer duration** - `--duration 120` for more stable metrics
3. **Watch live** - Connect viewer to `localhost::6898` during test
4. **Customize scenarios** - Edit `scenarios.py` for different patterns
5. **Integrate with CI** - Once stable, add to automated testing

## Quick Reference

```bash
# Minimal first run
cd ~/code/tigervnc/tests/e2e
./run_contentcache_test.py --duration 30

# View help
./run_contentcache_test.py --help

# Clean up stuck processes (emergency only)
pkill -f 'Xtigervnc.*:99[89]'
rm -f /tmp/.X11-unix/X99[89]
```

## Documentation

- **QUICKSTART.md** - Comprehensive usage guide
- **README.md** - Architecture and design
- **IMPLEMENTATION_COMPLETE.md** - What's implemented
- **IMPLEMENTATION_STATUS.md** - Technical details

## Support

If you encounter issues not covered here:

1. Check artifacts in `/tmp/tigervnc-e2e-test-*/`
2. Review all logs for error messages
3. Try running components manually to isolate issue
4. Consult detailed documentation (README.md)
5. Report bugs with full log output

---

**Ready to run?**

```bash
cd ~/code/tigervnc/tests/e2e
./run_contentcache_test.py --duration 30 --verbose
```

Good luck! 🚀
