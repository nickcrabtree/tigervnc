# Automatic Debug Logging for ContentCache and PersistentCache

Both ContentCache and PersistentCache automatically log detailed debug information to temporary files for troubleshooting and analysis.

## Automatic Log Files

### Log File Creation

When the viewer starts, both caches automatically create debug log files in `/tmp/`:

```
ContentCache debug log: /tmp/contentcache_debug_1729789234_567.log
PersistentCache debug log: /tmp/persistentcache_debug_1729789234_568.log
```

**Filename format**: `<cachetype>_debug_<timestamp>_<milliseconds>.log`

### Log File Locations

- **ContentCache**: `/tmp/contentcache_debug_<timestamp>_<ms>.log`
- **PersistentCache**: `/tmp/persistentcache_debug_<timestamp>_<ms>.log`

The timestamp ensures unique filenames for each viewer session.

## What Gets Logged

### ContentCache Debug Log

Tracks:
- Constructor/destructor lifecycle
- Content lookups and insertions
- Hash computations
- Cache hits and misses
- ARC algorithm operations (T1↔T2 moves, evictions)
- Pixel buffer operations

Example entries:
```
[1729789234.567] === ContentCache Debug Log Started ===
[1729789234.568] ContentCache constructor ENTER: maxSizeMB=2048, maxAgeSec=0
[1729789234.569] ContentCache constructor EXIT: initialized successfully
[1729789245.123] findContent ENTER: hash=0xabcdef1234567890
[1729789245.124] findContent: cache HIT for hash=0xabcdef1234567890
[1729789245.125] findContent EXIT: returning entry at 0x7f8a12345678
[1729789256.789] === ContentCache Debug Log Ended ===
```

### PersistentCache Debug Log

Tracks:
- Constructor/destructor lifecycle
- Cache file path determination
- Disk load/save operations
- Hash-based lookups and stores
- Query batching
- ARC algorithm behavior

Example entries:
```
[1729789234.568] === PersistentCache Debug Log Started ===
[1729789234.569] GlobalClientPersistentCache constructor ENTER: maxSizeMB=2048
[1729789234.570] GlobalClientPersistentCache constructor EXIT: cacheFilePath=/home/user/.cache/tigervnc/persistentcache.dat
[1729789235.123] loadFromDisk: loading from /home/user/.cache/tigervnc/persistentcache.dat
[1729789235.456] loadFromDisk: loaded 1234 entries (123456789 bytes)
[1729789256.789] GlobalClientPersistentCache destructor ENTER: entries=1234
[1729789256.790] GlobalClientPersistentCache destructor EXIT
[1729789256.791] === PersistentCache Debug Log Ended ===
```

## Log Format

Each log entry has the format:
```
[<unix_timestamp>.<milliseconds>] <message>
```

- **Timestamp**: Unix epoch time (seconds since 1970-01-01)
- **Milliseconds**: Sub-second precision (000-999)
- **Message**: Human-readable log message

Example:
```
[1729789234.567] ContentCache constructor ENTER: maxSizeMB=2048, maxAgeSec=0
```

## Viewing Logs

### During Runtime

```bash
# Follow ContentCache log in real-time
tail -f /tmp/contentcache_debug_*.log

# Follow PersistentCache log in real-time
tail -f /tmp/persistentcache_debug_*.log

# Both logs together
tail -f /tmp/contentcache_debug_*.log /tmp/persistentcache_debug_*.log
```

### After Session

```bash
# Find all ContentCache logs
ls -lt /tmp/contentcache_debug_*.log | head -5

# Find all PersistentCache logs
ls -lt /tmp/persistentcache_debug_*.log | head -5

# View the most recent logs
cat /tmp/contentcache_debug_*.log | tail -100
cat /tmp/persistentcache_debug_*.log | tail -100
```

## Integration with VNC Logging

The debug logs are **separate from** the standard VNC logging system (`-Log` parameter). Both systems run independently:

- **Standard VNC logs**: Controlled by `-Log` parameter, goes to stderr/file
- **Debug tmpfiles**: Always enabled, automatic, separate files in `/tmp/`

### Example Session

```bash
# Start viewer
~/scripts/njcvncviewer_start.sh localhost:2

# Output shows both log files:
# ContentCache debug log: /tmp/contentcache_debug_1729789234_567.log
# PersistentCache debug log: /tmp/persistentcache_debug_1729789234_568.log

# You can also enable verbose VNC logging:
~/scripts/njcvncviewer_start.sh localhost:2 -Log "*:stderr:30,ContentCache:stderr:100,PersistentCache:stderr:100"

# Now you have:
# 1. VNC logs on stderr (verbose, for real-time monitoring)
# 2. ContentCache debug log in /tmp/ (detailed, for post-mortem analysis)
# 3. PersistentCache debug log in /tmp/ (detailed, for post-mortem analysis)
```

## Use Cases

### 1. Debugging Cache Corruption

**Problem**: Viewer displays garbled rectangles

**Debug approach**:
```bash
# Start viewer and reproduce issue
~/scripts/njcvncviewer_start.sh localhost:2

# After crash/exit, examine logs
grep -i "error\|corrupt\|invalid" /tmp/contentcache_debug_*.log
grep -i "error\|corrupt\|invalid" /tmp/persistentcache_debug_*.log

# Look for mismatched dimensions, invalid pointers, etc.
```

### 2. Performance Analysis

**Problem**: Slow cache operations

**Debug approach**:
```bash
# Examine log timestamps to find slow operations
cat /tmp/persistentcache_debug_*.log | grep -E "ENTER|EXIT" | tail -100

# Calculate time between ENTER/EXIT pairs
# (shows how long each operation took)
```

### 3. Cache Hit Rate Investigation

**Problem**: Unexpectedly low cache hit rate

**Debug approach**:
```bash
# Count lookups
grep "findContent ENTER" /tmp/contentcache_debug_*.log | wc -l

# Count hits
grep "cache HIT" /tmp/contentcache_debug_*.log | wc -l

# Count misses
grep "cache MISS" /tmp/contentcache_debug_*.log | wc -l
```

### 4. Disk Persistence Verification

**Problem**: Cache not persisting across restarts

**Debug approach**:
```bash
# First session - check save
~/scripts/njcvncviewer_start.sh localhost:2
# (use for a while, then exit)

grep "saveToDisk" /tmp/persistentcache_debug_*.log
# Should show: "saveToDisk: saving N entries to ..."

# Second session - check load
~/scripts/njcvncviewer_start.sh localhost:2

# Find the NEW log file (most recent timestamp)
ls -lt /tmp/persistentcache_debug_*.log | head -1

# Check if it loaded the cache
grep "loadFromDisk" /tmp/persistentcache_debug_<new_timestamp>.log
# Should show: "loadFromDisk: loaded N entries"
```

## Log Cleanup

Debug logs accumulate in `/tmp/` over time. Clean them periodically:

```bash
# Remove old ContentCache logs (older than 7 days)
find /tmp -name "contentcache_debug_*.log" -mtime +7 -delete

# Remove old PersistentCache logs (older than 7 days)
find /tmp -name "persistentcache_debug_*.log" -mtime +7 -delete

# Or remove all debug logs
rm /tmp/contentcache_debug_*.log
rm /tmp/persistentcache_debug_*.log
```

### Automatic Cleanup Script

Add to crontab for weekly cleanup:
```bash
# Clean up old cache debug logs weekly
0 3 * * 0 find /tmp -name "*cache_debug_*.log" -mtime +7 -delete
```

## Troubleshooting

### Problem: No log files created

**Possible causes**:
1. `/tmp/` is read-only or full
2. Permissions issue
3. Viewer crashed before initialization

**Check**:
```bash
# Test write access
touch /tmp/test_cache_log.log && rm /tmp/test_cache_log.log

# Check disk space
df -h /tmp

# Check permissions
ls -ld /tmp
# Should show: drwxrwxrwt (1777)
```

### Problem: Log file exists but is empty

**Possible causes**:
1. Cache not being used (protocol not enabled)
2. Viewer exited immediately
3. File opened but no log calls made

**Check**:
```bash
# Verify file was created with header
head -1 /tmp/contentcache_debug_*.log
# Should show: "=== ContentCache Debug Log Started ==="

# Check file modification time
stat /tmp/contentcache_debug_*.log
# Should match viewer session time
```

### Problem: Log files grow too large

**Mitigation**:
```bash
# Monitor log file sizes
du -h /tmp/*cache_debug_*.log

# If a log is huge (>100MB), truncate it
tail -10000 /tmp/contentcache_debug_<timestamp>.log > /tmp/cache_debug_truncated.log

# Or disable debug logging temporarily (requires code change)
# Comment out: PersistentCacheDebugLogger::getInstance().log(...) calls
```

## Performance Impact

- **Disk I/O**: ~1-2% overhead (writes are buffered and flushed)
- **CPU**: <1% overhead (timestamp formatting is minimal)
- **Memory**: ~4KB per log file (file buffer)

For production use, debug logging is acceptable as it:
- Uses buffered I/O
- Only writes on significant events (not per-pixel)
- Flushes after each write for crash safety

## Log Rotation

Debug logs do **not** automatically rotate. Each viewer session creates a new log file with a unique timestamp.

To implement rotation:
```bash
#!/bin/bash
# Rotate and compress old logs

# Keep only last 10 logs per cache type
ls -t /tmp/contentcache_debug_*.log | tail -n +11 | xargs rm -f
ls -t /tmp/persistentcache_debug_*.log | tail -n +11 | xargs rm -f

# Compress logs older than 1 day
find /tmp -name "*cache_debug_*.log" -mtime +1 ! -name "*.gz" -exec gzip {} \;
```

## Comparison: VNC Logs vs Debug Tmpfiles

| Feature | VNC Logs (-Log parameter) | Debug Tmpfiles |
|---------|---------------------------|----------------|
| **Activation** | Manual (command-line) | Automatic |
| **Location** | stderr or specified file | `/tmp/<cache>_debug_<timestamp>.log` |
| **Format** | VNC log format | Timestamped entries |
| **Verbosity** | Configurable (0-100) | Always full detail |
| **Purpose** | General VNC debugging | Cache-specific debugging |
| **Overhead** | Depends on level | ~1-2% (always on) |
| **Cleanup** | Manual or log rotation | Manual (files accumulate) |
| **Best for** | Real-time monitoring | Post-mortem analysis |

## Related Documentation

- `PERSISTENTCACHE_LOGGING.md`: PersistentCache logging details
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: ContentCache architecture
- `PERSISTENTCACHE_DESIGN.md`: PersistentCache architecture

## Future Enhancements

Potential improvements:
1. **Log level control**: Environment variable to disable debug logs
2. **Automatic rotation**: Keep only N most recent logs
3. **Compression**: Automatically gzip old logs
4. **Log aggregation**: Single file for both caches
5. **Performance counters**: Periodic statistics dumps
