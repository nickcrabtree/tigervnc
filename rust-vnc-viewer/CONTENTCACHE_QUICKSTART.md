# ContentCache Implementation - Quick Start Guide

## Current State ✅

A **functional Rust VNC viewer** (`njcvncviewer-rs`) is working with:
- Complete RFB protocol handshake
- egui GUI with framebuffer rendering
- Raw and CopyRect encoding support
- Mouse and keyboard input

> **Note**: ContentCache/PersistentCache support in the Rust viewer is currently **under development**. The design in this document represents a target architecture; refer to `docs/CONTENTCACHE_RUST_PARITY_PLAN.md` for the up‑to‑date parity status relative to the C++ viewer.

## Goal 🎯

Add ContentCache protocol support for **97-99% bandwidth reduction** on repeated content.

## What is ContentCache?

Instead of re-transmitting pixels that were seen before, the server:
1. Assigns each unique content a **cache ID** (u64)
2. Sends only the **20-byte ID** for repeated content
3. Client looks up cached pixels and blits them

**Example**: Switching windows that were displayed 5 minutes ago requires only 20 bytes instead of megabytes.

---

## Implementation Checklist

### Week 1: Protocol Support

**Files to create/modify:**
- [ ] `rfb-protocol/src/messages/cache.rs` (new, ~300 LOC)
- [ ] `rfb-protocol/src/messages/types.rs` (add constants)
- [ ] `rfb-protocol/src/messages/server.rs` (add enum variants)
- [ ] `njcvncviewer-rs/src/connection.rs` (add capability)

**What to implement:**
```rust
// Constants
pub const ENCODING_CACHED_RECT: i32 = 0xFFFFFE00;
pub const ENCODING_CACHED_RECT_INIT: i32 = 0xFFFFFE01;
pub const PSEUDO_ENCODING_CONTENT_CACHE: i32 = 0xFFFFFE10;

// Message types
pub struct CachedRect { pub cache_id: u64 }
pub struct CachedRectInit {
    pub cache_id: u64,
    pub actual_encoding: i32,
    // + encoded pixel data follows
}
```

**Tests to write:**
- Message serialization/deserialization
- Network byte order correctness
- Edge cases (zero cache ID, unknown encoding)

---

### Week 2: Client Cache

**Files to create:**
- [ ] `rfb-encodings/src/content_cache.rs` (~500 LOC)
- [ ] `rfb-encodings/src/cached_rect.rs` (~200 LOC)
- [ ] `rfb-encodings/src/cached_rect_init.rs` (~250 LOC)

**Cache structure:**
```rust
pub struct ContentCache {
    pixels: HashMap<u64, CachedPixels>,  // ID -> pixel data
    max_size_mb: usize,
    current_size_bytes: usize,
    hit_count: u64,
    miss_count: u64,
    lru_order: VecDeque<u64>,  // For eviction
}

pub struct CachedPixels {
    cache_id: u64,
    pixels: Vec<u8>,           // Decoded RGBA data
    width: u32,
    height: u32,
    stride: usize,
    last_used: Instant,
}
```

**Decoder flow:**

**CachedRect**:
1. Read cache_id from stream
2. Lookup in cache
3. If hit: blit pixels to framebuffer
4. If miss: return error (triggers refresh)

**CachedRectInit**:
1. Read cache_id and actual_encoding
2. Decode pixels using actual_encoding decoder
3. Store decoded pixels in cache
4. Blit to framebuffer

---

### Week 3: Integration

**Files to modify:**
- [ ] `njcvncviewer-rs/src/connection.rs` (~80 LOC changes)
  - Create ContentCache instance
  - Pass to decoders
  - Handle cache miss errors
  - Log statistics

**Decoder registry update:**
```rust
enum DecoderImpl {
    Raw(RawDecoder),
    CopyRect(CopyRectDecoder),
    CachedRect(CachedRectDecoder),      // NEW
    CachedRectInit(CachedRectInitDecoder), // NEW
}

impl DecoderRegistry {
    fn new() -> Self {
        let cache = Arc::new(Mutex::new(ContentCache::new(2048)));
        
        let mut decoders = HashMap::new();
        decoders.insert(ENCODING_RAW, DecoderImpl::Raw(RawDecoder));
        decoders.insert(ENCODING_COPY_RECT, DecoderImpl::CopyRect(CopyRectDecoder));
        decoders.insert(ENCODING_CACHED_RECT, 
            DecoderImpl::CachedRect(CachedRectDecoder::new(cache.clone())));
        decoders.insert(ENCODING_CACHED_RECT_INIT,
            DecoderImpl::CachedRectInit(CachedRectInitDecoder::new(cache.clone())));
        
        Self { decoders }
    }
}
```

**Configuration:**
```rust
#[derive(Parser)]
struct Args {
    // ... existing fields
    
    /// ContentCache size in MB (default: 2048)
    #[arg(long, default_value_t = 2048)]
    cache_size: usize,
    
    /// Disable ContentCache
    #[arg(long)]
    disable_cache: bool,
}
```

---

### Week 4: Testing

**Unit tests:**
```rust
#[tokio::test]
async fn test_cache_insert_and_lookup() {
    let mut cache = ContentCache::new(100);
    
    let pixels = CachedPixels {
        cache_id: 12345,
        pixels: vec![0xFF; 64 * 64 * 4],
        width: 64,
        height: 64,
        stride: 64,
        last_used: Instant::now(),
    };
    
    cache.insert(12345, pixels).unwrap();
    assert!(cache.lookup(12345).is_some());
}

#[tokio::test]
async fn test_lru_eviction() {
    let mut cache = ContentCache::new(1); // 1MB only
    
    // Fill cache
    for i in 0..100 {
        let pixels = CachedPixels { /* 100KB each */ };
        cache.insert(i, pixels).unwrap();
    }
    
    // First entries should be evicted
    assert!(cache.lookup(0).is_none());
    assert!(cache.lookup(99).is_some());
}
```

**Integration test:**
```bash
# 1. Start the e2e test harness (spawns :998 and :999 locally)
python3 ../tests/e2e/run_contentcache_test.py --verbose &

# 2. Run Rust viewer
cargo run --package njcvncviewer-rs -- -vv localhost:999

# 3. Observe logs:
# "CachedRect received: cache_id=123"
# "Cache hit: blitting 64x64 rect"
# "ContentCache stats: hit_rate=85%, entries=42, size=156MB"
```

---

## Protocol Flow Example

### Initial Content Transmission

```
Server                              Client
------                              ------
1. New content appears
2. Assign cache_id = 12345
3. Send CachedRectInit:
   - cache_id = 12345
   - encoding = Tight
   - [compressed pixels]  -------→ 4. Decode Tight pixels
                                   5. Store in cache[12345]
                                   6. Display on screen
```

### Repeated Content (Cache Hit)

```
Server                              Client
------                              ------
1. Same content appears again
2. Lookup: cache_id = 12345
3. Send CachedRect:
   - cache_id = 12345    -------→ 4. Lookup cache[12345]
   (only 20 bytes!)                5. Blit cached pixels
                                   6. Display on screen
                                   (No decode needed!)
```

### Cache Miss Recovery

```
Server                              Client
------                              ------
1. Send CachedRect:
   - cache_id = 99999    -------→ 2. Lookup cache[99999]
                                   3. MISS (evicted)
                                   4. Request refresh
                         ←------- 5. FramebufferUpdateRequest
                                      (incremental=false)
6. Send CachedRectInit:
   - cache_id = 99999
   - [pixels]            -------→ 7. Decode and cache
                                   8. Display
```

---

## Testing with C++ Server

### Server Setup

```bash
# Use the e2e test framework instead of a shared server
python3 ../tests/e2e/run_contentcache_test.py --verbose
# This starts isolated servers on :998 and :999
```

### Viewer Testing

```bash
# Run Rust viewer with verbose logging against :999
cd ~/code/tigervnc/rust-vnc-viewer
cargo run --package njcvncviewer-rs -- -vv localhost:999 2>&1 | tee /tmp/viewer.log

# Look for these in logs:
# - "Negotiated encodings: [0, 1, -496]"  (includes ContentCache)
# - "Received CachedRect: cache_id=..."
# - "Cache hit: ..."
# - "Cache miss: ..."
```

### Verification

**Expected behavior:**
1. Initial connection shows mostly CachedRectInit messages
2. After switching windows/scrolling and returning:
   - Should see CachedRect messages (20 bytes each)
   - Cache hit rate should be >80%
3. Network traffic should drop dramatically on cache hits

**Debugging:**
```bash
# Monitor network traffic
tcpdump -i lo -n port 6899 -w /tmp/vnc.pcap

# Analyze in Wireshark later
wireshark /tmp/vnc.pcap
```

---

## Success Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Cache hit rate** | >80% | Check logs: `ContentCache stats: hit_rate=...` |
| **Bandwidth on hit** | 20 bytes | Verify CachedRect size in tcpdump |
| **Bandwidth on miss** | Similar to current | CachedRectInit = normal encoding + 12 bytes |
| **Memory usage** | <2GB | Check logs: `ContentCache: size=...MB` |
| **No visual corruption** | 100% | Manual testing - switch windows repeatedly |

---

## Common Issues and Solutions

### Ordering and Synchronization

The C++ viewer previously suffered from a subtle ContentCache visual corruption bug when
cache store/replay operations were performed outside the normal decode pipeline. This was
fixed by synchronizing cache operations with the decode queue (see
`CONTENTCACHE_DESIGN_IMPLEMENTATION.md` and `PERSISTENTCACHE_DESIGN.md`).

When implementing or updating the Rust viewer’s ContentCache/PersistentCache support, make
sure that cache store/replay behaviour follows the **same ordering rules** as normal
rects, including CopyRect. In practice this means:

- Do not perform cache blits concurrently with active decodes of overlapping regions.
- Do not snapshot framebuffer pixels for caching while decodes that contribute to that
  region are still in flight.
- Validate correctness with the black-box screenshot e2e tests, comparing cache-on vs
  cache-off runs.

## Common Issues and Solutions

### Issue: Cache miss on every rect

**Symptom**: All CachedRects result in cache misses  
**Cause**: Cache ID mapping inconsistency  
**Fix**: 
- Verify server increments cache IDs monotonically
- Check client stores with same ID from CachedRectInit
- Add debug logs: `"Storing cache_id={} with {} bytes"`

### Issue: Visual corruption

**Symptom**: Pixels in wrong locations or colors  
**Cause**: Stride/format mismatch  
**Fix**:
- Verify stride is in **pixels**, not bytes
- Ensure pixel format matches between cache and framebuffer
- Check blit operation uses correct stride: `buffer.image_rect(dest, &pixels, stride)`

### Issue: Memory leak

**Symptom**: Memory grows indefinitely  
**Cause**: Eviction not working  
**Fix**:
- Ensure `evict_lru()` actually removes entries
- Update `current_size_bytes` on both insert and evict
- Add debug logs showing cache size changes

### Issue: Protocol error

**Symptom**: Connection drops, "Failed to read..."  
**Cause**: Incorrect message parsing  
**Fix**:
- Verify byte order (network = big-endian)
- Check all fields read/written in correct order
- Add hex dumps of message bytes for comparison

---

## Performance Benchmarking

### Baseline (without ContentCache)

```bash
# Capture 60 seconds of traffic
tcpdump -i lo port 6899 -w /tmp/baseline.pcap &
TCPDUMP_PID=$!
cargo run --package njcvncviewer-rs -- --disable-cache localhost:999 &
VIEWER_PID=$!

# Interact: switch windows, scroll, return to previous windows
sleep 60

kill $VIEWER_PID
kill $TCPDUMP_PID

# Measure
tcpdump -r /tmp/baseline.pcap | wc -l
# Example: 50,000 packets, 100 MB transferred
```

### With ContentCache

```bash
# Same test with ContentCache enabled
tcpdump -i lo port 6899 -w /tmp/cached.pcap &
cargo run --package njcvncviewer-rs -- localhost:999 &
# ... same interaction ...

# Compare
tcpdump -r /tmp/cached.pcap | wc -l
# Expected: 5,000 packets, 5 MB transferred
# = 90% reduction
```

---

## Next Steps After Week 4

Once ContentCache is working:

1. **Week 5-8**: Implement advanced encodings
   - Tight (JPEG + zlib compression)
   - ZRLE (zlib run-length encoding)
   - Hextile (tiled encoding)

2. **Week 9**: Upgrade to ARC eviction
   - T1/T2 lists for recency vs frequency
   - B1/B2 ghost lists for adaptivity
   - See [ARC_ALGORITHM.md](../ARC_ALGORITHM.md)

3. **Week 10-12**: Polish features
   - Touch gestures
   - Clipboard
   - Full-screen improvements

---

## References

Quick links to detailed documentation:

- **[RUST_VIEWER_STATUS.md](RUST_VIEWER_STATUS.md)** - Complete implementation plan
- **[../CONTENTCACHE_DESIGN_IMPLEMENTATION.md](../CONTENTCACHE_DESIGN_IMPLEMENTATION.md)** - C++ reference
- **[../CACHE_PROTOCOL_DESIGN.md](../CACHE_PROTOCOL_DESIGN.md)** - Protocol spec
- **[../ARC_ALGORITHM.md](../ARC_ALGORITHM.md)** - ARC algorithm details

---

**Ready to start!** Begin with Week 1, Task 1.1: Message Types
