# E2E Cache Test Updates

## Summary

The e2e tests have been updated to properly test ContentCache and PersistentCache in isolation, ensuring only ONE cache protocol is active per connection.

## Changes Made

### 1. Framework Updates (`framework.py`)

Added `server_params` support to `VNCServer` class:

```python
def __init__(self, ..., server_params: Optional[Dict[str, str]] = None):
    self.server_params = server_params or {}
```

Server parameters are now appended to the server command line:
```python
for key, value in self.server_params.items():
    cmd.append(f'-{key}={value}')
```

### 2. Test Updates

#### `test_cpp_contentcache.py`
- **Client**: Sets `PersistentCache=0` (line 42)
- **Server**: Sets `EnablePersistentCache=0` via `server_params` (line 160)
- **Result**: ContentCache ONLY active

#### `test_cpp_persistentcache.py`
- **Client**: Sets `ContentCache=0` (line 42) and `PersistentCache=1` (line 43)
- **Server**: Sets `EnableContentCache=0` via `server_params` (line 162)
- **Result**: PersistentCache ONLY active

#### `test_cpp_cache_eviction.py`
- **Client**: Conditionally sets cache parameters based on `--cache-type` argument
  - ContentCache: `PersistentCache=0` (line 45)
  - PersistentCache: `ContentCache=0` (line 47)
- **Server**: Conditionally disables non-selected cache (lines 160-164)
- **Result**: Selected cache ONLY active

## Verification

### Test Run Example (ContentCache)

```bash
$ python3 test_cpp_contentcache.py --duration 20 --verbose
```

**Server Log Output**:
```
Config:      Set EnablePersistentCache(Bool) to off
SConnection: Client encodings: PersistentCache=0, ContentCache=1
SConnection: Using ContentCache protocol (PersistentCache not available)
ContentCache: Hit rate: 25.0% (1 hits, 3 misses, 4 total)
```

**Viewer Log Output**:
```
CConnection: Cache protocol: advertising ContentCache (-320)
```

✅ **Confirmed**: Only ContentCache is active on both client and server.

### Test Run Example (PersistentCache)

```bash
$ python3 test_cpp_persistentcache.py --duration 20 --verbose
```

**Server Log Output**:
```
Config:      Set EnableContentCache(Bool) to off
SConnection: Client encodings: PersistentCache=1, ContentCache=0
SConnection: Using PersistentCache protocol (ContentCache not available)
```

**Viewer Log Output**:
```
CConnection: Cache protocol: advertising PersistentCache (-321)
```

✅ **Confirmed**: Only PersistentCache is active on both client and server.

## Usage

### Running Tests with Specific Cache Protocols

```bash
# Test ContentCache only
./test_cpp_contentcache.py --duration 60

# Test PersistentCache only
./test_cpp_persistentcache.py --duration 60

# Test ContentCache eviction
./test_cpp_cache_eviction.py --cache-type content --duration 60

# Test PersistentCache eviction
./test_cpp_cache_eviction.py --cache-type persistent --duration 60
```

### Passing Custom Server Parameters

```python
# In your test code:
server = VNCServer(
    display, port, name, artifacts, tracker,
    geometry="1920x1080",
    server_params={
        'EnableContentCache': '0',      # Disable ContentCache
        'EnablePersistentCache': '1',   # Enable PersistentCache
        'PersistentCacheMinRectSize': '2048'  # Custom parameter
    }
)
```

## Configuration Options Reference

### Client-Side (Viewer)

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `ContentCache` | Bool | true | Enable ContentCache protocol |
| `ContentCacheSize` | Int | 2048 | Cache size in MB |
| `PersistentCache` | Bool | true | Enable PersistentCache protocol |
| `PersistentCacheSize` | Int | 2048 | Cache size in MB |

### Server-Side

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `EnableContentCache` | Bool | true | Enable ContentCache protocol |
| `ContentCacheSize` | Int | 2048 | Cache size in MB |
| `ContentCacheMaxAge` | Int | 0 | Max age in seconds (0=unlimited) |
| `ContentCacheMinRectSize` | Int | 4096 | Minimum pixels to cache |
| `EnablePersistentCache` | Bool | true | Enable PersistentCache protocol |
| `PersistentCacheMinRectSize` | Int | 4096 | Minimum pixels to cache |

## Protocol Selection Logic

When both protocols are enabled:

1. **Client advertises** both protocols in order of preference:
   - PersistentCache (encoding -321) - preferred
   - ContentCache (encoding -320)

2. **Server selects** first supported protocol from client's list

3. **Only ONE cache is used** per connection:
   - If PersistentCache selected → ContentCache disabled
   - If ContentCache selected → PersistentCache disabled

## Troubleshooting

### Low Hit Rates in Short Tests

Short-duration tests (< 30 seconds) may show low hit rates because:
- Initial content is always a cache miss (CachedRectInit)
- Need repeated content exposure for hits (CachedRect)
- Minimum ~2-3 cycles needed to see meaningful hit rates

**Solution**: Run tests with `--duration 60` or higher for reliable hit rate measurements.

### Checking Active Cache Protocol

Look for these log messages:

**Server log**:
```
SConnection: Using ContentCache protocol
  or
SConnection: Using PersistentCache protocol
```

**Client log**:
```
CConnection: Cache protocol: advertising ContentCache (-320)
CConnection: Cache protocol: advertising PersistentCache (-321)
```

### Verifying Cache Operations

**Server side** (both caches show statistics):
```
ContentCache: Hit rate: X.X% (N hits, M misses, T total)
  or
PersistentCache: Hit rate: X.X% (N hits, M misses, T total)
```

**Client side** (DecodeManager operations):
```
DecodeManager: Cache hit for cacheId=N
DecodeManager: Stored cached content: cacheId=N
```

## Related Files

- `/home/nickc/code/tigervnc/tests/e2e/framework.py` - Test framework with VNCServer
- `/home/nickc/code/tigervnc/tests/e2e/test_cpp_contentcache.py` - ContentCache test
- `/home/nickc/code/tigervnc/tests/e2e/test_cpp_persistentcache.py` - PersistentCache test
- `/home/nickc/code/tigervnc/tests/e2e/test_cpp_cache_eviction.py` - Eviction test (both caches)
- `/home/nickc/code/tigervnc/common/rfb/CConnection.cxx` - Client encoding advertisement
- `/home/nickc/code/tigervnc/common/rfb/EncodeManager.cxx` - Server cache selection
- `/home/nickc/code/tigervnc/vncviewer/parameters.cxx` - Client configuration parameters
- `/home/nickc/code/tigervnc/common/rfb/ServerCore.cxx` - Server configuration parameters

## Date

Last updated: 2025-11-12
