# Cross-Platform Testing Guide

## Overview

TigerVNC includes dedicated scripts for **cross-platform testing** where the VNC viewer runs on one machine (e.g., macOS) and connects to a VNC server on another machine (e.g., Linux). This is essential for validating ContentCache behavior across different operating systems and network configurations.

**Use Cases**:
- Test macOS viewer against Linux server
- Validate ContentCache protocol across different platforms
- Test eviction behavior with different cache sizes
- Debug protocol issues in realistic deployment scenarios
- Verify cross-platform compatibility

---

## Quick Start (macOS → Linux)

The simplest way to run a cross-platform test:

```bash
cd ~/code/tigervnc
./scripts/cachedrect_crosshost_debug_macos.sh
```

This will:
1. Start a test VNC server on your Linux machine (quartz)
2. Auto-detect if direct LAN connection is possible (or use SSH tunnel)
3. Launch the macOS viewer with verbose logging
4. Display the viewer window on your Mac
5. Retrieve server logs when done
6. Compare client and server logs automatically

---

## Available Scripts

### 1. `scripts/cachedrect_crosshost_debug_macos.sh` ⭐ Recommended

**Purpose**: Simplified macOS-specific cross-platform test

**Features**:
- ✅ Auto-detects LAN vs SSH tunnel mode
- ✅ Manages remote server lifecycle
- ✅ Retrieves and compares logs automatically
- ✅ Interactive cleanup (asks before stopping remote server)

**Usage**:
```bash
# Basic usage (all defaults)
./scripts/cachedrect_crosshost_debug_macos.sh

# Custom remote host
REMOTE=user@hostname ./scripts/cachedrect_crosshost_debug_macos.sh

# Force SSH tunnel mode
MODE=tunnel ./scripts/cachedrect_crosshost_debug_macos.sh

# Use custom viewer binary
VIEWER_BIN=/path/to/njcvncviewer ./scripts/cachedrect_crosshost_debug_macos.sh
```

**Environment Variables**:
| Variable | Default | Description |
|----------|---------|-------------|
| `REMOTE` | `nickc@quartz.local` | SSH target for remote server |
| `REMOTE_DIR` | `/home/nickc/code/tigervnc` | Remote TigerVNC repo path |
| `MODE` | `auto` | Connection mode: `auto`, `lan`, or `tunnel` |
| `VIEWER_BIN` | `build/vncviewer/njcvncviewer` | Local viewer binary path |
| `LOCAL_LOG_DIR` | `/tmp/cachedrect_debug` | Local directory for logs |

**Connection Modes**:
- `auto`: Tries direct LAN connection first, falls back to SSH tunnel
- `lan`: Direct TCP connection (e.g., `quartz.local::6898`)
- `tunnel`: SSH tunnel (`localhost::6898` → remote `localhost::6898`)

### 2. `scripts/cachedrect_crosshost_debug.sh`

**Purpose**: Full-featured cross-platform test with more control and diagnostics

**Key Differences**:
- More verbose output and progress indicators
- Creates remote control script for better server management
- More robust error handling and log retrieval
- Better for debugging complex issues

**Usage**: Same as macOS version above

### 3. `scripts/server_only_cachedrect_test.py`

**Purpose**: Server-side component that runs on Linux

**What it does**:
- Starts VNC server on display `:998` (port `6898`)
- Runs ContentCache test scenarios continuously
- Keeps server alive for external viewer connections
- Safe: Refuses to use production displays (`:1`, `:2`, `:3`)

**Usage** (normally invoked by shell scripts, but can run manually):
```bash
# On remote Linux machine
cd /home/nickc/code/tigervnc
python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 60
```

**Parameters**:
- `--display`: X display number (default: 998)
- `--port`: VNC server port (default: 6898)
- `--duration`: Initial scenario duration in seconds (default: 60)

### 4. `scripts/compare_cachedrect_logs.py`

**Purpose**: Analyzes and compares server and client logs

**Features**:
- Extracts ContentCache metrics from both logs
- Detects protocol negotiation issues
- Identifies missing capabilities
- Diagnoses common problems

**Usage**:
```bash
python3 scripts/compare_cachedrect_logs.py \
  --server /path/to/server.log \
  --client /path/to/viewer.log
```

**Metrics Extracted**:
- Server: Cache lookups, references sent, cache hits, client requests
- Client: CachedRect messages, cache misses, decoded stores, CachedRectInit
- Protocol: Capability negotiation, unknown encodings

---

## Architecture

### Standard Flow

```
┌──────────────────────────────────────────────────────────────────┐
│                        Local Machine (macOS)                     │
│                                                                  │
│  1. Run cachedrect_crosshost_debug_macos.sh                     │
│     ↓                                                            │
│  2. SSH to remote → Start server_only_cachedrect_test.py        │
│     ↓                                                            │
│  3. Wait for "SERVER_READY"                                     │
│     ↓                                                            │
│  4. Check if remote server reachable via LAN                    │
│     ├─ Yes: Direct connection (quartz.local::6898)             │
│     └─ No:  SSH tunnel (localhost::6898 → remote:6898)         │
│     ↓                                                            │
│  5. Launch local viewer                                         │
│     - macOS native viewer                                       │
│     - Verbose logging: Log=*:stderr:100                         │
│     - Logs to: /tmp/cachedrect_debug/viewer_YYYYMMDD.log       │
│     ↓                                                            │
│  6. User interacts with viewer (or let scenarios run)           │
│     ↓                                                            │
│  7. User closes viewer                                          │
│     ↓                                                            │
│  8. Retrieve remote server log via SCP                          │
│     ↓                                                            │
│  9. Compare logs: compare_cachedrect_logs.py                    │
│     ↓                                                            │
│  10. Ask: Stop remote server? [Y/N]                             │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
                             │
                             │ SSH
                             ↓
┌──────────────────────────────────────────────────────────────────┐
│                     Remote Machine (Linux/quartz)                │
│                                                                  │
│  server_only_cachedrect_test.py running                         │
│  ├─ VNC Server: Xnjcvnc :998 (port 6898)                       │
│  ├─ Window Manager: openbox                                     │
│  ├─ Scenarios: cache_hits_minimal() in loop                     │
│  ├─ Logs: tests/e2e/_artifacts/YYYYMMDD_HHMMSS/logs/           │
│  └─ Keeps running until Ctrl+C or stopped                       │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### Network Modes

#### LAN Mode (Direct Connection)
```
macOS Viewer ──TCP──> Linux Server
   (Mac)    port 6898    (quartz.local:6898)
```

**When Used**: Machines on same local network

**Advantages**:
- No SSH overhead
- Lower latency
- Simpler setup

#### Tunnel Mode (SSH Tunnel)
```
macOS Viewer ──TCP──> SSH Tunnel ──SSH──> Linux Server
   (Mac)    localhost:6898            quartz:6898
```

**When Used**: 
- Remote server not directly reachable
- Firewall blocks VNC ports
- Additional security needed

**Advantages**:
- Works anywhere SSH works
- Encrypted transport
- No firewall configuration needed

---

## Common Test Scenarios

### 1. Basic ContentCache Validation

Test that ContentCache protocol works correctly across platforms:

```bash
# Uses default settings (90 second scenario)
./scripts/cachedrect_crosshost_debug_macos.sh
```

**Expected Results**:
- Server sends CachedRect and CachedRectInit messages
- Client receives and processes cache references
- Cache hit rate > 50%
- No protocol errors

### 2. Eviction Testing (Small Cache)

Test ContentCache ARC eviction with limited memory:

**Edit the script** to add `ContentCacheSize=4` parameter:
```bash
# Edit scripts/cachedrect_crosshost_debug_macos.sh line 98:
"${VIEWER_BIN}" -Log="*:stderr:100" ContentCacheSize=4 "${TARGET_HOST}::${SERVER_PORT}" 2>&1 | tee "${VIEWER_LOG}"
```

Then run:
```bash
./scripts/cachedrect_crosshost_debug_macos.sh
```

**Expected Results**:
- Cache fills to 4MB
- Evictions occur (check client log for "Sending N cache eviction")
- Server receives eviction notifications
- Cache continues working after evictions
- Hit rate remains high

### 3. Long-Duration Stress Test

Test stability over extended period:

**Edit script** to increase duration:
```bash
# Edit scripts/server_only_cachedrect_test.py line 107:
runner.cache_hits_minimal(duration_sec=300)  # 5 minutes
```

Run test:
```bash
./scripts/cachedrect_crosshost_debug_macos.sh
```

Let viewer run for full duration, then analyze logs.

### 4. Manual Connection (Custom Parameters)

For maximum control, start server manually then connect:

**On remote (quartz)**:
```bash
cd /home/nickc/code/tigervnc
python3 scripts/server_only_cachedrect_test.py --display 998 --port 6898 --duration 120
# Wait for "SERVER_READY"
```

**On local (macOS)**:
```bash
cd ~/code/tigervnc
build/vncviewer/njcvncviewer quartz.local::6898 \
  ContentCacheSize=4 \
  Log=*:stderr:100 \
  2>&1 | tee /tmp/eviction_test_$(date +%Y%m%d_%H%M%S).log
```

**Retrieve and compare**:
```bash
# Get latest server log
scp quartz:/home/nickc/code/tigervnc/tests/e2e/_artifacts/latest/logs/*.log /tmp/

# Compare
python3 scripts/compare_cachedrect_logs.py \
  --server /tmp/server.log \
  --client /tmp/eviction_test_YYYYMMDD_HHMMSS.log
```

---

## Log Locations

### Local (macOS)

**Default log directory**: `/tmp/cachedrect_debug/`

Files:
- `viewer_YYYYMMDD_HHMMSS.log` - Local viewer log (verbose)
- `contentcache_test_YYYYMMDD_HHMMSS.log` - Downloaded server log

**Manual test logs**: `/tmp/eviction_test_*.log`

### Remote (Linux/quartz)

**Artifacts directory**: `/home/nickc/code/tigervnc/tests/e2e/_artifacts/`

Structure:
```
_artifacts/
└── YYYYMMDD_HHMMSS/
    └── logs/
        ├── contentcache_server_only.log
        └── vnc_contentcache_server_only.log
```

**Temporary logs**: `/tmp/cachedrect_server_stdout.log`

---

## Troubleshooting


### Remote Server Not Reachable

**Symptom**: "Timed out waiting for server"

**Diagnose**:
```bash
# Check if server is running
ssh quartz "ps aux | grep 'Xnjcvnc :998'"

# Check if port is listening
ssh quartz "ss -tln | grep :6898"

# Check server stdout
ssh quartz "tail -50 /tmp/cachedrect_server_stdout.log"
```

**Solution**: Server may have failed to start. Check remote logs for errors.

### Port Already in Use

**Symptom**: "Port 6898 already in use"

**Solution**:
```bash
# Check what's using the port on remote
ssh quartz "lsof -i :6898"

# If it's a stale test server, kill it safely
ssh quartz "ps aux | grep 'Xnjcvnc :998' | grep -v grep"
# Verify it's display :998 (not :1, :2, or :3!)
ssh quartz "kill -TERM <PID>"
```

### Display Already in Use

**Symptom**: "Display :998 already in use"

**Solution**:
```bash
# Check for stale lock file
ssh quartz "ls -la /tmp/.X11-unix/X998"

# Only remove if you're sure it's stale
ssh quartz "rm -f /tmp/.X11-unix/X998 /tmp/.X998-lock"
```

### No Cache Activity

**Symptom**: Server log shows 0 cache hits

**Possible Causes**:
1. ContentCache not enabled on server
2. Scenario not generating repeated content
3. Viewer not advertising ContentCache capability

**Diagnose**:
```bash
# Check if viewer advertises capability
grep -i "SetEncodings" /tmp/cachedrect_debug/viewer_*.log

# Check server capability
ssh quartz "grep -i 'ContentCache' /home/nickc/code/tigervnc/tests/e2e/_artifacts/latest/logs/*.log"

# Verify server binary has ContentCache
ssh quartz "strings /home/nickc/code/tigervnc/build/unix/vncserver/Xnjcvnc | grep -i contentcache"
```

### SSH Tunnel Issues

**Symptom**: Connection hangs or fails in tunnel mode

**Solution**:
```bash
# Test SSH connectivity
ssh quartz echo "SSH works"

# Manually create tunnel and test
ssh -L 6898:localhost:6898 quartz
# In another terminal:
nc -z localhost 6898  # Should succeed

# Kill stale tunnel
pkill -f "ssh -fN -L 6898"
```

### Viewer Crashes or Exits Immediately

**Check**:
1. Viewer binary exists and is executable
2. Server is actually ready before viewer starts

**Debug**:
```bash
# Test viewer directly
build/vncviewer/njcvncviewer --help
# Should show help text

# Check viewer log for errors
tail -50 /tmp/cachedrect_debug/viewer_*.log
```

---

## Integration with E2E Tests

The cross-platform scripts complement the main e2e test suite in `tests/e2e/`:

### When to Use Each

**Use cross-platform scripts when**:
- Testing macOS viewer ↔ Linux server compatibility
- Debugging protocol issues across platforms
- Validating real-world network scenarios
- Testing with actual GUI interaction needed

**Use e2e tests (`run_contentcache_test.py`) when**:
- Comparing C++ vs Rust viewer behavior
- Running automated CI/CD tests
- Testing on single Linux machine
- Comparing different server modes (system vs local)

### Combining Tests

You can run e2e tests on Linux, then validate with cross-platform test:

```bash
# 1. Run e2e test on Linux server
ssh quartz "cd /home/nickc/code/tigervnc/tests/e2e && ./run_contentcache_test.py"

# 2. Then run cross-platform test from macOS
./scripts/cachedrect_crosshost_debug_macos.sh

# 3. Compare results
```

---

## Customization Examples

### Custom Cache Size Test

Create a wrapper script:

```bash
#!/bin/bash
# test_eviction_4mb.sh

# Modify viewer command to use 4MB cache
export VIEWER_BIN="build/vncviewer/njcvncviewer"

# Run with custom log directory
export LOCAL_LOG_DIR="/tmp/eviction_test_4mb"
mkdir -p "$LOCAL_LOG_DIR"

# Temporarily patch the script to add ContentCacheSize
sed 's/\(njcvncviewer.*-Log=\)/\1ContentCacheSize=4 /' \
  scripts/cachedrect_crosshost_debug_macos.sh > /tmp/test_eviction.sh

chmod +x /tmp/test_eviction.sh
/tmp/test_eviction.sh
```

### Multiple Test Runs

Run several tests with different configurations:

```bash
#!/bin/bash
# run_multiple_tests.sh

for cache_size in 4 8 16 32; do
  echo "Testing with ${cache_size}MB cache..."
  
  # Modify and run
  sed "s/Log=\"\\*:stderr:100\"/Log=\"*:stderr:100\" ContentCacheSize=${cache_size}/" \
    scripts/cachedrect_crosshost_debug_macos.sh > /tmp/test_${cache_size}mb.sh
  
  chmod +x /tmp/test_${cache_size}mb.sh
  /tmp/test_${cache_size}mb.sh
  
  # Wait between tests
  sleep 10
done
```

### Custom Scenario Duration

Edit `scripts/server_only_cachedrect_test.py` to change scenario behavior:

```python
# Line 107: Change duration
runner.cache_hits_minimal(duration_sec=180)  # 3 minutes instead of default

# Line 123: Change periodic interval
runner.cache_hits_minimal(duration_sec=60)  # Longer periodic runs
```

---

## Best Practices

### Before Running Tests

1. **Build binaries**: Ensure viewer is built locally and server is built remotely
   ```bash
   # Local
   make viewer
   
   # Remote
   ssh quartz "cd /home/nickc/code/tigervnc && make viewer && make server"
   ```

2. **Check prerequisites**:
   - SSH key auth configured (no password prompts)
   - Network connectivity to remote host
   - macOS viewer binary built

3. **Clean up previous tests**:
   ```bash
   # Local
   rm -rf /tmp/cachedrect_debug/*
   
   # Remote
   ssh quartz "pkill -f server_only_cachedrect_test || true"
   ```

### During Tests

1. **Monitor logs in real-time**:
   ```bash
   # In another terminal
   tail -f /tmp/cachedrect_debug/viewer_*.log
   ```

2. **Check server status**:
   ```bash
   ssh quartz "tail -f /tmp/cachedrect_server_stdout.log"
   ```

3. **Verify cache activity**:
   Look for these log messages:
   - Server: "ContentCache protocol hit"
   - Client: "Cache hit for ID", "Storing decoded rect"

### After Tests

1. **Always analyze comparison output**: 
   - Check for "No obvious issues detected"
   - Review any warnings or problems

2. **Save important logs**:
   ```bash
   # Archive successful test results
   cp -r /tmp/cachedrect_debug /tmp/archived_$(date +%Y%m%d_%H%M%S)
   ```

3. **Stop remote servers**: 
   - Don't leave test servers running indefinitely
   - Scripts will prompt you

---

## Safety Reminders

⚠️ **CRITICAL**: Cross-platform scripts **NEVER** touch production VNC servers

**Protected displays**: `:1`, `:2`, `:3`
- These are production servers in active use
- Scripts explicitly refuse to use these displays
- Server-only script validates display numbers

**Test displays**: `:998`, `:999`
- Isolated test servers
- Safe to start/stop
- Managed by test framework

**Process safety**:
- Scripts use specific PIDs, never pattern-based `pkill`
- Always verify display number before killing processes
- See `WARP.md` for detailed safety guidelines

---

## FAQ

**Q: Do I need to be on the same network as the remote server?**  
A: No. The script auto-detects and uses SSH tunnel if direct connection fails.

**Q: Can I run tests from Linux to macOS?**  
A: The scripts are optimized for macOS→Linux. For Linux→Linux, use the standard e2e tests. Linux→macOS would require script modifications.

**Q: How do I test the Rust viewer?**  
A: Change `VIEWER_BIN` to point to the Rust viewer:
```bash
VIEWER_BIN=rust-vnc-viewer/target/release/njcvncviewer-rs ./scripts/cachedrect_crosshost_debug_macos.sh
```

**Q: Can I use a different remote machine?**  
A: Yes, set `REMOTE` and `REMOTE_DIR`:
```bash
REMOTE=user@hostname REMOTE_DIR=/path/to/tigervnc ./scripts/cachedrect_crosshost_debug_macos.sh
```

**Q: What if I want to test with a real application instead of xterm?**  
A: Modify `scripts/server_only_cachedrect_test.py` to run different scenarios. See `tests/e2e/scenarios.py` for available scenarios or create custom ones.

**Q: How do I add the eviction test to the automated script?**  
A: Edit `scripts/cachedrect_crosshost_debug_macos.sh` line 98 to add `ContentCacheSize=4` or your desired cache size.

---

## See Also

- **E2E Tests**: `tests/e2e/README.md` - Full e2e test documentation
- **E2E Quick Start**: `tests/e2e/QUICKSTART.md` - Quick reference for e2e tests
- **Safety Guidelines**: `WARP.md` - Process management and safety rules
- **ContentCache Design**: `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` - Protocol details
- **ARC Eviction**: `docs/CONTENTCACHE_ARC_EVICTION_SUMMARY.md` - Eviction implementation
