# PersistentCache Disk Persistence Testing Guide

**Phase 7 Testing**: Verify cache load/save functionality

## Quick Test Procedure

### Test 1: Basic Persistence (Cache Survives Restart)

```bash
# 1. Clean slate - remove any existing cache
rm -f ~/.cache/tigervnc/persistentcache.dat

# 2. Start viewer and connect (let it run for a bit to populate cache)
~/scripts/njcvncviewer_start.sh localhost:2

# 3. After some usage, cleanly exit viewer (Ctrl+Q or close window)
# This should trigger saveToDisk()

# 4. Verify cache file was created
ls -lh ~/.cache/tigervnc/persistentcache.dat
# Should see file with size > 0

# 5. Check file header (first 8 bytes should be magic + version)
hexdump -C ~/.cache/tigervnc/persistentcache.dat | head -5
# Should see: 43 56 43 50 (CVCP in little-endian = PCVC reversed)
#             01 00 00 00 (version 1)

# 6. Restart viewer and check logs
~/scripts/njcvncviewer_start.sh localhost:2 2>&1 | tee /tmp/test.log
# Look for: "PersistentCache: loading from ~/.cache/tigervnc/persistentcache.dat"
# Look for: "PersistentCache: loaded N entries"

# 7. Verify immediate cache hits
# Monitor logs for "PersistentCache" messages showing hits
grep -i "PersistentCache" /tmp/test.log
```

### Test 2: Corruption Recovery

```bash
# 1. Corrupt the cache file (wrong magic number)
echo "CORRUPT" > ~/.cache/tigervnc/persistentcache.dat

# 2. Start viewer
~/scripts/njcvncviewer_start.sh localhost:2 2>&1 | tee /tmp/corrupt_test.log

# 3. Check logs - should see error and fallback
grep -i "persistentcache.*invalid magic" /tmp/corrupt_test.log
# Should see: "PersistentCache: invalid magic number..."
# Viewer should continue without crashing

# 4. On exit, should create fresh cache
ls -lh ~/.cache/tigervnc/persistentcache.dat
# File should be larger now (valid cache written)
```

### Test 3: Cache Size Limits

```bash
# 1. Clean start
rm -f ~/.cache/tigervnc/persistentcache.dat

# 2. Use viewer extensively to fill cache (browse large desktop, scrolling, etc.)
~/scripts/njcvncviewer_start.sh localhost:2

# 3. After exit, check cache size
du -h ~/.cache/tigervnc/persistentcache.dat

# 4. Restart - verify it stops loading at max size
~/scripts/njcvncviewer_start.sh localhost:2 2>&1 | grep "reached max size"
```

## Expected Log Output

### Successful Load (DecodeManager constructor)
```
PersistentCache created with ARC: maxSize=2048MB, path=/home/user/.cache/tigervnc/persistentcache.dat
Client PersistentCache initialized: 2048MB (ARC-managed)
PersistentCache: loading from /home/user/.cache/tigervnc/persistentcache.dat
PersistentCache: header valid, loading 1234 entries (123456789 bytes)
PersistentCache: loaded 1234 entries (123456789 bytes) from disk
PersistentCache loaded from disk
```

### Fresh Start (No Cache File)
```
PersistentCache created with ARC: maxSize=2048MB, path=/home/user/.cache/tigervnc/persistentcache.dat
Client PersistentCache initialized: 2048MB (ARC-managed)
PersistentCache: no cache file found at /home/user/.cache/tigervnc/persistentcache.dat (fresh start)
PersistentCache starting fresh (no cache file or load failed)
```

### Successful Save (DecodeManager destructor)
```
PersistentCache: saving 1234 entries to /home/user/.cache/tigervnc/persistentcache.dat
PersistentCache: saved 1234 entries to disk
PersistentCache saved to disk
```

### Corruption Detected
```
PersistentCache: loading from /home/user/.cache/tigervnc/persistentcache.dat
PersistentCache: invalid magic number 0x12345678 (expected 0x50435643)
PersistentCache starting fresh (no cache file or load failed)
```

## Verification Checklist

- [ ] Cache file created in `~/.cache/tigervnc/persistentcache.dat`
- [ ] File has valid header (magic = 0x50435643, version = 1)
- [ ] Cache survives viewer restart
- [ ] Loaded entries show up in ARC statistics
- [ ] Corrupted cache handled gracefully (no crash)
- [ ] Fresh cache created after corruption
- [ ] Directory automatically created if missing
- [ ] Cache respects max size during load

## Performance Benchmarks

**Expected timings** (on SSD with 2GB cache):

- Load 100,000 entries: ~2-3 seconds
- Save 100,000 entries: ~3-4 seconds
- Startup overhead: ~500ms for empty/small cache

**Measure actual times:**
```bash
# Time the load
time ~/scripts/njcvncviewer_start.sh localhost:2
# (exit immediately after connection)

# Check timestamps in logs
grep "PersistentCache" ~/.vnc/*.log | grep -E "loading|loaded|saving|saved"
```

## Debugging Tips

### Enable Verbose Logging

Edit `~/.vnc/config` or add command-line args:
```
Log=*:stderr:30,PersistentCache:stderr:100
```

### Inspect Cache File Manually

```bash
# Show header
hexdump -C ~/.cache/tigervnc/persistentcache.dat | head -10

# Count entries in file (rough estimate)
# Each entry is hashLen(1) + hash(16) + metadata(~40) + pixels
stat -f%z ~/.cache/tigervnc/persistentcache.dat
# Compare to expected: 64 (header) + entries*size + 32 (checksum)

# Check modification time
stat ~/.cache/tigervnc/persistentcache.dat
```

### Watch Cache Updates in Real-Time

```bash
# Terminal 1: Monitor cache file
watch -n 1 'ls -lh ~/.cache/tigervnc/persistentcache.dat'

# Terminal 2: Run viewer
~/scripts/njcvncviewer_start.sh localhost:2
```

## Known Limitations (Phase 7)

1. **No checksum verification**: Checksum field is written as zeros, not verified on load
2. **Synchronous I/O**: Blocks on startup/shutdown (acceptable for now)
3. **No incremental save**: Entire cache rewritten on each save
4. **No compression**: Cache file stores raw pixel data (could add zlib in future)

## Next Steps After Testing

Once basic persistence is verified:

1. Add SHA-256 checksum computation and verification
2. Consider lazy loading (background thread)
3. Add cache file versioning for migration
4. Implement incremental/append-only saves
5. Add metrics: load/save times, compression ratio, hit rate persistence

## Success Criteria

Phase 7 is successful if:

- ✅ Cache file is created on first save
- ✅ Cache is loaded on subsequent starts
- ✅ Viewer doesn't crash on corrupted cache
- ✅ Cross-session persistence works (restart scenario)
- ✅ Build completes without errors
- ✅ No memory leaks (valgrind clean)
