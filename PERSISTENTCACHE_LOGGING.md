# PersistentCache Debug Logging Guide

PersistentCache now has comprehensive debug logging matching ContentCache's verbosity level.

## Logging Levels

### Info Level (Default)
Shows initialization, lifecycle, and summary statistics:

```
Client PersistentCache initialized: 2048MB (ARC-managed)
PersistentCache: loading from ~/.cache/tigervnc/persistentcache.dat
PersistentCache: header valid, loading 1234 entries (123456789 bytes)
PersistentCache: loaded 1234 entries (123456789 bytes) from disk
PersistentCache loaded from disk
PersistentCache saved to disk

Client-side PersistentCache statistics:
  Protocol operations (PersistentCachedRect received):
    Lookups: 5432, Hits: 4123 (75.9%)
    Misses: 1309, Queries sent: 1309
  ARC cache performance:
    Total entries: 1234, Total bytes: 512 MiB
    Cache hits: 4123, Cache misses: 1309, Evictions: 234
    T1 (recency): 567 entries, T2 (frequency): 667 entries
    B1 (ghost-T1): 123 entries, B2 (ghost-T2): 89 entries
    ARC parameter p (target T1 bytes): 256 MiB
```

### Debug Level (Verbose)
Shows per-rectangle operations with hash prefixes:

#### Server-Side (EncodeManager)
```
PersistentCache protocol HIT: rect [100,200-500,600] hash=a1b2c3d4... saved 48020 bytes
PersistentCache MISS: rect [100,200-500,600] - client doesn't have hash, falling back to regular encoding
```

#### Client-Side (DecodeManager)
```
PersistentCache HIT: rect [100,200-500,600] hash=a1b2c3d4... cached=400x400 stride=400
PersistentCache MISS: rect [100,200-500,600] hash=a1b2c3d4... (len=16), queuing query
PersistentCache STORE: rect [100,200-500,600] hash=a1b2c3d4... (len=16)
PersistentCache STORE details: bpp=32 stridePx=400 pixelBytes=640000
Flushing 10 pending PersistentCache queries
```

#### Cache Operations (GlobalClientPersistentCache)
```
PersistentCache created with ARC: maxSize=2048MB, path=~/.cache/tigervnc/persistentcache.dat
PersistentCache inserted: hashLen=16 size=640000 bytes, rect=400x400, T1=123456/456 T2=234567 p=512000000
PersistentCache cleared
PersistentCache destroyed: 1234 entries, T1=567 T2=667
```

## Enabling Debug Logging

### Command Line (Viewer)
```bash
~/scripts/njcvncviewer_start.sh localhost:2 -Log "*:stderr:30,PersistentCache:stderr:100"
```

### Command Line (Server)
```bash
Xnjcvnc :2 -Log "*:stderr:30,PersistentCache:stderr:100,EncodeManager:stderr:100"
```

### Configuration File (~/.vnc/config)
```ini
Log=*:stderr:30,PersistentCache:stderr:100,EncodeManager:stderr:100,DecodeManager:stderr:100
```

## Log Message Format

### Hash Display
Hashes are displayed as the first 8 bytes in hexadecimal:
- Full hash: `a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6` (16 bytes)
- Logged as: `a1b2c3d4e5f6a7b8...`

This provides enough uniqueness for debugging while keeping logs readable.

### Rectangle Format
Rectangles use TigerVNC's standard format:
- `[x1,y1-x2,y2]` where (x1,y1) is top-left, (x2,y2) is bottom-right
- Example: `[100,200-500,600]` = 400x400 rectangle at position (100,200)

## Comparison: ContentCache vs PersistentCache

| Feature | ContentCache | PersistentCache |
|---------|--------------|-----------------|
| **Hit Logging** | ✅ `ContentCache protocol hit: rect [x,y-x,y] cacheId=N` | ✅ `PersistentCache protocol HIT: rect [x,y-x,y] hash=...` |
| **Miss Logging** | ❌ (silent) | ✅ `PersistentCache MISS: rect [x,y-x,y] - client doesn't have hash` |
| **Store Logging** | ✅ `ContentCache insert: rect [x,y-x,y], hash=X, cacheId=N` | ✅ `PersistentCache STORE: rect [x,y-x,y] hash=...` |
| **Statistics** | ✅ ARC stats at shutdown | ✅ ARC stats at shutdown + query stats |
| **Disk I/O** | ❌ N/A | ✅ Load/save operations logged |
| **Hash Display** | ✅ `hash=llx` (64-bit) | ✅ `hash=xx...` (first 8 bytes) |
| **Bytes Saved** | ✅ Shown in hit message | ✅ Shown in hit message |

## Debug Scenarios

### Scenario 1: Verify Cache Hits
**Goal**: Confirm PersistentCache is working

```bash
# Start with verbose logging
~/scripts/njcvncviewer_start.sh localhost:2 -Log "*:stderr:30,PersistentCache:stderr:100" 2>&1 | tee /tmp/pc.log

# After using for a while, check for hits
grep "PersistentCache HIT" /tmp/pc.log | wc -l
grep "PersistentCache MISS" /tmp/pc.log | wc -l

# Calculate hit rate
echo "scale=2; $(grep -c 'HIT' /tmp/pc.log) * 100 / $(grep -c 'HIT\|MISS' /tmp/pc.log)" | bc
```

### Scenario 2: Track Query Batching
**Goal**: Verify queries are batched efficiently

```bash
# Watch for query flushes
grep "Flushing.*pending.*queries" /tmp/pc.log

# Example output:
# Flushing 10 pending PersistentCache queries
# Flushing 7 pending PersistentCache queries
```

### Scenario 3: Monitor Cache Growth
**Goal**: See cache filling up over time

```bash
# Watch insert operations
grep "PersistentCache inserted" /tmp/pc.log | tail -20

# Example shows T1/T2 growing:
# PersistentCache inserted: hashLen=16 size=640000 bytes, rect=400x400, T1=640000/1 T2=0 p=0
# PersistentCache inserted: hashLen=16 size=640000 bytes, rect=400x400, T1=1280000/2 T2=0 p=0
# ...
```

### Scenario 4: Debug Corruption
**Goal**: Verify corruption handling

```bash
# Corrupt cache
echo "CORRUPT" > ~/.cache/tigervnc/persistentcache.dat

# Start viewer with debug logging
~/scripts/njcvncviewer_start.sh localhost:2 -Log "*:stderr:30,PersistentCache:stderr:100" 2>&1 | grep -A 5 "invalid magic"

# Should see:
# PersistentCache: invalid magic number 0x50524f43 (expected 0x50435643)
# PersistentCache starting fresh (no cache file or load failed)
```

## Performance Impact

Debug logging has minimal performance impact:
- **Info level**: ~1% overhead (only lifecycle and summary)
- **Debug level**: ~5-10% overhead (per-rectangle logging)

For production use, stick with info level or disable PersistentCache logging entirely:
```bash
Log=*:stderr:30,PersistentCache:stderr:30
```

## Log Filtering Tips

```bash
# Show only hits (successes)
grep "PersistentCache HIT" /tmp/pc.log

# Show only misses (cache faults)
grep "PersistentCache MISS" /tmp/pc.log

# Show cache modifications (stores)
grep "PersistentCache STORE\|inserted" /tmp/pc.log

# Show ARC activity (evictions, adaptations)
grep "T1=\|T2=\|p=" /tmp/pc.log

# Show disk I/O
grep "loading from\|loaded.*from disk\|saving.*to\|saved.*to disk" /tmp/pc.log

# Show statistics only
grep "PersistentCache statistics" -A 20 /tmp/pc.log
```

## Troubleshooting with Logs

### Problem: No cache hits after restart
**Check**:
```bash
grep "PersistentCache: loaded" /tmp/pc.log
# Should show entries loaded

grep "PersistentCache HIT" /tmp/pc.log
# If empty, client isn't receiving PersistentCachedRect messages
```

**Possible causes**:
- Server doesn't support PersistentCache (check server logs for protocol negotiation)
- Client didn't advertise support (check CConnection::updateEncodings)
- Cache file didn't save (check for "saved to disk" message)

### Problem: High miss rate
**Check**:
```bash
# Calculate hit rate
hits=$(grep -c "PersistentCache HIT" /tmp/pc.log)
misses=$(grep -c "PersistentCache MISS" /tmp/pc.log)
echo "Hit rate: $(echo "scale=1; $hits * 100 / ($hits + $misses)" | bc)%"
```

**Possible causes**:
- Cache too small (check eviction messages)
- Content changing frequently (expected)
- Server not tracking client hashes (check handlePersistentHashList)

### Problem: Cache not persisting
**Check**:
```bash
# On exit, should see
grep "PersistentCache saved to disk" /tmp/pc.log

# Check file exists
ls -lh ~/.cache/tigervnc/persistentcache.dat
```

**Possible causes**:
- Viewer crashed (no clean shutdown, no save)
- Permission denied (check file/directory permissions)
- Disk full (check `df -h`)

## Related Documentation

- `PERSISTENTCACHE_DESIGN.md`: Protocol specification and architecture
- `PERSISTENTCACHE_TESTING.md`: Testing procedures and verification
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md`: ContentCache comparison
