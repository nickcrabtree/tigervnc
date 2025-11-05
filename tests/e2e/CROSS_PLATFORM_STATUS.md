# Cross-Platform Testing - Implementation Status

**Last Updated**: 2025-11-05  
**Status**: Partially Working - Needs Parameter Registration

---

## Current State

### ✅ What's Working

1. **Infrastructure Complete**
   - Cross-platform test script exists: `scripts/cachedrect_crosshost_debug_macos.sh`
   - Non-interactive mode implemented (`NONINTERACTIVE=1`)
   - Comprehensive timeout protection on all SSH/SCP operations
   - Server startup successfully backgrounds without hanging
   - Documentation complete in `tests/e2e/CROSS_PLATFORM_TESTING.md`

2. **Server Side**
   - Remote server starts successfully on quartz.local
   - Runs on display :999 (port 6899) as configured
   - ContentCache scenarios execute correctly
   - Logs are generated properly

3. **Network Connectivity**
   - LAN mode works (direct connection)
   - SSH tunnel mode available
   - Auto-detection of connection mode functional

### ⚠️ What Needs Work

1. **ContentCacheSize Parameter Not Recognized** (CRITICAL)
   - Parameter defined in `vncviewer/parameters.h` (line 87)
   - Parameter definition exists in `vncviewer/parameters.cxx` (lines 247-251)
   - **BUT**: Not registered in viewer's parameter array for command-line parsing
   - Result: Viewer rejects `-ContentCacheSize=4` as "Unrecognized option"

2. **SERVER_READY Detection**
   - Script looks for "SERVER_READY" in `/tmp/cachedrect_server_stdout.log`
   - May not be written due to output redirection
   - Causes script to timeout waiting (60 seconds)
   - Not critical since server does start successfully

3. **End-to-End Automation**
   - Script progresses but can't complete full automated test
   - Requires ContentCacheSize parameter fix to test eviction

---

## Recent Commits

| Commit | Date | Description |
|--------|------|-------------|
| 67f0aaf2 | 2025-11-05 | Fix SSH hanging by backgrounding locally |
| 71ceb2ff | 2025-11-05 | Add timeouts to all SSH/SCP commands |
| 53a07593 | 2025-11-04 | Add non-interactive mode |
| 554a1c24 | 2025-11-04 | Add mandatory timeout requirement to WARP.md |
| fb403e61 | 2025-11-04 | Remove XQuartz references |
| 8e933733 | 2025-11-04 | Add comprehensive documentation |

---

## Next Steps to Complete

### Priority 1: Fix ContentCacheSize Parameter (Required for Testing)

**Problem**: The `ContentCacheSize` parameter is not recognized by the viewer at runtime.

**Root Cause**: Parameter is defined but not added to the viewer's saveable parameter array in `vncviewer/parameters.cxx`.

**Solution**:

1. **Add ContentCacheSize to parameter array** in `vncviewer/parameters.cxx`:

   ```cpp
   // Around line 267, add to parameterArray[]
   static core::VoidParameter* parameterArray[] = {
     /* Security */
   #ifdef HAVE_GNUTLS
     &rfb::CSecurityTLS::X509CA,
     &rfb::CSecurityTLS::X509CRL,
   #endif
     &rfb::SecurityClient::secTypes,
     /* Misc. */
     &reconnectOnError,
     &shared,
     /* Compression */
     &autoSelect,
     &fullColour,
     &lowColourLevel,
     &preferredEncoding,
     &customCompressLevel,
     &compressLevel,
     &noJpeg,
     &qualityLevel,
     /* ContentCache */  // ADD THIS SECTION
     &contentCacheSize,  // ADD THIS LINE
     /* Display */
     &fullScreen,
     // ... rest of array
   };
   ```

2. **Rebuild the viewer**:
   ```bash
   cd /Users/nickc/code/tigervnc
   make viewer
   ```

3. **Test parameter recognition**:
   ```bash
   build/vncviewer/njcvncviewer --help 2>&1 | grep -i contentcache
   # Should show ContentCacheSize parameter
   ```

4. **Test with parameter**:
   ```bash
   build/vncviewer/njcvncviewer -ContentCacheSize=4 quartz.local::6899
   # Should NOT show "Unrecognized option" error
   ```

**Expected Result**: Viewer accepts and uses the ContentCacheSize parameter.

---

### Priority 2: Improve SERVER_READY Detection (Nice to Have)

**Problem**: Script waits 60 seconds looking for "SERVER_READY" that may not be in the log.

**Options**:

**Option A**: Fix output redirection in server startup
```bash
# In cachedrect_crosshost_debug_macos.sh, line 31:
# Change:
timeout 30 ssh "${REMOTE}" "... >/tmp/cachedrect_server_stdout.log 2>&1 ..." 
# To:
timeout 30 ssh "${REMOTE}" "... 2>&1 | tee /tmp/cachedrect_server_stdout.log ..."
```

**Option B**: Check for running process instead of log message
```bash
# Replace SERVER_READY detection loop with:
for i in {1..30}; do
  if timeout 10 ssh "${REMOTE}" "ps aux | grep -q 'Xnjcvnc :${DISPLAY_NUM}.*-rfbport ${SERVER_PORT}'"; then
    echo "[ok] Remote server is running on :${DISPLAY_NUM}"
    break
  fi
  sleep 2
done
```

**Option C**: Check if port is listening
```bash
# Replace SERVER_READY detection loop with:
for i in {1..30}; do
  if timeout 10 ssh "${REMOTE}" "ss -tln | grep -q :${SERVER_PORT}"; then
    echo "[ok] Server is listening on port ${SERVER_PORT}"
    break
  fi
  sleep 2
done
```

**Recommendation**: Option C (port check) is most reliable.

---

### Priority 3: Test Complete End-to-End Flow

Once ContentCacheSize parameter is fixed:

1. **Run non-interactive test**:
   ```bash
   cd /Users/nickc/code/tigervnc
   NONINTERACTIVE=1 VIEWER_DURATION=30 ./scripts/cachedrect_crosshost_debug_macos.sh
   ```

2. **Verify test completes**:
   - Server starts on :999
   - Viewer connects with 4MB cache
   - Viewer runs for 30 seconds
   - Logs are collected
   - Comparison runs successfully

3. **Check for evictions**:
   ```bash
   # In viewer log:
   grep -i "eviction" /tmp/cachedrect_debug/viewer_*.log
   # Should show cache eviction messages
   ```

4. **Review comparison results**:
   - Script runs `compare_cachedrect_logs.py`
   - Should show cache activity and eviction stats

---

## Usage Examples

### Interactive Mode (Manual Testing)

```bash
cd /Users/nickc/code/tigervnc
./scripts/cachedrect_crosshost_debug_macos.sh
# Viewer window opens on your Mac
# Use it, then close when done
# Script retrieves logs and compares
```

### Non-Interactive Mode (Automated Testing)

```bash
cd /Users/nickc/code/tigervnc

# 30 second test
NONINTERACTIVE=1 VIEWER_DURATION=30 ./scripts/cachedrect_crosshost_debug_macos.sh

# 60 second test with verbose output
NONINTERACTIVE=1 VIEWER_DURATION=60 ./scripts/cachedrect_crosshost_debug_macos.sh

# Custom display and duration
DISPLAY_NUM=998 NONINTERACTIVE=1 VIEWER_DURATION=45 ./scripts/cachedrect_crosshost_debug_macos.sh
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `REMOTE` | `nickc@quartz.local` | SSH target for remote server |
| `REMOTE_DIR` | `/home/nickc/code/tigervnc` | Remote repo path |
| `MODE` | `auto` | Connection mode: `auto`, `lan`, `tunnel` |
| `DISPLAY_NUM` | `999` | X display number (port = 5900 + display) |
| `VIEWER_BIN` | `build/vncviewer/njcvncviewer` | Local viewer binary |
| `VIEWER_DURATION` | `60` | Seconds to run viewer (non-interactive) |
| `NONINTERACTIVE` | `0` | Set to `1` for automated mode |
| `LOCAL_LOG_DIR` | `/tmp/cachedrect_debug` | Local log directory |

---

## Known Issues

### Issue 1: ContentCacheSize Parameter Not Available

**Impact**: Cannot test with custom cache sizes (e.g., 4MB for eviction testing)

**Workaround**: None currently. Must fix parameter registration.

**Status**: Documented above in Priority 1.

### Issue 2: "Unknown parameter" warnings in viewer log

**Error Message**:
```
Parameters:  Failed to read line 27 in file "/Users/nickc/.vnc/default.tigervnc": Unknown parameter
Parameters:  Failed to read line 30 in file "/Users/nickc/.vnc/default.tigervnc": Unknown parameter
```

**Cause**: Configuration file has parameters not recognized by viewer build

**Impact**: None - warnings only, viewer works

**Workaround**: Edit or remove `~/.vnc/default.tigervnc` if desired

### Issue 3: "Failed to determine locale directory"

**Message**: `Failed to determine locale directory`

**Cause**: Viewer can't find locale files for translations

**Impact**: None - English messages still work

**Workaround**: Ignore or install locale files if needed

---

## Testing Checklist

Before considering cross-platform testing "complete", verify:

- [ ] ContentCacheSize parameter recognized by viewer
- [ ] Viewer accepts `-ContentCacheSize=4` without error
- [ ] Non-interactive mode completes without hanging
- [ ] Server starts and runs scenarios successfully
- [ ] Viewer connects to remote server
- [ ] Viewer runs for specified duration
- [ ] Viewer auto-terminates after duration
- [ ] Server logs retrieved successfully
- [ ] Client logs generated correctly
- [ ] Log comparison runs and produces output
- [ ] Cache evictions detected in logs (when using small cache)
- [ ] Remote server stopped after test
- [ ] No stale processes left behind

---

## Files Modified

### Core Implementation
- `vncviewer/parameters.h` - ContentCacheSize parameter declaration (DONE)
- `vncviewer/parameters.cxx` - ContentCacheSize definition (DONE, needs array registration)
- `common/rfb/DecodeManager.cxx` - ContentCacheSize usage (DONE)

### Testing Infrastructure
- `scripts/cachedrect_crosshost_debug_macos.sh` - Main test script (DONE)
- `scripts/server_only_cachedrect_test.py` - Remote server component (DONE)
- `scripts/compare_cachedrect_logs.py` - Log comparison (EXISTS)
- `tests/e2e/CROSS_PLATFORM_TESTING.md` - Documentation (DONE)
- `tests/e2e/CROSS_PLATFORM_STATUS.md` - This file (NEW)

### Documentation
- `WARP.md` - Added timeout requirements (DONE)
- `tests/e2e/README.md` - Added cross-platform reference (DONE)

---

## Success Criteria

The cross-platform testing infrastructure will be considered **complete** when:

1. ✅ Script can start remote server without hanging
2. ✅ All SSH/SCP operations have timeouts
3. ✅ Non-interactive mode runs to completion
4. ❌ Viewer accepts ContentCacheSize parameter (BLOCKED - needs parameter array fix)
5. ❌ End-to-end automated test completes successfully (BLOCKED - needs #4)
6. ❌ Eviction testing works with 4MB cache (BLOCKED - needs #4)
7. ✅ Documentation covers all use cases
8. ✅ Interactive mode works for manual testing

**Current Progress**: 5/8 criteria met (62.5%)

**Blocking Issue**: ContentCacheSize parameter registration

**Estimated Time to Complete**: ~30 minutes
1. Add parameter to array (5 min)
2. Rebuild viewer (10 min)
3. Test parameter recognition (5 min)
4. Run full end-to-end test (10 min)

---

## Quick Start for Next Developer

To pick up where this left off:

1. **Read this document** to understand current state

2. **Fix the parameter registration** (Priority 1 above):
   - Edit `vncviewer/parameters.cxx` line ~267
   - Add `&contentCacheSize,` to the parameter array
   - Rebuild: `make viewer`

3. **Test the fix**:
   ```bash
   build/vncviewer/njcvncviewer -ContentCacheSize=4 quartz.local::6899
   # Should connect without "Unrecognized option" error
   ```

4. **Run full test**:
   ```bash
   NONINTERACTIVE=1 VIEWER_DURATION=30 ./scripts/cachedrect_crosshost_debug_macos.sh
   # Should complete successfully
   ```

5. **Verify evictions**:
   ```bash
   grep -i "eviction" /tmp/cachedrect_debug/viewer_*.log
   # Should show eviction messages
   ```

6. **Update this document** with results

---

## Resources

- **Main Documentation**: `tests/e2e/CROSS_PLATFORM_TESTING.md`
- **E2E Tests**: `tests/e2e/README.md`
- **Safety Guidelines**: `WARP.md`
- **ContentCache Design**: `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`
- **Eviction Implementation**: `docs/CONTENTCACHE_ARC_EVICTION_SUMMARY.md`

---

## Contact

For questions about this implementation:
1. Check the documentation files listed above
2. Review commit history for context
3. Test manually with interactive mode first
4. Check logs in `/tmp/cachedrect_debug/` for diagnostics
