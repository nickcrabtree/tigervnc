# PersistentCache Protocol: Implementation Plan for Rust VNC Viewer

**Author**: TigerVNC Rust Implementation Team  
**Date**: 2025-10-24  
**Status**: Draft - Implementation Roadmap  
**Related Documents**:
- [PERSISTENTCACHE_RUST.md](docs/protocol/PERSISTENTCACHE_RUST.md) - Protocol specification
- [CONTENTCACHE_QUICKSTART.md](CONTENTCACHE_QUICKSTART.md) - ContentCache background
- [C++ Reference: PERSISTENTCACHE_DESIGN.md](../PERSISTENTCACHE_DESIGN.md)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Architecture Overview](#2-architecture-overview)
3. [Protocol Specification](#3-protocol-specification)
4. [Implementation Phases](#4-implementation-phases)
5. [Phase A: Dependencies and Scaffolding](#phase-a-dependencies-and-scaffolding)
6. [Phase B: SHA-256 Hashing and Stride Handling](#phase-b-sha-256-hashing-and-stride-handling)
7. [Phase C: ARC Cache with Byte-Sized Capacity](#phase-c-arc-cache-with-byte-sized-capacity)
8. [Phase D: Disk Persistence Store and I/O](#phase-d-disk-persistence-store-and-io)
9. [Phase E: Wire Protocol and Negotiation](#phase-e-wire-protocol-and-negotiation)
10. [Phase F: Batch Query Aggregator](#phase-f-batch-query-aggregator)
11. [Phase G: Viewer Integration and Rendering Path](#phase-g-viewer-integration-and-rendering-path)
12. [Phase H: Error Handling and Graceful Degradation](#phase-h-error-handling-and-graceful-degradation)
13. [Phase I: Testing Strategy and Cross-Session Validation](#phase-i-testing-strategy-and-cross-session-validation)
14. [Phase J: Performance Targets and Optimization](#phase-j-performance-targets-and-optimization)
15. [Troubleshooting Guide](#troubleshooting-guide)
16. [Timeline and Milestones](#timeline-and-milestones)
17. [Appendix A: C++ Reference Points](#appendix-a-c-reference-points)

---

## Quick Start

Once implemented, PersistentCache will be available through feature flags:

```bash
# Build with PersistentCache support
cargo build --release --features persistent-cache

# Run viewer with PersistentCache enabled
cargo run --release --features persistent-cache -- localhost:2

# Verify in logs
# Expected output:
# [INFO  rfb_encodings::persistent_cache] PersistentCache enabled, loaded 1,234 entries (156 MB) from disk
# [DEBUG rfb_encodings::persistent_cache] Cache hit: id=a1b2c3d4... (64x64 rect)
```

**Success Indicators**:
- Log shows "PersistentCache enabled"
- On reconnect, immediate cache hits from disk before any server data arrives
- Cache hit rate >80% for previously-visited screen regions

---

## 1. Executive Summary

### What is PersistentCache?

**ContentCache** (existing in this viewer) provides content-addressable caching using **server-assigned numeric IDs** within a single session. It achieves 97-99% bandwidth reduction by referencing cached content instead of re-encoding pixels.

**PersistentCache** extends this concept with two critical enhancements:

1. **Content-based addressing**: Uses **SHA-256 hashes (truncated to 16 bytes)** as stable cache keys instead of server-assigned IDs
2. **Cross-session persistence**: Stores decoded rectangles to disk (`~/.cache/tigervnc/rust-viewer/persistentcache-*.dat`) so they survive client restarts

| Feature | ContentCache | PersistentCache |
|---------|--------------|-----------------|
| **Cache key** | Server-assigned u64 | SHA-256 hash (16 bytes) |
| **Persistence** | Session-only (RAM) | Disk-backed |
| **Cross-server** | No | Yes (same content → same hash) |
| **Protocol** | `-320` pseudo-encoding | `-321` pseudo-encoding |
| **Encodings** | `-512`, `-511` | `102`, `103` |

### Benefits for the Rust Viewer

1. **Instant reconnects**: On viewer restart, the cache is loaded from disk. Previously-seen screen regions appear instantly with zero server data transfer.

2. **Bandwidth elimination**: First connection saves ~97-99%. Subsequent connections save ~99.9% for cached content (only 19-byte references vs KB-MB of encoded data).

3. **Server CPU relief**: Server skips encoding for cached rectangles, referencing them by hash instead.

4. **Cross-server efficiency**: Connect to multiple servers showing similar content (e.g., same IDE theme, same documentation) and benefit from cache hits across servers.

### Goals and Non-Goals

**Goals**:
- ✅ Protocol negotiation preferring `-321` over `-320`
- ✅ Correct SHA-256 hashing with **stride in pixels** (not bytes!)
- ✅ ARC (Adaptive Replacement Cache) with T1/T2/B1/B2 lists and byte-sized capacity tracking
- ✅ On-disk format with checksums and corruption recovery
- ✅ Batch query aggregation (reduce roundtrips)
- ✅ Size tracking and eviction in **bytes**, not entry count

**Non-Goals**:
- ❌ Server-side persistence (server ephemeral, client persistent)
- ❌ Multi-user shared cache (one cache per viewer instance)
- ❌ Content deduplication across different pixel formats (format-specific hashes)

### Critical Gotchas

> **⚠️ STRIDE IS IN PIXELS, NOT BYTES!**
>
> This was the source of a critical bug in the C++ implementation (Oct 7, 2025) that caused hash collisions and visual corruption.
>
> ```rust
> // ❌ WRONG - treats stride as bytes
> let row_offset = y * stride;
>
> // ✅ CORRECT - multiply by bytes_per_pixel
> let bytes_per_pixel = pixel_format.bits_per_pixel / 8;
> let row_offset = y * stride * bytes_per_pixel;
> ```

> **⚠️ Cache ID is 16 bytes (not 32)**
>
> SHA-256 produces 32 bytes, but we **truncate to 16 bytes** for efficiency. This provides 2^128 possible values, far exceeding collision risks for our use case.

> **⚠️ Track capacity in BYTES, not entry count**
>
> ARC accounting, size limits, and eviction decisions must all use **bytes of pixel payload**, not number of entries. A cache holding 10,000 small tiles is very different from 100 large screen captures.

> **⚠️ Negotiation order matters**
>
> Client MUST send `-321` before `-320` in `SetEncodings`. Server prefers the first supported capability in order. If `-320` appears first, the server will never see `-321`.

### Success Criteria

After reading this section, you should be able to:
- [ ] Explain the difference between ContentCache and PersistentCache
- [ ] Describe the primary benefits (instant reconnects, bandwidth elimination, cross-server hits)
- [ ] Identify the four critical gotchas and their implications
- [ ] Understand why persistence + content hashing enables new capabilities

---

## 2. Architecture Overview

### Proposed Rust Module Structure

```
rfb-encodings/src/persistent_cache/
├── mod.rs              # Public API, GlobalClientPersistentCache
├── hashing.rs          # SHA-256 content hashing with stride handling
├── arc.rs              # ARC cache with T1/T2/B1/B2 lists (byte-sized)
├── store.rs            # Disk persistence: format, I/O, checksums
├── wire.rs             # Protocol constants, message serialization
├── metrics.rs          # Counters, histograms, periodic logs
└── config.rs           # Configuration: sizes, paths, feature flags
```

**Public API** (`mod.rs`):
```rust
pub struct GlobalClientPersistentCache {
    arc: Arc<RwLock<ArcCache>>,
    store: Arc<Mutex<DiskStore>>,
    metrics: Arc<Metrics>,
    config: Config,
}

impl GlobalClientPersistentCache {
    pub fn new(config: Config) -> Result<Self>;
    pub fn load_from_disk() -> Result<Self>;
    pub fn get(&self, cache_id: &CacheId) -> Option<CachedEntry>;
    pub fn insert(&self, cache_id: CacheId, entry: CachedEntry) -> Result<()>;
    pub fn save_to_disk(&self) -> Result<()>;
    pub fn stats(&self) -> CacheStats;
}

pub type CacheId = [u8; 16];

pub struct CachedEntry {
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: usize,  // CRITICAL: in pixels, not bytes!
}
```

### Integration Points

#### 1. **Decoder Integration**

Modify `rfb-encodings/src/cached_rect.rs` and `cached_rect_init.rs`:

```rust
// cached_rect.rs - Handle cache hits (encoding 102)
pub async fn decode(&self, stream, rect, pf, buffer) -> Result<()> {
    let cache_id = stream.read_bytes(16).await?; // 16-byte hash
    
    if let Some(cached) = self.persistent_cache.get(&cache_id) {
        // Cache HIT: blit from cache
        buffer.image_rect(rect, &cached.pixels, cached.stride_pixels)?;
        Ok(())
    } else {
        // Cache MISS: enqueue batch request
        self.batch_aggregator.request_id(cache_id).await?;
        Err(anyhow!("Cache miss for ID {:?}, queued request", cache_id))
    }
}

// cached_rect_init.rs - Handle cache misses (encoding 103)
pub async fn decode(&self, stream, rect, pf, buffer) -> Result<()> {
    let cache_id = stream.read_bytes(16).await?;
    let actual_encoding = stream.read_i32().await?;
    
    // Decode using actual_encoding decoder
    let decoded_pixels = self.decode_inner(stream, rect, actual_encoding, pf).await?;
    
    // Compute hash (should match cache_id)
    let computed_hash = compute_rect_hash(&decoded_pixels, rect.width, rect.height, 
                                          rect.width, pf.bytes_per_pixel());
    
    // Store in cache
    let entry = CachedEntry {
        pixels: decoded_pixels,
        format: *pf,
        width: rect.width,
        height: rect.height,
        stride_pixels: rect.width as usize,
    };
    self.persistent_cache.insert(cache_id, entry)?;
    
    // Blit to framebuffer
    buffer.image_rect(rect, &entry.pixels, entry.stride_pixels)?;
    Ok(())
}
```

#### 2. **Capability Negotiation**

Modify `rfb-client/src/connection.rs` where `SetEncodings` is constructed:

```rust
fn build_encodings_list(&self) -> Vec<i32> {
    vec![
        // Standard encodings
        ENCODING_TIGHT,
        ENCODING_ZRLE,
        ENCODING_HEXTILE,
        ENCODING_RRE,
        ENCODING_COPYRECT,
        ENCODING_RAW,
        
        // Pseudo-encodings (ORDER MATTERS!)
        PSEUDO_ENCODING_PERSISTENT_CACHE,  // -321 (FIRST!)
        PSEUDO_ENCODING_CONTENT_CACHE,     // -320 (fallback)
        PSEUDO_ENCODING_LAST_RECT,         // -224
        PSEUDO_ENCODING_DESKTOP_SIZE,      // -223
    ]
}
```

**Why order matters**: The server prefers the first supported capability in the list. If `-320` is listed before `-321`, the server will select ContentCache mode and ignore PersistentCache.

#### 3. **Batch Query Aggregator**

Sits between the decoder (which discovers cache misses) and the outbound writer:

```rust
pub struct BatchAggregator {
    pending: Arc<Mutex<HashSet<CacheId>>>,
    flush_threshold_count: usize,    // e.g., 64 IDs
    flush_threshold_bytes: usize,    // e.g., 2048 bytes
    flush_timeout: Duration,         // e.g., 5 ms
}

impl BatchAggregator {
    pub async fn request_id(&self, id: CacheId) {
        let mut pending = self.pending.lock().unwrap();
        pending.insert(id);
        
        if self.should_flush(&pending) {
            self.flush_batch(&pending).await;
            pending.clear();
        }
    }
    
    fn should_flush(&self, pending: &HashSet<CacheId>) -> bool {
        pending.len() >= self.flush_threshold_count ||
        pending.len() * 16 >= self.flush_threshold_bytes ||
        /* timeout triggered */
    }
}
```

#### 4. **Pixel Pipeline Hook**

After decoding any rectangle, compute its hash:

```rust
pub fn compute_rect_hash(
    pixels: &[u8],
    width: usize,
    height: usize,
    stride_pixels: usize,
    bytes_per_pixel: usize,
) -> CacheId {
    let mut hasher = Sha256::new();
    let stride_bytes = stride_pixels * bytes_per_pixel;  // CRITICAL!
    let row_bytes = width * bytes_per_pixel;
    
    for y in 0..height {
        let row_start = y * stride_bytes;
        let row_end = row_start + row_bytes;
        hasher.update(&pixels[row_start..row_end]);
    }
    
    let result = hasher.finalize();
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&result[..16]);  // Truncate to 16 bytes
    hash
}
```

### Data Flow Narrative

#### **On Connect**

1. **Load disk cache**: `GlobalClientPersistentCache::load_from_disk()` scans the cache file, builds an in-memory index (CacheId → file offset), and verifies checksums.

2. **Negotiate capabilities**: Client sends `SetEncodings` with `-321, -320, ...`. Server replies indicating support for `-321` (persistent) or `-320` (session-only).

3. **Enforce byte budget**: ARC cache initializes with configured capacity (e.g., 2048 MB). Disk store may be larger; evictions prune both RAM and disk.

#### **On Server References Rectangle by ID**

1. **Receive CachedRectRef** (encoding 102): Read 16-byte `cache_id` from stream.

2. **Check ARC cache**: Look up `cache_id` in T1 or T2 lists.
   - **HIT in T1**: Move entry to T2 (frequency promotion).
   - **HIT in T2**: Move to MRU position in T2.
   - **MISS**: Check if ID is in ghost lists (B1/B2) for adaptation, then enqueue batch request.

3. **Blit or queue**: If hit, blit cached pixels to framebuffer. If miss, aggregate request in `BatchAggregator`.

#### **On Miss Response**

1. **Receive CachedRectInit** (encoding 103): Read `cache_id`, `actual_encoding`, and encoded payload.

2. **Decode pixels**: Use the appropriate decoder (Tight, ZRLE, etc.) to decode the payload.

3. **Compute hash**: Hash the decoded pixels using `compute_rect_hash()`. Verify it matches the `cache_id` (paranoid check).

4. **Store in ARC**: Insert entry into T1 (recency list). If capacity exceeded, evict using ARC replacement policy (considers T1/T2/B1/B2 state and parameter `p`).

5. **Store to disk**: Append entry to disk store with checksum. Update in-memory index.

6. **Blit to framebuffer**: Draw the rectangle on screen.

### Success Criteria

After reading this section, you should be able to:
- [ ] Identify the 7 modules and their responsibilities
- [ ] Explain how decoders integrate with PersistentCache
- [ ] Describe the negotiation order and why it matters
- [ ] Trace the data flow for cache hit vs miss
- [ ] Understand where ARC promotion and eviction occur

---

## 3. Protocol Specification

### Constants and Capability Negotiation

#### **Pseudo-Encodings**

```rust
// rfb-encodings/src/lib.rs and rfb-protocol/src/messages/types.rs

// ContentCache (existing)
pub const PSEUDO_ENCODING_CONTENT_CACHE: i32 = -320;

// PersistentCache (new)
pub const PSEUDO_ENCODING_PERSISTENT_CACHE: i32 = -321;
```

#### **Rectangle Encodings**

```rust
// PersistentCache-specific encodings
pub const ENCODING_PERSISTENT_CACHED_RECT: i32 = 102;       // Hash reference
pub const ENCODING_PERSISTENT_CACHED_RECT_INIT: i32 = 103;  // Full data + hash
```

#### **Client-to-Server Message Types**

```rust
// rfb-protocol/src/messages/types.rs

pub const MSG_TYPE_PERSISTENT_CACHE_QUERY: u8 = 254;       // Request missing data
pub const MSG_TYPE_PERSISTENT_CACHE_HASH_LIST: u8 = 253;   // Advertise known hashes (optional)
```

### Message Wire Formats

**All fields use network byte order (big-endian)** per RFB specification.

#### **1. PersistentCachedRect (Server → Client, Encoding 102)**

**Purpose**: Reference cached content by hash without resending pixels.

```
┌─────────────────────────────────────┐
│ Standard RFB Rectangle Header       │
├─────────────────────────────────────┤
│ x: u16                              │
│ y: u16                              │
│ width: u16                          │
│ height: u16                         │
│ encoding: i32 = 102                 │
├─────────────────────────────────────┤
│ Payload (19 bytes)                  │
├─────────────────────────────────────┤
│ hashLen: u8 = 16                    │
│ hashBytes: [u8; 16]                 │
│ flags: u16 = 0 (reserved)           │
└─────────────────────────────────────┘
```

**Total size**: 12 (header) + 19 (payload) = **31 bytes** vs KB-MB of encoded data!

**Client behavior**:
1. Read `hashBytes` (16 bytes).
2. Look up in ARC cache (T1/T2).
3. **HIT**: Blit cached pixels to framebuffer, promote entry.
4. **MISS**: Enqueue `hashBytes` in batch aggregator, return error to trigger refresh.

#### **2. PersistentCachedRectInit (Server → Client, Encoding 103)**

**Purpose**: Send full rectangle data plus hash for caching.

```
┌─────────────────────────────────────┐
│ Standard RFB Rectangle Header       │
├─────────────────────────────────────┤
│ x: u16                              │
│ y: u16                              │
│ width: u16                          │
│ height: u16                         │
│ encoding: i32 = 103                 │
├─────────────────────────────────────┤
│ Payload (25 + N bytes)              │
├─────────────────────────────────────┤
│ hashLen: u8 = 16                    │
│ hashBytes: [u8; 16]                 │
│ innerEncoding: i32                  │
│   (Tight, ZRLE, H.264, etc.)        │
│ payloadLen: u32                     │
│ payloadBytes: [u8; payloadLen]      │
└─────────────────────────────────────┘
```

**Client behavior**:
1. Read `hashBytes`, `innerEncoding`, `payloadLen`.
2. Dispatch to appropriate decoder based on `innerEncoding`.
3. Decode `payloadBytes` to RGBA pixels.
4. Compute hash of decoded pixels, verify it matches `hashBytes` (paranoid check).
5. Store in ARC cache (T1) and disk store.
6. Blit to framebuffer.

#### **3. PersistentCacheQuery (Client → Server, Message Type 254)**

**Purpose**: Request initialization data for missing hashes.

```
┌─────────────────────────────────────┐
│ type: u8 = 254                      │
│ count: u16                          │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: u8 = 16                  │
│   hashBytes: [u8; 16]               │
└─────────────────────────────────────┘
```

**Server behavior**:
- Queue `PersistentCachedRectInit` messages for requested hashes in next framebuffer update.
- May coalesce, rate-limit, or batch responses to avoid network flooding.

### Negotiation Flow

```
Client                                Server
------                                ------
1. Connect, complete handshake

2. Send SetEncodings:
   [Tight, ZRLE, Hextile, RRE,
    CopyRect, Raw,
    -321,  ← PersistentCache (FIRST!)
    -320,  ← ContentCache (fallback)
    -224, -223]                  ───→

3.                                    Server checks capabilities:
                                      - Supports -321? → Use PersistentCache
                                      - Only -320? → Use ContentCache
                                      - Neither? → Standard encodings only

4. Request framebuffer update    ───→

5.                              ←───  FramebufferUpdate:
                                      - CachedRectRef (encoding 102) for hits
                                      - CachedRectInit (encoding 103) for misses
                                      - Standard encodings for new content

6. On cache miss:
   Send PersistentCacheQuery    ───→
   (batch of missing hashes)

7.                              ←───  FramebufferUpdate:
                                      - CachedRectInit for requested hashes
```

### Batching Strategy

**Goal**: Reduce roundtrip overhead by aggregating multiple cache miss requests into a single message.

**Thresholds** (flush batch when any condition met):
- **Count**: 64 cache IDs
- **Bytes**: 2048 bytes (64 × (1 + 16) = 1088, but leave headroom)
- **Time**: 5 milliseconds since first miss in batch
- **Frame boundary**: Server sends `LastRect` pseudo-encoding

**Backpressure**:
- Limit to 4 outstanding batch requests to avoid flooding server.
- If 4 batches are in-flight, wait for at least one response before sending more.

**Deduplication**:
- Use `HashSet<CacheId>` to avoid requesting the same ID multiple times in a batch.

### Size Tracking and ARC Semantics

**CRITICAL**: All capacity limits and accounting use **bytes of pixel payload**, not entry count.

```rust
pub struct ArcCache {
    t1: IndexMap<CacheId, CachedEntry>,  // Recently used once
    t2: IndexMap<CacheId, CachedEntry>,  // Frequently used
    b1: IndexMap<CacheId, usize>,        // Ghost: evicted from T1 (size only)
    b2: IndexMap<CacheId, usize>,        // Ghost: evicted from T2 (size only)
    
    t1_size_bytes: usize,
    t2_size_bytes: usize,
    max_size_bytes: usize,
    p: usize,  // Target T1 size in bytes (adaptive parameter)
}

impl CachedEntry {
    fn size_bytes(&self) -> usize {
        self.pixels.len() + std::mem::size_of::<Self>()
    }
}
```

**Why bytes matter**:
- A 64×64 RGBA tile is ~16 KB.
- A 1920×1080 screenshot is ~8 MB.
- A 2048 MB cache holds ~131,000 tiles OR ~256 screenshots.
- Tracking by entry count would allow 131,000 screenshots (1 TB+)!

### Success Criteria

After reading this section, you should be able to:
- [ ] List all protocol constants (-321, 102, 103, 254, 253)
- [ ] Draw the wire format for PersistentCachedRect and PersistentCachedRectInit
- [ ] Explain the negotiation flow and server capability detection
- [ ] Describe the batching strategy and thresholds
- [ ] Understand why size tracking must use bytes, not count

---

## 4. Implementation Phases

This implementation is divided into 10 phases, each with clear scope, deliverables, and acceptance criteria.

### Phase Overview

| Phase | Name | Duration | Blockers | Key Deliverable |
|-------|------|----------|----------|-----------------|
| **A** | Dependencies & Scaffolding | 0.5-1 day | None | Empty modules compile |
| **B** | SHA-256 Hashing | 0.5 day | A | Hash function with tests |
| **C** | ARC Cache | 1.5-2 days | B | In-memory cache with byte tracking |
| **D** | Disk Persistence | 2-3 days | C | Load/save with checksums |
| **E** | Wire Protocol | 1 day | None | Message serialization |
| **F** | Batch Aggregator | 0.5 day | E | Query batching logic |
| **G** | Viewer Integration | 1.5-2 days | B, C, D, E, F | End-to-end decode path |
| **H** | Error Handling | 0.5 day | G | Graceful degradation |
| **I** | Testing | 1-2 days | All | Cross-session validation |
| **J** | Performance Tuning | 1 day | I | Benchmarks meet targets |

**Total Estimated Duration**: 10-15 developer days

### Gating Criteria

Each phase must meet its acceptance criteria before proceeding:

- **Unit tests pass** for all new code
- **Integration tests pass** (if applicable)
- **Documentation complete** (inline docs + this plan updated)
- **Code review** (if team-based) or self-review checklist
- **No regressions** in existing tests

---

## Phase A: Dependencies and Scaffolding

### Scope

Create the module structure, add dependencies, and establish empty stub implementations that compile.

### Estimated Effort

**0.5 to 1 day**

### Files to Create or Modify

#### **1. Add Dependencies** (`Cargo.toml`)

```toml
[workspace.dependencies]
# Existing dependencies...

# PersistentCache dependencies
sha2 = "0.10"         # SHA-256 hashing
indexmap = "2"        # Ordered HashMap for LRU behavior
byteorder = "1"       # Binary I/O with network byte order
directories = "5"     # Cross-platform XDG directory support
```

Update `rfb-encodings/Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# PersistentCache (feature-gated)
sha2 = { workspace = true, optional = true }
indexmap = { workspace = true, optional = true }
byteorder = { workspace = true, optional = true }
directories = { workspace = true, optional = true }

[features]
persistent-cache = ["dep:sha2", "dep:indexmap", "dep:byteorder", "dep:directories"]
```

#### **2. Create Module Files**

```
rfb-encodings/src/persistent_cache/
├── mod.rs              # 100-150 LOC: Public API, GlobalClientPersistentCache stub
├── hashing.rs          # 50 LOC: Empty hash function stub
├── arc.rs              # 200 LOC: ARC structs and empty impl
├── store.rs            # 150 LOC: DiskStore struct and empty impl
├── wire.rs             # 50 LOC: Protocol constants
├── metrics.rs          # 50 LOC: Metrics struct
└── config.rs           # 30 LOC: Config struct
```

#### **3. Public API** (`mod.rs`)

```rust
//! PersistentCache - Cross-session content-addressable cache.
//!
//! Extends ContentCache with:
//! - SHA-256 content hashing for stable cache keys
//! - Disk persistence for cross-session cache survival
//! - ARC eviction for optimal hit rates

#[cfg(feature = "persistent-cache")]
pub mod hashing;
#[cfg(feature = "persistent-cache")]
pub mod arc;
#[cfg(feature = "persistent-cache")]
pub mod store;
#[cfg(feature = "persistent-cache")]
pub mod wire;
#[cfg(feature = "persistent-cache")]
pub mod metrics;
#[cfg(feature = "persistent-cache")]
pub mod config;

#[cfg(feature = "persistent-cache")]
pub use self::hashing::compute_rect_hash;
#[cfg(feature = "persistent-cache")]
pub use self::arc::ArcCache;
#[cfg(feature = "persistent-cache")]
pub use self::store::DiskStore;
#[cfg(feature = "persistent-cache")]
pub use self::config::Config;

/// 16-byte content hash (truncated SHA-256).
pub type CacheId = [u8; 16];

/// Cached pixel data entry.
#[derive(Debug, Clone)]
pub struct CachedEntry {
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: usize,  // CRITICAL: in pixels, not bytes!
}

impl CachedEntry {
    /// Calculate memory size in bytes (includes struct overhead).
    pub fn size_bytes(&self) -> usize {
        self.pixels.len() + std::mem::size_of::<Self>()
    }
}

/// Global persistent cache (main API).
pub struct GlobalClientPersistentCache {
    // TODO: Implementation in later phases
}

#[cfg(feature = "persistent-cache")]
impl GlobalClientPersistentCache {
    pub fn new(config: Config) -> Result<Self> {
        todo!("Phase C")
    }
    
    pub fn load_from_disk() -> Result<Self> {
        todo!("Phase D")
    }
    
    pub fn get(&self, cache_id: &CacheId) -> Option<&CachedEntry> {
        todo!("Phase C")
    }
    
    pub fn insert(&self, cache_id: CacheId, entry: CachedEntry) -> Result<()> {
        todo!("Phase C")
    }
    
    pub fn save_to_disk(&self) -> Result<()> {
        todo!("Phase D")
    }
}
```

#### **4. Update `lib.rs`**

```rust
// rfb-encodings/src/lib.rs

// Existing modules...

// PersistentCache (feature-gated)
#[cfg(feature = "persistent-cache")]
pub mod persistent_cache;
#[cfg(feature = "persistent-cache")]
pub use persistent_cache::{GlobalClientPersistentCache, CacheId, CachedEntry};
```

### Key Structures

#### **`CacheId`**: 16-byte hash

```rust
pub type CacheId = [u8; 16];
```

#### **`CachedEntry`**: Pixel data with metadata

```rust
pub struct CachedEntry {
    pub pixels: Vec<u8>,          // Decoded RGBA data
    pub format: PixelFormat,      // Pixel format metadata
    pub width: u32,
    pub height: u32,
    pub stride_pixels: usize,     // Row stride in PIXELS (not bytes!)
}
```

#### **`Config`**: Configuration parameters

```rust
pub struct Config {
    pub max_size_mb: usize,       // e.g., 2048 MB
    pub cache_dir: PathBuf,       // e.g., ~/.cache/tigervnc/rust-viewer/
    pub enable_persistence: bool, // false for RAM-only mode
}
```

### Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_compiles_without_feature() {
        // Ensure crate compiles with feature disabled
    }
    
    #[cfg(feature = "persistent-cache")]
    #[test]
    fn test_compiles_with_feature() {
        use crate::persistent_cache::CacheId;
        let _id: CacheId = [0u8; 16];
    }
}
```

### Dependencies and Prerequisites

- **None** (first phase)

### Success Criteria

- [ ] `cargo build` succeeds without `--features persistent-cache`
- [ ] `cargo build --features persistent-cache` succeeds
- [ ] All stub modules compile with `todo!()` macros
- [ ] `cargo test --features persistent-cache` runs (may skip todo tests)
- [ ] Public API types are defined and exported
- [ ] No clippy warnings

---

## Phase B: SHA-256 Hashing and Stride Handling

### Scope

Implement content hashing of pixel buffers with **correct stride handling** (in pixels, not bytes), truncating SHA-256 to 16 bytes.

### Estimated Effort

**0.5 day**

### Files

- `rfb-encodings/src/persistent_cache/hashing.rs`

### Implementation

```rust
//! Content hashing with SHA-256.
//!
//! CRITICAL: Stride is in pixels, not bytes!

use sha2::{Sha256, Digest};
use crate::persistent_cache::CacheId;
use rfb_protocol::messages::types::PixelFormat;

/// Compute 16-byte content hash of rectangle pixel data.
///
/// # Critical Gotcha
///
/// `stride_pixels` is the row stride in **pixels**, not bytes!
/// This must be multiplied by `bytes_per_pixel` when calculating byte offsets.
///
/// # Arguments
///
/// - `pixels`: Raw pixel data (row-major, tightly packed or with stride)
/// - `pixel_format`: Pixel format descriptor (used for bytes_per_pixel)
/// - `width`: Rectangle width in pixels
/// - `height`: Rectangle height in pixels
/// - `stride_pixels`: Row stride in **pixels** (may be > width for alignment)
///
/// # Returns
///
/// 16-byte cache ID (truncated SHA-256 digest)
///
/// # Example
///
/// ```
/// let pf = PixelFormat { bits_per_pixel: 32, ... };
/// let pixels = vec![0xFF; 64 * 64 * 4];  // 64x64 RGBA
/// let hash = compute_rect_hash(&pixels, &pf, 64, 64, 64);
/// assert_eq!(hash.len(), 16);
/// ```
pub fn compute_rect_hash(
    pixels: &[u8],
    pixel_format: &PixelFormat,
    width: usize,
    height: usize,
    stride_pixels: usize,
) -> CacheId {
    let bytes_per_pixel = pixel_format.bytes_per_pixel() as usize;
    
    // CRITICAL: stride_pixels * bytes_per_pixel, not just stride_pixels!
    let stride_bytes = stride_pixels * bytes_per_pixel;
    let row_bytes = width * bytes_per_pixel;
    
    tracing::trace!(
        "Hashing rect: {}x{} stride_pixels={} bytes_per_pixel={} stride_bytes={}",
        width, height, stride_pixels, bytes_per_pixel, stride_bytes
    );
    
    // Hash row-by-row to handle stride correctly
    let mut hasher = Sha256::new();
    for y in 0..height {
        let row_start = y * stride_bytes;
        let row_end = row_start + row_bytes;
        
        if row_end > pixels.len() {
            tracing::error!(
                "Buffer overflow: row_end={} > len={} (y={}, stride_bytes={}, row_bytes={})",
                row_end, pixels.len(), y, stride_bytes, row_bytes
            );
            panic!("Invalid stride or buffer size");
        }
        
        hasher.update(&pixels[row_start..row_end]);
    }
    
    // Finalize and truncate to 16 bytes
    let result = hasher.finalize();
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&result[..16]);
    
    tracing::trace!("Computed hash: {:02x?}", hash);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_test_format() -> PixelFormat {
        PixelFormat {
            bits_per_pixel: 32,
            depth: 24,
            big_endian: 0,
            true_color: 1,
            red_max: 255,
            green_max: 255,
            blue_max: 255,
            red_shift: 16,
            green_shift: 8,
            blue_shift: 0,
        }
    }
    
    #[test]
    fn test_hash_deterministic() {
        let pf = make_test_format();
        let pixels = vec![0xFF; 64 * 64 * 4];
        
        let hash1 = compute_rect_hash(&pixels, &pf, 64, 64, 64);
        let hash2 = compute_rect_hash(&pixels, &pf, 64, 64, 64);
        
        assert_eq!(hash1, hash2, "Hash must be deterministic");
    }
    
    #[test]
    fn test_hash_with_stride() {
        let pf = make_test_format();
        
        // Tightly packed (stride = width)
        let pixels_tight = vec![0xAB; 64 * 64 * 4];
        let hash_tight = compute_rect_hash(&pixels_tight, &pf, 64, 64, 64);
        
        // With stride padding (stride = 80)
        let mut pixels_stride = Vec::new();
        for _ in 0..64 {
            pixels_stride.extend(vec![0xAB; 64 * 4]);  // 64 pixels of data
            pixels_stride.extend(vec![0x00; 16 * 4]);  // 16 pixels of padding
        }
        let hash_stride = compute_rect_hash(&pixels_stride, &pf, 64, 64, 80);
        
        assert_eq!(hash_tight, hash_stride, "Hash should ignore stride padding");
    }
    
    #[test]
    fn test_hash_differs_for_different_content() {
        let pf = make_test_format();
        let pixels1 = vec![0xFF; 64 * 64 * 4];
        let pixels2 = vec![0x00; 64 * 64 * 4];
        
        let hash1 = compute_rect_hash(&pixels1, &pf, 64, 64, 64);
        let hash2 = compute_rect_hash(&pixels2, &pf, 64, 64, 64);
        
        assert_ne!(hash1, hash2, "Different content must produce different hashes");
    }
    
    #[test]
    fn test_hash_truncation() {
        // Verify we're using 16 bytes, not 32
        let pf = make_test_format();
        let pixels = vec![0x42; 32 * 32 * 4];
        let hash = compute_rect_hash(&pixels, &pf, 32, 32, 32);
        
        assert_eq!(hash.len(), 16, "Hash must be 16 bytes");
    }
    
    #[test]
    #[should_panic(expected = "Invalid stride or buffer size")]
    fn test_invalid_stride_panics() {
        let pf = make_test_format();
        let pixels = vec![0xFF; 64 * 64 * 4];
        
        // stride_pixels=128 would require 64 * 128 * 4 bytes, but we only have 64*64*4
        compute_rect_hash(&pixels, &pf, 64, 64, 128);
    }
}
```

### Example Signature (No Generics)

```rust
pub fn compute_rect_hash(
    pixels: &[u8],
    pixel_format: &PixelFormat,
    width: usize,
    height: usize,
    stride_pixels: usize,
) -> CacheId
```

### Tests

1. **Deterministic**: Same input → same hash
2. **Stride handling**: Tightly-packed vs stride-padded data produces same hash
3. **Content sensitivity**: Different pixels → different hash
4. **Truncation**: Result is 16 bytes, not 32
5. **Invalid stride**: Panic with clear message

### Dependencies and Prerequisites

- **Phase A** complete (module scaffolding)

### Success Criteria

- [ ] All 5 unit tests pass
- [ ] Hash output is deterministic across platforms
- [ ] Stride padding is correctly ignored
- [ ] No use of "stride in bytes" anywhere in code
- [ ] Logs show `bytes_per_pixel` calculation

---

## Phase C: ARC Cache with Byte-Sized Capacity

### Scope

Implement **ARC (Adaptive Replacement Cache)** with lists T1 (recency), T2 (frequency), B1 (ghost T1), B2 (ghost T2), and adaptive parameter `p`. All size tracking and eviction decisions use **bytes**, not entry count.

### Estimated Effort

**1.5 to 2 days**

### Files

- `rfb-encodings/src/persistent_cache/arc.rs` (300-400 LOC)

### Data Structures

```rust
use indexmap::IndexMap;
use std::collections::HashMap;
use crate::persistent_cache::{CacheId, CachedEntry};

/// ARC (Adaptive Replacement Cache) with byte-sized capacity.
///
/// Maintains four lists:
/// - **T1**: Recently used once (recency)
/// - **T2**: Frequently used (frequency)
/// - **B1**: Ghost list for evicted T1 entries (no payload, size only)
/// - **B2**: Ghost list for evicted T2 entries (no payload, size only)
///
/// Parameter `p` adapts between favoring recency (T1) vs frequency (T2)
/// based on access patterns.
pub struct ArcCache {
    /// Main storage: cache_id → entry
    cache: HashMap<CacheId, CachedEntry>,
    
    /// T1: Recently used once (most recent at front)
    t1: IndexMap<CacheId, ()>,
    
    /// T2: Frequently used (most recent at front)
    t2: IndexMap<CacheId, ()>,
    
    /// B1: Ghost list for evicted T1 (no payload, just size)
    b1: IndexMap<CacheId, usize>,
    
    /// B2: Ghost list for evicted T2 (no payload, just size)
    b2: IndexMap<CacheId, usize>,
    
    /// Sizes in bytes
    t1_size_bytes: usize,
    t2_size_bytes: usize,
    max_size_bytes: usize,
    
    /// Adaptive parameter: target T1 size in bytes
    p: usize,
    
    /// Statistics
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl ArcCache {
    pub fn new(max_size_bytes: usize) -> Self {
        Self {
            cache: HashMap::new(),
            t1: IndexMap::new(),
            t2: IndexMap::new(),
            b1: IndexMap::new(),
            b2: IndexMap::new(),
            t1_size_bytes: 0,
            t2_size_bytes: 0,
            max_size_bytes,
            p: 0,  // Initially favor T1 (will adapt)
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }
    
    /// Look up entry by cache_id.
    ///
    /// On hit, promotes entry according to ARC policy:
    /// - Hit in T1: Move to T2 (frequency promotion)
    /// - Hit in T2: Move to MRU in T2 (LRU refresh)
    pub fn get(&mut self, cache_id: &CacheId) -> Option<&CachedEntry> {
        if self.cache.contains_key(cache_id) {
            self.hits += 1;
            
            // Promotion logic
            if self.t1.contains_key(cache_id) {
                // Hit in T1: Move to T2 (frequency promotion)
                self.t1.shift_remove(cache_id);
                self.t2.insert(*cache_id, ());
            } else if self.t2.contains_key(cache_id) {
                // Hit in T2: Move to front (LRU refresh)
                self.t2.shift_remove(cache_id);
                self.t2.insert(*cache_id, ());
            }
            
            self.cache.get(cache_id)
        } else {
            self.misses += 1;
            
            // Ghost hit adaptation
            if self.b1.contains_key(cache_id) {
                // Ghost hit in B1: Increase p (favor recency)
                let size = self.b1.shift_remove(cache_id).unwrap();
                self.p = (self.p + size).min(self.max_size_bytes);
            } else if self.b2.contains_key(cache_id) {
                // Ghost hit in B2: Decrease p (favor frequency)
                let size = self.b2.shift_remove(cache_id).unwrap();
                self.p = self.p.saturating_sub(size);
            }
            
            None
        }
    }
    
    /// Insert new entry into cache.
    ///
    /// Entry is added to T1 (recency list). If capacity exceeded, evict
    /// according to ARC replacement policy.
    pub fn insert(&mut self, cache_id: CacheId, entry: CachedEntry) -> Result<Vec<CacheId>> {
        let entry_size = entry.size_bytes();
        let mut evicted = Vec::new();
        
        // Make room if necessary
        while self.t1_size_bytes + self.t2_size_bytes + entry_size > self.max_size_bytes {
            let evicted_id = self.evict_one()?;
            evicted.push(evicted_id);
        }
        
        // Remove if already exists (update case)
        if let Some(old_entry) = self.cache.remove(&cache_id) {
            let old_size = old_entry.size_bytes();
            if self.t1.contains_key(&cache_id) {
                self.t1.shift_remove(&cache_id);
                self.t1_size_bytes -= old_size;
            } else if self.t2.contains_key(&cache_id) {
                self.t2.shift_remove(&cache_id);
                self.t2_size_bytes -= old_size;
            }
        }
        
        // Insert into T1 (recency)
        self.cache.insert(cache_id, entry);
        self.t1.insert(cache_id, ());
        self.t1_size_bytes += entry_size;
        
        Ok(evicted)
    }
    
    /// Evict one entry according to ARC replacement policy.
    fn evict_one(&mut self) -> Result<CacheId> {
        if self.t1.is_empty() && self.t2.is_empty() {
            anyhow::bail!("Cannot evict: cache is empty");
        }
        
        // ARC replacement policy
        let from_t1 = if self.t1_size_bytes > self.p {
            true  // Evict from T1
        } else {
            false  // Evict from T2
        };
        
        let (cache_id, size) = if from_t1 {
            // Evict LRU from T1 → B1
            let (id, _) = self.t1.pop().ok_or_else(|| anyhow::anyhow!("T1 empty"))?;
            let entry = self.cache.remove(&id).unwrap();
            let size = entry.size_bytes();
            
            self.t1_size_bytes -= size;
            self.b1.insert(id, size);  // Add ghost
            
            (id, size)
        } else {
            // Evict LRU from T2 → B2
            let (id, _) = self.t2.pop().ok_or_else(|| anyhow::anyhow!("T2 empty"))?;
            let entry = self.cache.remove(&id).unwrap();
            let size = entry.size_bytes();
            
            self.t2_size_bytes -= size;
            self.b2.insert(id, size);  // Add ghost
            
            (id, size)
        };
        
        self.evictions += 1;
        
        tracing::debug!(
            "ARC evicted: {:02x?} size={}KB from={} (p={}MB, T1={}MB, T2={}MB)",
            &cache_id[..4], size / 1024, if from_t1 { "T1" } else { "T2" },
            self.p / (1024*1024), self.t1_size_bytes / (1024*1024), self.t2_size_bytes / (1024*1024)
        );
        
        Ok(cache_id)
    }
    
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            size_bytes: self.t1_size_bytes + self.t2_size_bytes,
            max_size_bytes: self.max_size_bytes,
            t1_entries: self.t1.len(),
            t2_entries: self.t2.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            p_bytes: self.p,
        }
    }
}
```

### ARC Replacement Algorithm

**On insertion when capacity exceeded**:

1. **Choose list to evict from**:
   - If `T1_size_bytes > p`: Evict from T1
   - Else: Evict from T2

2. **Evict LRU entry**:
   - Remove from tail of chosen list
   - Remove payload from `cache` HashMap
   - Add ghost entry (ID + size) to B1 or B2

3. **Adaptation** (on ghost hit):
   - Hit in B1: Increase `p` (favor recency)
   - Hit in B2: Decrease `p` (favor frequency)

### API

```rust
impl ArcCache {
    pub fn new(max_size_bytes: usize) -> Self;
    pub fn get(&mut self, cache_id: &CacheId) -> Option<&CachedEntry>;
    pub fn insert(&mut self, cache_id: CacheId, entry: CachedEntry) -> Result<Vec<CacheId>>;
    pub fn stats(&self) -> CacheStats;
}
```

### Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_test_entry(id: u8, size_kb: usize) -> (CacheId, CachedEntry) {
        let cache_id = [id; 16];
        let entry = CachedEntry {
            pixels: vec![0xFF; size_kb * 1024],
            format: test_pixel_format(),
            width: 64,
            height: 64,
            stride_pixels: 64,
        };
        (cache_id, entry)
    }
    
    #[test]
    fn test_arc_insert_and_get() {
        let mut cache = ArcCache::new(10 * 1024 * 1024);  // 10 MB
        let (id, entry) = make_test_entry(1, 100);  // 100 KB
        
        cache.insert(id, entry).unwrap();
        
        assert!(cache.get(&id).is_some());
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().t1_entries, 1);
    }
    
    #[test]
    fn test_arc_promotion_t1_to_t2() {
        let mut cache = ArcCache::new(10 * 1024 * 1024);
        let (id, entry) = make_test_entry(1, 100);
        
        cache.insert(id, entry).unwrap();
        assert_eq!(cache.stats().t1_entries, 1);
        assert_eq!(cache.stats().t2_entries, 0);
        
        // Second access: T1 → T2 (frequency promotion)
        cache.get(&id);
        assert_eq!(cache.stats().t1_entries, 0);
        assert_eq!(cache.stats().t2_entries, 1);
    }
    
    #[test]
    fn test_arc_eviction_by_bytes() {
        let mut cache = ArcCache::new(500 * 1024);  // 500 KB limit
        
        // Insert 5 × 100KB entries (should trigger evictions)
        for i in 1..=5 {
            let (id, entry) = make_test_entry(i, 100);
            cache.insert(id, entry).unwrap();
        }
        
        let stats = cache.stats();
        assert!(stats.entries < 5, "Should have evicted some entries");
        assert!(stats.evictions > 0);
        assert!(stats.size_bytes <= 500 * 1024);
    }
    
    #[test]
    fn test_arc_adaptation() {
        let mut cache = ArcCache::new(1024 * 1024);  // 1 MB
        
        // Fill T1 and T2
        for i in 1..=5 {
            let (id, entry) = make_test_entry(i, 50);
            cache.insert(id, entry).unwrap();
            cache.get(&[i; 16]);  // Access twice → T2
        }
        
        let initial_p = cache.p;
        
        // Evict and create ghost in B1
        let (id, entry) = make_test_entry(99, 500);  // Force eviction
        cache.insert(id, entry).unwrap();
        
        // Ghost hit in B1: should increase p
        // (Would need to track which entry was evicted to B1)
        // This is a simplified test; real test would verify adaptation
    }
}
```

### Dependencies and Prerequisites

- **Phase B** complete (hashing for test vectors)

### Success Criteria

- [ ] All unit tests pass
- [ ] T1 → T2 promotion works correctly
- [ ] Eviction respects byte capacity (not entry count)
- [ ] Parameter `p` adapts based on ghost hits
- [ ] Hit rate improves vs LRU on mixed workloads (microbenchmark)
- [ ] No panics or crashes under stress (fuzzing recommended)

---

## Phase D: Disk Persistence Store and I/O

### Scope

Design and implement on-disk cache file format with checksums, append-only writes, corruption recovery, and optional compaction.

### Estimated Effort

**2 to 3 days**

### Files

- `rfb-encodings/src/persistent_cache/store.rs` (400-600 LOC)

### File Location

```rust
use directories::BaseDirs;

fn cache_file_path(server_host: &str, server_port: u16, pf_signature: u64) -> Result<PathBuf> {
    let cache_dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else if let Some(base) = BaseDirs::new() {
        base.cache_dir().to_path_buf()
    } else {
        bail!("Cannot determine cache directory");
    };
    
    let filename = format!(
        "persistentcache-{}-{}-{:016x}.dat",
        server_host, server_port, pf_signature
    );
    
    let path = cache_dir
        .join("tigervnc")
        .join("rust-viewer")
        .join(filename);
    
    Ok(path)
}
```

**Why per-server files?**
- Different servers may have different pixel formats (32-bit vs 16-bit, different endianness).
- Hashes are pixel-format-specific, so mixing formats in one cache would cause mismatches.

### File Format

```
┌────────────────────────────────────────┐
│ Header (64 bytes)                      │
├────────────────────────────────────────┤
│ magic: [u8; 4] = b"PCAC"              │
│ version: u32 = 1                       │
│ endian_marker: u8 = 1 (big)           │
│ reserved: [u8; 27]                     │
│ header_checksum: [u8; 32] (SHA-256)   │
└────────────────────────────────────────┘
│ Record 1                               │
│ Record 2                               │
│ ...                                    │
│ Record N                               │
└────────────────────────────────────────┘

Record Format:
┌────────────────────────────────────────┐
│ record_length: u32                     │
│ cache_id: [u8; 16]                     │
│ width: u16                             │
│ height: u16                            │
│ stride_pixels: u32                     │
│ bytes_per_pixel: u8                    │
│ pixel_format_sig: u64                  │
│ payload_length: u32                    │
│ payload: [u8; payload_length]          │
│ record_checksum: [u8; 32] (SHA-256)   │
└────────────────────────────────────────┘
```

### Implementation

```rust
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write, Seek, SeekFrom};
use std::path::PathBuf;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use sha2::{Sha256, Digest};

const MAGIC: &[u8; 4] = b"PCAC";
const VERSION: u32 = 1;
const HEADER_SIZE: usize = 64;

pub struct DiskStore {
    path: PathBuf,
    file: Option<BufWriter<File>>,
    index: HashMap<CacheId, FileOffset>,
    total_size_bytes: usize,
}

struct FileOffset {
    offset: u64,
    size: usize,
}

impl DiskStore {
    pub fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            path,
            file: None,
            index: HashMap::new(),
            total_size_bytes: 0,
        })
    }
    
    /// Open cache file and load index into memory.
    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            tracing::info!("Cache file does not exist, starting fresh: {:?}", self.path);
            return Ok(());
        }
        
        tracing::info!("Loading cache from disk: {:?}", self.path);
        
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        
        // Read and validate header
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            anyhow::bail!("Invalid magic: expected {:?}, got {:?}", MAGIC, magic);
        }
        
        let version = reader.read_u32::<BigEndian>()?;
        if version != VERSION {
            anyhow::bail!("Unsupported version: {}", version);
        }
        
        // Skip reserved bytes
        let mut reserved = [0u8; 27];
        reader.read_exact(&mut reserved)?;
        
        // Read header checksum
        let mut header_checksum = [0u8; 32];
        reader.read_exact(&mut header_checksum)?;
        // TODO: Verify header checksum
        
        // Read records and build index
        let mut offset = HEADER_SIZE as u64;
        let mut record_count = 0;
        
        loop {
            // Try to read record_length
            let record_length = match reader.read_u32::<BigEndian>() {
                Ok(len) => len,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            };
            
            if record_length == 0 {
                break;  // End of file
            }
            
            // Read cache_id
            let mut cache_id = [0u8; 16];
            reader.read_exact(&mut cache_id)?;
            
            // Read metadata
            let width = reader.read_u16::<BigEndian>()?;
            let height = reader.read_u16::<BigEndian>()?;
            let stride_pixels = reader.read_u32::<BigEndian>()?;
            let bytes_per_pixel = reader.read_u8()?;
            let pf_sig = reader.read_u64::<BigEndian>()?;
            let payload_length = reader.read_u32::<BigEndian>()?;
            
            // Skip payload and checksum (we'll read on-demand)
            let skip_bytes = payload_length as u64 + 32;
            reader.seek(SeekFrom::Current(skip_bytes as i64))?;
            
            // Add to index
            self.index.insert(cache_id, FileOffset {
                offset,
                size: payload_length as usize,
            });
            self.total_size_bytes += payload_length as usize;
            
            offset += record_length as u64;
            record_count += 1;
        }
        
        tracing::info!(
            "Loaded {} entries ({} MB) from disk cache",
            record_count,
            self.total_size_bytes / (1024 * 1024)
        );
        
        Ok(())
    }
    
    /// Append new entry to cache file.
    pub fn append(&mut self, cache_id: CacheId, entry: &CachedEntry) -> Result<()> {
        // Open file for append (lazy)
        if self.file.is_none() {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.file = Some(BufWriter::new(file));
        }
        
        let writer = self.file.as_mut().unwrap();
        
        // Calculate sizes
        let payload_length = entry.pixels.len() as u32;
        let record_length = 4 + 16 + 2 + 2 + 4 + 1 + 8 + 4 + payload_length + 32;
        
        // Write record
        writer.write_u32::<BigEndian>(record_length)?;
        writer.write_all(&cache_id)?;
        writer.write_u16::<BigEndian>(entry.width as u16)?;
        writer.write_u16::<BigEndian>(entry.height as u16)?;
        writer.write_u32::<BigEndian>(entry.stride_pixels as u32)?;
        writer.write_u8(entry.format.bytes_per_pixel())?;
        let pf_sig = compute_pixel_format_signature(&entry.format);
        writer.write_u64::<BigEndian>(pf_sig)?;
        writer.write_u32::<BigEndian>(payload_length)?;
        writer.write_all(&entry.pixels)?;
        
        // Compute and write record checksum
        let mut hasher = Sha256::new();
        hasher.update(&cache_id);
        hasher.update(&entry.pixels);
        let checksum = hasher.finalize();
        writer.write_all(&checksum)?;
        
        writer.flush()?;
        
        // Update index
        let offset = self.index.len() as u64 * 1024;  // Approximate
        self.index.insert(cache_id, FileOffset {
            offset,
            size: entry.pixels.len(),
        });
        self.total_size_bytes += entry.pixels.len();
        
        Ok(())
    }
    
    /// Read entry from disk by cache_id.
    pub fn read(&self, cache_id: &CacheId) -> Result<Option<CachedEntry>> {
        let offset_info = match self.index.get(cache_id) {
            Some(info) => info,
            None => return Ok(None),
        };
        
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        
        // Seek to record
        reader.seek(SeekFrom::Start(offset_info.offset))?;
        
        // Read record (similar to load() logic)
        let record_length = reader.read_u32::<BigEndian>()?;
        let mut read_cache_id = [0u8; 16];
        reader.read_exact(&mut read_cache_id)?;
        
        if &read_cache_id != cache_id {
            anyhow::bail!("Cache ID mismatch at offset {}", offset_info.offset);
        }
        
        // Read metadata
        let width = reader.read_u16::<BigEndian>()? as u32;
        let height = reader.read_u16::<BigEndian>()? as u32;
        let stride_pixels = reader.read_u32::<BigEndian>()? as usize;
        let bytes_per_pixel = reader.read_u8()?;
        let pf_sig = reader.read_u64::<BigEndian>()?;
        let payload_length = reader.read_u32::<BigEndian>()?;
        
        // Read payload
        let mut pixels = vec![0u8; payload_length as usize];
        reader.read_exact(&mut pixels)?;
        
        // Read and verify checksum
        let mut checksum = [0u8; 32];
        reader.read_exact(&mut checksum)?;
        
        let mut hasher = Sha256::new();
        hasher.update(cache_id);
        hasher.update(&pixels);
        let computed = hasher.finalize();
        
        if &checksum != computed.as_slice() {
            anyhow::bail!("Checksum mismatch for cache_id {:?}", cache_id);
        }
        
        // Reconstruct PixelFormat (would need to store more fields)
        let format = reconstruct_pixel_format(bytes_per_pixel, pf_sig)?;
        
        Ok(Some(CachedEntry {
            pixels,
            format,
            width,
            height,
            stride_pixels,
        }))
    }
    
    pub fn size_bytes(&self) -> usize {
        self.total_size_bytes
    }
    
    pub fn entry_count(&self) -> usize {
        self.index.len()
    }
}

fn compute_pixel_format_signature(pf: &PixelFormat) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(&[pf.bits_per_pixel]);
    hasher.update(&[pf.depth]);
    hasher.update(&[pf.big_endian]);
    hasher.update(&[pf.true_color]);
    let hash = hasher.finalize();
    u64::from_be_bytes(hash[..8].try_into().unwrap())
}

fn reconstruct_pixel_format(bytes_per_pixel: u8, _signature: u64) -> Result<PixelFormat> {
    // Simplified: assume standard RGB888 format
    // Real implementation would store full PixelFormat fields
    Ok(PixelFormat {
        bits_per_pixel: bytes_per_pixel * 8,
        depth: 24,
        big_endian: 0,
        true_color: 1,
        red_max: 255,
        green_max: 255,
        blue_max: 255,
        red_shift: 16,
        green_shift: 8,
        blue_shift: 0,
    })
}
```

### I/O Behavior

- **Append-only writes**: New entries are appended to the end of the file.
- **Buffered I/O**: Use `BufReader`/`BufWriter` for performance.
- **fsync on close**: Call `file.sync_all()` before dropping the file handle.
- **Corruption recovery**: If a record checksum fails, log error and skip to next record.
- **Compaction** (optional): Background task to rewrite file with only live entries.

### Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_store_round_trip() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        
        let mut store = DiskStore::new(path.clone()).unwrap();
        
        let cache_id = [0x42; 16];
        let entry = make_test_entry();
        
        store.append(cache_id, &entry).unwrap();
        drop(store);
        
        // Reload from disk
        let mut store2 = DiskStore::new(path).unwrap();
        store2.load().unwrap();
        
        let loaded = store2.read(&cache_id).unwrap().unwrap();
        assert_eq!(loaded.pixels, entry.pixels);
        assert_eq!(loaded.width, entry.width);
    }
    
    #[test]
    fn test_store_corruption_recovery() {
        // TODO: Write valid record, corrupt checksum, verify skip
    }
}
```

### Dependencies and Prerequisites

- **Phase C** complete (ARC cache for testing round-trip)

### Success Criteria

- [ ] Round-trip test passes (write, close, reopen, read)
- [ ] Corruption test: flip bytes, ensure graceful skip
- [ ] Index built correctly on load
- [ ] Checksums verified on read
- [ ] No data loss on clean shutdown
- [ ] File format documented and stable

---

## Phase E: Wire Protocol and Negotiation

### Scope

Implement protocol constants, message encoding/decoding, and capability negotiation that prefers `-321` over `-320`.

### Estimated Effort

**1 day**

### Files

- `rfb-encodings/src/persistent_cache/wire.rs` (150-250 LOC)
- Modify `rfb-client/src/connection.rs` (add `-321` to encodings list)

### Constants

```rust
// rfb-encodings/src/persistent_cache/wire.rs

/// Pseudo-encoding for PersistentCache capability.
pub const PSEUDO_ENCODING_PERSISTENT_CACHE: i32 = -321;

/// Encoding for PersistentCachedRect (hash reference).
pub const ENCODING_PERSISTENT_CACHED_RECT: i32 = 102;

/// Encoding for PersistentCachedRectInit (full data + hash).
pub const ENCODING_PERSISTENT_CACHED_RECT_INIT: i32 = 103;

/// Message type for client → server cache query.
pub const MSG_TYPE_PERSISTENT_CACHE_QUERY: u8 = 254;

/// Message type for client → server hash list (optional).
pub const MSG_TYPE_PERSISTENT_CACHE_HASH_LIST: u8 = 253;
```

### Message Encoding/Decoding

```rust
use crate::persistent_cache::CacheId;
use rfb_protocol::io::{RfbInStream, RfbOutStream};

/// PersistentCachedRect - Read hash reference from stream.
pub async fn read_persistent_cached_rect<R: AsyncRead + Unpin>(
    stream: &mut RfbInStream<R>,
) -> Result<CacheId> {
    let hash_len = stream.read_u8().await?;
    if hash_len != 16 {
        anyhow::bail!("Expected hash_len=16, got {}", hash_len);
    }
    
    let mut cache_id = [0u8; 16];
    stream.read_bytes(&mut cache_id).await?;
    
    let flags = stream.read_u16().await?;
    if flags != 0 {
        tracing::warn!("Non-zero flags in PersistentCachedRect: {}", flags);
    }
    
    Ok(cache_id)
}

/// PersistentCachedRectInit - Read hash + inner encoding from stream.
pub async fn read_persistent_cached_rect_init<R: AsyncRead + Unpin>(
    stream: &mut RfbInStream<R>,
) -> Result<(CacheId, i32)> {
    let hash_len = stream.read_u8().await?;
    if hash_len != 16 {
        anyhow::bail!("Expected hash_len=16, got {}", hash_len);
    }
    
    let mut cache_id = [0u8; 16];
    stream.read_bytes(&mut cache_id).await?;
    
    let inner_encoding = stream.read_i32().await?;
    
    Ok((cache_id, inner_encoding))
}

/// PersistentCacheQuery - Write batch request to stream.
pub fn write_persistent_cache_query<W: AsyncWrite + Unpin>(
    stream: &mut RfbOutStream<W>,
    cache_ids: &[CacheId],
) -> Result<()> {
    stream.write_u8(MSG_TYPE_PERSISTENT_CACHE_QUERY);
    stream.write_u16(cache_ids.len() as u16);
    
    for cache_id in cache_ids {
        stream.write_u8(16);  // hash_len
        stream.write_bytes(cache_id);
    }
    
    Ok(())
}
```

### Negotiation Integration

Modify `rfb-client/src/connection.rs`:

```rust
fn build_set_encodings(&self) -> Vec<i32> {
    let mut encodings = vec![
        // Standard encodings (in preference order)
        ENCODING_TIGHT,
        ENCODING_ZRLE,
        ENCODING_HEXTILE,
        ENCODING_RRE,
        ENCODING_COPYRECT,
        ENCODING_RAW,
    ];
    
    // Pseudo-encodings (ORDER MATTERS!)
    if self.config.enable_persistent_cache {
        encodings.push(PSEUDO_ENCODING_PERSISTENT_CACHE);  // -321 FIRST!
    }
    encodings.push(PSEUDO_ENCODING_CONTENT_CACHE);         // -320 fallback
    encodings.push(PSEUDO_ENCODING_LAST_RECT);             // -224
    encodings.push(PSEUDO_ENCODING_DESKTOP_SIZE);          // -223
    
    encodings
}
```

### Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    
    #[tokio::test]
    async fn test_read_persistent_cached_rect() {
        let mut data = Vec::new();
        data.push(16u8);  // hash_len
        data.extend_from_slice(&[0x42; 16]);  // cache_id
        data.extend_from_slice(&[0, 0]);  // flags
        
        let mut stream = RfbInStream::new(Cursor::new(data));
        let cache_id = read_persistent_cached_rect(&mut stream).await.unwrap();
        
        assert_eq!(cache_id, [0x42; 16]);
    }
    
    #[tokio::test]
    async fn test_write_persistent_cache_query() {
        let mut buffer = Vec::new();
        let mut stream = RfbOutStream::new(&mut buffer);
        
        let ids = vec![[0x11; 16], [0x22; 16]];
        write_persistent_cache_query(&mut stream, &ids).unwrap();
        stream.flush().await.unwrap();
        
        // Verify format
        assert_eq!(buffer[0], MSG_TYPE_PERSISTENT_CACHE_QUERY);
        assert_eq!(u16::from_be_bytes([buffer[1], buffer[2]]), 2);
    }
}
```

### Dependencies and Prerequisites

- **None** (can be done in parallel with Phases B-D)

### Success Criteria

- [ ] All message round-trip tests pass
- [ ] Negotiation places `-321` before `-320` in encodings list
- [ ] Big-endian byte order verified
- [ ] Integration test with mock server accepts order

---

## Phase F: Batch Query Aggregator

### Scope

Aggregate unknown cache IDs and flush on thresholds or timeout to reduce roundtrips.

### Estimated Effort

**0.5 day**

### Files

- `rfb-encodings/src/persistent_cache/batch.rs` or extend `wire.rs` (100-150 LOC)

### Implementation

```rust
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use crate::persistent_cache::CacheId;

pub struct BatchAggregator {
    pending: Arc<Mutex<HashSet<CacheId>>>,
    flush_threshold_count: usize,
    flush_threshold_bytes: usize,
    flush_timeout: Duration,
    last_flush: Arc<Mutex<Instant>>,
    sender: mpsc::Sender<Vec<CacheId>>,
}

impl BatchAggregator {
    pub fn new(sender: mpsc::Sender<Vec<CacheId>>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashSet::new())),
            flush_threshold_count: 64,
            flush_threshold_bytes: 2048,
            flush_timeout: Duration::from_millis(5),
            last_flush: Arc::new(Mutex::new(Instant::now())),
            sender,
        }
    }
    
    /// Request a cache ID (non-blocking).
    pub async fn request(&self, cache_id: CacheId) {
        let mut pending = self.pending.lock().await;
        let is_new = pending.insert(cache_id);
        
        if !is_new {
            return;  // Deduplicated
        }
        
        // Check flush conditions
        if self.should_flush(&pending) {
            self.flush_batch(&mut pending).await;
        }
    }
    
    /// Start background flush timer.
    pub fn start_timer(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                sleep(self.flush_timeout).await;
                
                let mut pending = self.pending.lock().await;
                let elapsed = self.last_flush.lock().await.elapsed();
                
                if !pending.is_empty() && elapsed >= self.flush_timeout {
                    self.flush_batch(&mut pending).await;
                }
            }
        });
    }
    
    fn should_flush(&self, pending: &HashSet<CacheId>) -> bool {
        pending.len() >= self.flush_threshold_count ||
        pending.len() * 17 >= self.flush_threshold_bytes  // 1 + 16 per ID
    }
    
    async fn flush_batch(&self, pending: &mut HashSet<CacheId>) {
        if pending.is_empty() {
            return;
        }
        
        let batch: Vec<CacheId> = pending.drain().collect();
        
        tracing::debug!("Flushing batch: {} cache IDs", batch.len());
        
        if let Err(e) = self.sender.send(batch).await {
            tracing::error!("Failed to send batch: {}", e);
        }
        
        *self.last_flush.lock().await = Instant::now();
    }
    
    /// Force flush (e.g., on frame boundary).
    pub async fn flush_now(&self) {
        let mut pending = self.pending.lock().await;
        self.flush_batch(&mut pending).await;
    }
}
```

### Behavior

- **Deduplication**: Use `HashSet` to avoid requesting the same ID twice in a batch.
- **Flush conditions** (any met):
  - Count ≥ 64 IDs
  - Accumulated bytes ≥ 2048
  - Timeout: 5 ms since first miss
  - Frame boundary: Server sends `LastRect` pseudo-encoding
- **Backpressure**: Limit to 4 outstanding batch requests (use semaphore or channel capacity).

### Tests

```rust
#[tokio::test]
async fn test_batch_aggregation() {
    let (tx, mut rx) = mpsc::channel(10);
    let aggregator = Arc::new(BatchAggregator::new(tx));
    
    // Request IDs
    for i in 0..70 {
        aggregator.request([i; 16]).await;
    }
    
    // Should flush at 64
    let batch1 = rx.recv().await.unwrap();
    assert_eq!(batch1.len(), 64);
    
    aggregator.flush_now().await;
    let batch2 = rx.recv().await.unwrap();
    assert_eq!(batch2.len(), 6);
}

#[tokio::test]
async fn test_batch_deduplication() {
    let (tx, mut rx) = mpsc::channel(10);
    let aggregator = Arc::new(BatchAggregator::new(tx));
    
    // Request same ID multiple times
    for _ in 0..10 {
        aggregator.request([0x42; 16]).await;
    }
    
    aggregator.flush_now().await;
    let batch = rx.recv().await.unwrap();
    assert_eq!(batch.len(), 1);  // Deduplicated
}
```

### Dependencies and Prerequisites

- **Phase E** complete (message encoding)

### Success Criteria

- [ ] Batching reduces message count (verified in logs)
- [ ] Deduplication works correctly
- [ ] Timeout flush triggers reliably
- [ ] No added latency visible to user (<10ms)

---

## Phase G: Viewer Integration and Rendering Path

### Scope

Wire PersistentCache into the decode pipeline: handle cache hits (blit from cache), handle cache misses (enqueue batch request), and handle cache init (decode, hash, store, blit).

### Estimated Effort

**1.5 to 2 days**

### Files

- Modify `rfb-encodings/src/cached_rect.rs` (add persistent mode)
- Modify `rfb-encodings/src/cached_rect_init.rs` (add persistent mode)
- Modify `rfb-client/src/connection.rs` (integrate batch aggregator)

### Decoder Integration

#### **CachedRect (Encoding 102) - Cache Hit Path**

```rust
// rfb-encodings/src/cached_rect.rs

impl Decoder for CachedRectDecoder {
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Determine protocol mode
        let cache_id = if self.persistent_mode {
            // PersistentCache: 16-byte hash
            read_persistent_cached_rect(stream).await?
        } else {
            // ContentCache: 8-byte u64
            let id = stream.read_u64().await?;
            // Convert to CacheId (fill with zeros)
            let mut cache_id = [0u8; 16];
            cache_id[..8].copy_from_slice(&id.to_be_bytes());
            cache_id
        };
        
        // Look up in cache
        let cache_hit = {
            let mut cache = self.cache.lock().unwrap();
            cache.get(&cache_id).cloned()
        };
        
        match cache_hit {
            Some(cached_pixels) => {
                // Cache HIT: blit from cache
                let dest_rect = Rect::new(
                    rect.x as i32,
                    rect.y as i32,
                    rect.width as u32,
                    rect.height as u32,
                );
                
                buffer.image_rect(dest_rect, &cached_pixels.pixels, cached_pixels.stride_pixels)?;
                
                tracing::debug!(
                    "PersistentCache HIT: cache_id={:02x?} rect={}x{} at ({},{})",
                    &cache_id[..4], rect.width, rect.height, rect.x, rect.y
                );
                
                Ok(())
            }
            None => {
                // Cache MISS: enqueue batch request
                tracing::warn!(
                    "PersistentCache MISS: cache_id={:02x?} rect={}x{} at ({},{})",
                    &cache_id[..4], rect.width, rect.height, rect.x, rect.y
                );
                
                if self.persistent_mode {
                    self.batch_aggregator.request(cache_id).await;
                }
                
                Err(anyhow!(
                    "Cache miss for cache_id {:02x?}, queued request",
                    &cache_id[..4]
                ))
            }
        }
    }
}
```

#### **CachedRectInit (Encoding 103) - Cache Miss Response**

```rust
// rfb-encodings/src/cached_rect_init.rs

impl Decoder for CachedRectInitDecoder {
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read cache_id and inner encoding
        let (cache_id, inner_encoding) = if self.persistent_mode {
            read_persistent_cached_rect_init(stream).await?
        } else {
            let id = stream.read_u64().await?;
            let enc = stream.read_i32().await?;
            let mut cache_id = [0u8; 16];
            cache_id[..8].copy_from_slice(&id.to_be_bytes());
            (cache_id, enc)
        };
        
        // Decode using inner encoding
        let decoded_pixels = self.decode_inner_encoding(
            stream, rect, inner_encoding, pixel_format
        ).await?;
        
        // Compute hash (paranoid check)
        let computed_hash = compute_rect_hash(
            &decoded_pixels,
            pixel_format,
            rect.width as usize,
            rect.height as usize,
            rect.width as usize,
        );
        
        if self.persistent_mode && computed_hash != cache_id {
            tracing::error!(
                "Hash mismatch! Expected {:02x?}, computed {:02x?}",
                &cache_id[..4], &computed_hash[..4]
            );
            // Continue anyway (server may have different hash algorithm)
        }
        
        // Store in cache
        let entry = CachedEntry {
            pixels: decoded_pixels.clone(),
            format: *pixel_format,
            width: rect.width as u32,
            height: rect.height as u32,
            stride_pixels: rect.width as usize,
        };
        
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(cache_id, entry)?;
        }
        
        // Blit to framebuffer
        let dest_rect = Rect::new(
            rect.x as i32,
            rect.y as i32,
            rect.width as u32,
            rect.height as u32,
        );
        buffer.image_rect(dest_rect, &decoded_pixels, rect.width as usize)?;
        
        tracing::debug!(
            "PersistentCache INIT: cache_id={:02x?} rect={}x{} encoding={}",
            &cache_id[..4], rect.width, rect.height, inner_encoding
        );
        
        Ok(())
    }
}
```

### Key Notes

- **Always compute `bytes_per_pixel`** as `pixel_format.bits_per_pixel / 8`.
- **Read-only buffer pattern**: Use `buffer.get_buffer(rect, &stride)` for read access, `buffer.get_buffer_rw()` for write access.
- **Thread synchronization**: Use `Arc<Mutex<>>` for cache access from decoder threads.

### Tests

```rust
#[tokio::test]
async fn test_persistent_cache_hit() {
    // Populate cache with known entry
    let cache = Arc::new(Mutex::new(GlobalClientPersistentCache::new(...)));
    let cache_id = [0x42; 16];
    let entry = make_test_entry();
    cache.lock().unwrap().insert(cache_id, entry).unwrap();
    
    // Create decoder
    let decoder = CachedRectDecoder::new(cache.clone(), true);
    
    // Simulate stream with PersistentCachedRect
    let mut stream = make_cached_rect_stream(cache_id);
    let rect = Rectangle { x: 0, y: 0, width: 64, height: 64, encoding: 102 };
    let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());
    
    // Decode should succeed (cache hit)
    let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
    assert!(result.is_ok());
    
    // Verify statistics
    let stats = cache.lock().unwrap().stats();
    assert_eq!(stats.hits, 1);
}

#[tokio::test]
async fn test_persistent_cache_miss() {
    // Empty cache
    let cache = Arc::new(Mutex::new(GlobalClientPersistentCache::new(...)));
    let (tx, mut rx) = mpsc::channel(10);
    let aggregator = Arc::new(BatchAggregator::new(tx));
    
    let decoder = CachedRectDecoder::new_with_aggregator(cache.clone(), aggregator.clone(), true);
    
    // Simulate stream with unknown cache_id
    let unknown_id = [0xFF; 16];
    let mut stream = make_cached_rect_stream(unknown_id);
    let rect = Rectangle { x: 0, y: 0, width: 64, height: 64, encoding: 102 };
    let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());
    
    // Decode should fail (cache miss)
    let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
    assert!(result.is_err());
    
    // Verify batch request was queued
    aggregator.flush_now().await;
    let batch = rx.recv().await.unwrap();
    assert!(batch.contains(&unknown_id));
}
```

### Dependencies and Prerequisites

- **Phase B** complete (hashing)
- **Phase C** complete (ARC cache)
- **Phase D** complete (disk store)
- **Phase E** complete (wire protocol)
- **Phase F** complete (batch aggregator)

### Success Criteria

- [ ] Golden test: Known tile referenced multiple times, only first decode
- [ ] Cache hits blit from RAM instantly (no decode)
- [ ] Cache misses enqueue batch requests
- [ ] Cache init stores entry and blits correctly
- [ ] No visual corruption (manual testing)
- [ ] Cross-session test: Restart viewer, immediate cache hits from disk

---

## Phase H: Error Handling and Graceful Degradation

### Scope

Make PersistentCache failures non-fatal. If disk is unavailable or negotiation fails, fall back to ContentCache behavior or RAM-only operation.

### Estimated Effort

**0.5 day**

### Files

- `rfb-encodings/src/persistent_cache/mod.rs`
- `rfb-encodings/src/persistent_cache/store.rs`

### Behavior

#### **1. Disk Unavailable**

```rust
impl GlobalClientPersistentCache {
    pub fn load_from_disk() -> Result<Self> {
        match DiskStore::load() {
            Ok(store) => {
                tracing::info!("Loaded {} entries from disk", store.entry_count());
                Ok(Self::new_with_store(store))
            }
            Err(e) => {
                tracing::warn!("Failed to load disk cache: {}, using RAM-only mode", e);
                Ok(Self::new_ram_only())
            }
        }
    }
    
    pub fn insert(&mut self, cache_id: CacheId, entry: CachedEntry) -> Result<()> {
        // Insert into ARC (always works)
        self.arc.insert(cache_id, entry.clone())?;
        
        // Try to persist (non-fatal)
        if let Some(store) = &mut self.store {
            if let Err(e) = store.append(cache_id, &entry) {
                tracing::error!("Failed to append to disk: {}", e);
                // Continue without persistence
            }
        }
        
        Ok(())
    }
}
```

#### **2. Negotiation Fallback**

```rust
// rfb-client/src/connection.rs

pub fn determine_cache_mode(&self, server_encodings: &[i32]) -> CacheMode {
    if server_encodings.contains(&PSEUDO_ENCODING_PERSISTENT_CACHE) {
        tracing::info!("Server supports PersistentCache (-321)");
        CacheMode::Persistent
    } else if server_encodings.contains(&PSEUDO_ENCODING_CONTENT_CACHE) {
        tracing::info!("Server supports ContentCache (-320), using session-only mode");
        CacheMode::Session
    } else {
        tracing::info!("Server does not support caching");
        CacheMode::None
    }
}
```

#### **3. Hash Mismatch**

```rust
// In CachedRectInit decoder

let computed_hash = compute_rect_hash(...);
if computed_hash != cache_id {
    tracing::error!(
        "Hash mismatch for rect {}x{}: expected {:02x?}, computed {:02x?}",
        rect.width, rect.height, &cache_id[..4], &computed_hash[..4]
    );
    
    // Don't cache (but still display)
    // Evict if it was already in cache
    self.cache.lock().unwrap().remove(&cache_id);
}
```

#### **4. Metrics and Warnings**

```rust
// Log at INFO level on startup/shutdown
tracing::info!("PersistentCache: loaded {} entries ({} MB)", count, size_mb);

// Log at WARN level for non-fatal errors
tracing::warn!("Disk cache unavailable, using RAM-only mode");

// Log at ERROR level for unexpected failures
tracing::error!("Checksum mismatch for cache_id {:?}", cache_id);

// Don't spam logs on every cache miss
if self.miss_count % 100 == 0 {
    tracing::debug!("PersistentCache: {} misses, {} hits", self.miss_count, self.hit_count);
}
```

### Tests

```rust
#[tokio::test]
async fn test_disk_failure_fallback() {
    // Simulate permission denied
    let bad_path = PathBuf::from("/root/cache.dat");
    let cache = GlobalClientPersistentCache::new_with_path(bad_path);
    
    // Should fall back to RAM-only
    assert!(cache.is_ram_only());
    
    // Should still work for insert/get
    let entry = make_test_entry();
    cache.insert([0x42; 16], entry).unwrap();
    assert!(cache.get(&[0x42; 16]).is_some());
}

#[test]
fn test_checksum_mismatch_recovery() {
    // Corrupt cache file
    let mut store = DiskStore::new(...);
    store.load().unwrap();
    
    // Flip a byte in the file
    corrupt_file(&store.path);
    
    // Reload should skip corrupt record
    let mut store2 = DiskStore::new(...);
    store2.load().unwrap();
    
    // Should have one less entry
    assert!(store2.entry_count() < store.entry_count());
}
```

### Dependencies and Prerequisites

- **Phase G** complete (viewer integration)

### Success Criteria

- [ ] Viewer remains functional even with disk failures
- [ ] Graceful fallback to ContentCache if server doesn't support `-321`
- [ ] Hash mismatches logged but don't crash
- [ ] Metrics counters and warnings logged appropriately
- [ ] No panics or unwraps in error paths

---

## Phase I: Testing Strategy and Cross-Session Validation

### Scope

Comprehensive test suite covering unit tests, integration tests, and cross-session validation.

### Estimated Effort

**1 to 2 days**

### Test Categories

#### **1. Unit Tests**

- **Hashing**: Determinism, stride handling, truncation
- **ARC**: T1/T2 promotion, eviction by bytes, parameter `p` adaptation
- **Disk I/O**: Round-trip, corruption recovery, checksum validation
- **Wire protocol**: Message serialization, batch encoding
- **Batch aggregator**: Thresholds, deduplication, timeout

#### **2. Integration Tests**

Connect to test server (`Xnjcvnc :2`) with PersistentCache enabled:

```rust
#[tokio::test]
#[ignore]  // Requires test server
async fn test_persistent_cache_integration() {
    // Connect to test server
    let mut client = VncClient::connect("localhost:2").await.unwrap();
    
    // Verify PersistentCache negotiated
    assert_eq!(client.cache_mode(), CacheMode::Persistent);
    
    // Capture initial state
    let initial_hits = client.cache_stats().hits;
    
    // Request framebuffer updates
    for _ in 0..10 {
        client.request_update(false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    // Verify cache hits increased
    let final_hits = client.cache_stats().hits;
    assert!(final_hits > initial_hits);
}
```

#### **3. Cross-Session Validation**

```rust
#[tokio::test]
#[ignore]  // Requires test server
async fn test_cross_session_persistence() {
    let cache_path = temp_cache_path();
    
    // Session 1: Populate cache
    {
        let mut client = VncClient::connect_with_cache("localhost:2", cache_path.clone())
            .await.unwrap();
        
        for _ in 0..20 {
            client.request_update(false).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        let stats = client.cache_stats();
        tracing::info!("Session 1: {} entries cached", stats.entries);
        
        // Shutdown (triggers disk save)
        client.shutdown().await.unwrap();
    }
    
    // Session 2: Verify immediate hits from disk
    {
        let mut client = VncClient::connect_with_cache("localhost:2", cache_path)
            .await.unwrap();
        
        let stats_before = client.cache_stats();
        assert!(stats_before.entries > 0, "Cache should be loaded from disk");
        
        // Request update (should hit cache immediately)
        client.request_update(false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let stats_after = client.cache_stats();
        assert!(stats_after.hits > stats_before.hits, "Should have cache hits from disk");
        
        // Log hit rate
        let hit_rate = stats_after.hits as f64 / (stats_after.hits + stats_after.misses) as f64;
        tracing::info!("Session 2 hit rate: {:.1}%", hit_rate * 100.0);
        assert!(hit_rate > 0.8, "Expected >80% hit rate on reconnect");
    }
}
```

#### **4. Property Tests (Optional)**

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn arc_invariant_size_within_capacity(
        entries in prop::collection::vec((any::<[u8; 16]>(), any::<Vec<u8>>()), 10..100)
    ) {
        let mut cache = ArcCache::new(1024 * 1024);  // 1 MB
        
        for (id, pixels) in entries {
            let entry = CachedEntry { pixels, ... };
            let _ = cache.insert(id, entry);
        }
        
        let stats = cache.stats();
        prop_assert!(stats.size_bytes <= cache.max_size_bytes);
    }
}
```

### Dependencies and Prerequisites

- **All phases A-H** complete

### Success Criteria

- [ ] All unit tests pass in CI
- [ ] Integration tests pass against test server
- [ ] Cross-session test shows >80% hit rate on reconnect
- [ ] Property tests (if implemented) find no invariant violations
- [ ] No regressions in existing ContentCache tests

---

## Phase J: Performance Targets and Optimization

### Scope

Tune performance to meet targets: hashing throughput, disk I/O, memory overhead, and hit rate.

### Estimated Effort

**1 day**

### Performance Targets

| Metric | Target | Rationale |
|--------|--------|-----------|
| **Hashing throughput** | >2 GB/s | Single-threaded on modern core |
| **Disk append** | >100 MB/s | Sustained writes in release build |
| **Memory overhead** | <15% | Beyond raw pixel payload |
| **Hit rate (reconnect)** | >80% | For previously-visited areas |

### Optimization Tips

#### **1. Hashing Performance**

```rust
// Hash row-by-row to keep cache hot
pub fn compute_rect_hash(
    pixels: &[u8],
    pixel_format: &PixelFormat,
    width: usize,
    height: usize,
    stride_pixels: usize,
) -> CacheId {
    let bytes_per_pixel = pixel_format.bytes_per_pixel() as usize;
    let stride_bytes = stride_pixels * bytes_per_pixel;
    let row_bytes = width * bytes_per_pixel;
    
    let mut hasher = Sha256::new();
    
    // SIMD-friendly row iteration
    for y in 0..height {
        let row_start = y * stride_bytes;
        let row = &pixels[row_start..row_start + row_bytes];
        hasher.update(row);
    }
    
    // ... truncate to 16 bytes
}

// Benchmark
#[bench]
fn bench_hash_1080p(b: &mut Bencher) {
    let pf = PixelFormat::rgb888();
    let pixels = vec![0xFF; 1920 * 1080 * 4];  // 8 MB
    
    b.iter(|| {
        compute_rect_hash(&pixels, &pf, 1920, 1080, 1920)
    });
    
    // Target: <4ms (>2 GB/s)
}
```

#### **2. Disk I/O Performance**

```rust
// Use buffered writer with large buffer
const DISK_BUFFER_SIZE: usize = 256 * 1024;  // 256 KB

impl DiskStore {
    pub fn new(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        
        let writer = BufWriter::with_capacity(DISK_BUFFER_SIZE, file);
        
        // ...
    }
    
    // Batch fsync: don't sync on every write
    pub fn checkpoint(&mut self) -> Result<()> {
        if let Some(writer) = &mut self.file {
            writer.flush()?;
            writer.get_mut().sync_all()?;
        }
        Ok(())
    }
}

// Call checkpoint every 10 MB or 30 seconds
```

#### **3. Memory Overhead**

```rust
// Measure actual memory usage
impl CachedEntry {
    pub fn size_bytes(&self) -> usize {
        self.pixels.len() +  // Payload (largest)
        std::mem::size_of::<Self>() +  // Struct (small)
        // Note: format, width, height, stride are inline
    }
}

// Benchmark memory overhead
#[test]
fn test_memory_overhead() {
    let entry = CachedEntry {
        pixels: vec![0xFF; 64 * 64 * 4],  // 16 KB payload
        format: PixelFormat::rgb888(),
        width: 64,
        height: 64,
        stride_pixels: 64,
    };
    
    let overhead = entry.size_bytes() - entry.pixels.len();
    let overhead_pct = (overhead as f64 / entry.pixels.len() as f64) * 100.0;
    
    println!("Overhead: {} bytes ({:.1}%)", overhead, overhead_pct);
    assert!(overhead_pct < 15.0);
}
```

#### **4. Lock Contention**

```rust
// Minimize lock hold times
pub fn get(&self, cache_id: &CacheId) -> Option<CachedEntry> {
    // Lock, clone, unlock (short critical section)
    let cache = self.arc.lock().unwrap();
    cache.get(cache_id).cloned()
    // Lock released here
}

// Avoid:
pub fn get(&self, cache_id: &CacheId) -> Option<&CachedEntry> {
    let cache = self.arc.lock().unwrap();
    cache.get(cache_id)  // Returns reference, lock held until return value dropped!
}
```

### Benchmarks

```rust
// tests/perf/persistent_cache_bench.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_hash(c: &mut Criterion) {
    let pf = PixelFormat::rgb888();
    let pixels = vec![0xFF; 1920 * 1080 * 4];
    
    c.bench_function("hash_1080p", |b| {
        b.iter(|| {
            compute_rect_hash(
                black_box(&pixels),
                black_box(&pf),
                1920, 1080, 1920
            )
        })
    });
}

fn bench_arc_insert(c: &mut Criterion) {
    let mut cache = ArcCache::new(100 * 1024 * 1024);
    let entries: Vec<_> = (0..1000)
        .map(|i| ([i as u8; 16], make_test_entry(i, 16)))
        .collect();
    
    c.bench_function("arc_insert", |b| {
        b.iter(|| {
            let (id, entry) = &entries[black_box(0)];
            cache.insert(*id, entry.clone()).unwrap();
        })
    });
}

criterion_group!(benches, bench_hash, bench_arc_insert);
criterion_main!(benches);
```

### Dependencies and Prerequisites

- **Phase I** complete (tests stabilized)

### Success Criteria

- [ ] Hashing benchmark: >2 GB/s
- [ ] Disk append benchmark: >100 MB/s
- [ ] Memory overhead: <15%
- [ ] Integration test hit rate: >80%
- [ ] No performance regressions vs ContentCache

---

## Troubleshooting Guide

Common issues and their remedies:

| **Symptom** | **Likely Cause** | **Remedy** |
|-------------|------------------|------------|
| **Visual corruption after hashing** | Stride treated as bytes instead of pixels | Check all hash calculations: `stride_bytes = stride_pixels * bytes_per_pixel` |
| **Many cache misses in persistent mode** | Negotiation failed or server doesn't support `-321` | Verify `SetEncodings` has `-321` before `-320`. Check server logs for accepted capabilities. |
| **Checksum failures on startup** | File truncated or corrupted | Skip bad records and continue. Log error. Schedule compaction to rebuild file. |
| **Disk cache growing too large** | Capacity not enforced or compaction not running | Verify ARC evictions trigger disk prunes. Run compaction periodically. |
| **High latency on first paints** | Batch timeout too long or too many in-flight requests | Lower batch timeout from 5ms to 2ms. Reduce in-flight limit from 4 to 2. |
| **Permission or path errors** | XDG cache directory not writable | Verify `directories` crate returns valid path. Check process permissions. Create directory if missing. |
| **Platform endianness confusion** | Fields not using big-endian | All RFB wire fields use big-endian. All disk format fields use big-endian. Verify with `byteorder` crate. |
| **Hash collisions (very rare)** | Insufficient hash length or buggy hashing | Use full 16 bytes (2^128 space). Verify hash function matches C++ implementation. |
| **ARC not adapting** | Ghost lists not tracked or parameter `p` not updated | Check B1/B2 insertions on eviction. Verify `p` adjustment on ghost hits. |
| **Slow disk loads** | Large files without buffering | Use `BufReader` with 256KB buffer. Consider mmap for read-only loads (advanced). |

---

## Timeline and Milestones

Proposed 15-day schedule with crisp acceptance checks:

| **Day** | **Phase** | **Milestone** | **Acceptance** |
|---------|-----------|---------------|----------------|
| **1** | A | Dependencies & Scaffolding | `cargo build --features persistent-cache` succeeds, modules compile |
| **2** | B | SHA-256 Hashing | All hash tests pass, deterministic output verified |
| **3-4** | C | ARC Cache | ARC unit tests pass, microbenchmark shows hit rate improvement vs LRU |
| **5-7** | D | Disk Persistence | Round-trip test passes, corruption test skips bad records, index built on load |
| **8** | E | Wire Protocol | Message round-trip tests pass, negotiation prefers `-321` |
| **9** | F | Batch Aggregator | Batching thresholds work, deduplication verified, no added latency |
| **10-11** | G | Viewer Integration | Golden tests pass, cache hits blit instantly, visuals correct |
| **12** | H | Error Handling | Graceful degradation tests pass, no panics on disk failures |
| **13-14** | I | Testing | Cross-session test shows >80% hit rate, CI tests green |
| **15** | J | Performance Tuning | Benchmarks meet targets (>2GB/s hash, >100MB/s disk, <15% overhead) |

### Milestone Acceptance

Each phase requires:
- [ ] All unit tests pass
- [ ] Integration tests pass (if applicable)
- [ ] Code review or self-review checklist complete
- [ ] Documentation updated (inline docs + this plan)
- [ ] No regressions in existing tests

---

## Appendix A: C++ Reference Points

When implementing or debugging, consult these C++ files for reference:

### **ContentCache Implementation**

- **`common/rfb/ContentCache.h`** and **`ContentCache.cxx`**
  - ARC algorithm implementation (T1, T2, B1, B2 lists)
  - Cache API: `insert()`, `lookup()`, `evict()`
  - Size tracking and adaptation of parameter `p`

### **Protocol Integration**

- **`common/rfb/EncodeManager.cxx`**
  - Server-side encoding decisions
  - When to send `CachedRect` vs `CachedRectInit`
  - Cache hit/miss logging patterns

- **`common/rfb/DecodeManager.cxx`**
  - Client-side decoding pipeline
  - Handling of cache messages
  - Framebuffer blit operations

### **Protocol Constants**

- **`common/rfb/encodings.h`**
  - Pseudo-encoding values (`-320`, `-321`)
  - Rectangle encoding values (`102`, `103`)
  - Capability negotiation flags

### **Design Documents**

- **`CONTENTCACHE_DESIGN_IMPLEMENTATION.md`**
  - Overall architecture and rationale
  - Known issues and gotchas
  - Performance characteristics

- **`ARC_ALGORITHM.md`**
  - Detailed ARC algorithm explanation
  - T1/T2 promotion rules
  - B1/B2 ghost list behavior
  - Parameter `p` adaptation formulas

### **Critical Bug Reference**

> **Historic Bug: Stride as Bytes (Oct 7, 2025)**
>
> In the C++ implementation, `PixelBuffer::getBuffer()` returns stride in **pixels**.
> A critical bug occurred when stride was treated as bytes directly in hash calculation:
>
> ```cpp
> // WRONG - caused hash collisions and visual corruption
> size_t byteLen = rect.height() * stride;
>
> // CORRECT
> size_t bytesPerPixel = pb->getPF().bpp / 8;
> size_t byteLen = rect.height() * stride * bytesPerPixel;
> ```
>
> **Lesson**: Always verify stride units. When computing byte offsets or lengths,
> multiply stride by `bytes_per_pixel`.

---

## Configuration Example

```rust
// rfb-encodings/src/persistent_cache/config.rs

pub struct Config {
    /// Maximum cache size in megabytes (default: 2048 MB = 2 GB)
    pub max_size_mb: usize,
    
    /// Cache directory (default: ~/.cache/tigervnc/rust-viewer/)
    pub cache_dir: PathBuf,
    
    /// Enable disk persistence (default: true)
    pub enable_persistence: bool,
    
    /// Batch query threshold: count (default: 64)
    pub batch_threshold_count: usize,
    
    /// Batch query threshold: bytes (default: 2048)
    pub batch_threshold_bytes: usize,
    
    /// Batch flush timeout (default: 5 ms)
    pub batch_timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        let cache_dir = BaseDirs::new()
            .map(|b| b.cache_dir().join("tigervnc").join("rust-viewer"))
            .unwrap_or_else(|| PathBuf::from("/tmp/tigervnc-cache"));
        
        Self {
            max_size_mb: 2048,
            cache_dir,
            enable_persistence: true,
            batch_threshold_count: 64,
            batch_threshold_bytes: 2048,
            batch_timeout_ms: 5,
        }
    }
}
```

**CLI integration**:

```rust
// rvncviewer/src/args.rs

#[derive(Parser)]
struct Args {
    // ... existing args
    
    /// PersistentCache size in MB (default: 2048)
    #[arg(long, default_value_t = 2048)]
    cache_size: usize,
    
    /// Disable PersistentCache
    #[arg(long)]
    disable_persistent_cache: bool,
}
```

**Environment variables**:

```bash
# Override cache directory
export TIGERVNC_CACHE_DIR=/mnt/fast-ssd/vnc-cache

# Override cache size
export TIGERVNC_CACHE_SIZE_MB=4096

# Disable persistence (RAM-only)
export TIGERVNC_PERSISTENT_CACHE_DISABLE=1
```

---

## Logging Configuration

Component-level log tags for debugging:

```bash
# Enable all PersistentCache logs
RUST_LOG=rfb_encodings::persistent_cache=debug cargo run

# Specific components
RUST_LOG=rfb_encodings::persistent_cache::hashing=trace  # Hash computation
RUST_LOG=rfb_encodings::persistent_cache::arc=debug       # ARC cache operations
RUST_LOG=rfb_encodings::persistent_cache::store=debug     # Disk I/O
RUST_LOG=rfb_encodings::persistent_cache::wire=debug      # Protocol messages
RUST_LOG=rfb_encodings::persistent_cache::batch=trace     # Query batching

# Combined
RUST_LOG=rfb_encodings::persistent_cache::arc=debug,rfb_encodings::persistent_cache::store=debug
```

**Log output examples**:

```
[DEBUG rfb_encodings::persistent_cache::arc] ARC: T1 hit, promoting to T2: cache_id=a1b2c3d4...
[DEBUG rfb_encodings::persistent_cache::arc] ARC evicted: 12ab34cd... size=16KB from=T1 (p=512MB, T1=1024MB, T2=1024MB)
[DEBUG rfb_encodings::persistent_cache::store] Appended entry: cache_id=5678abcd... size=16KB offset=1234567
[DEBUG rfb_encodings::persistent_cache::batch] Flushing batch: 37 cache IDs
[TRACE rfb_encodings::persistent_cache::hashing] Hashing rect: 64x64 stride_pixels=64 bytes_per_pixel=4 stride_bytes=256
[TRACE rfb_encodings::persistent_cache::hashing] Computed hash: a1b2c3d4e5f6789...
```

---

## Changelog

- **2025-10-24**: Initial draft created
  - All 10 phases defined with detailed scope, effort, files, algorithms, tests, and dependencies
  - Comprehensive troubleshooting guide added
  - Timeline with 15-day schedule and milestone acceptance criteria
  - Configuration and logging sections added
  - Appendix A with C++ reference points

---

**End of Implementation Plan**

For questions or clarifications, refer to:
- [PERSISTENTCACHE_RUST.md](docs/protocol/PERSISTENTCACHE_RUST.md) - Protocol specification
- [C++ Reference: PERSISTENTCACHE_DESIGN.md](../PERSISTENTCACHE_DESIGN.md) - Design document
- [ARC_ALGORITHM.md](../ARC_ALGORITHM.md) - ARC cache algorithm details
