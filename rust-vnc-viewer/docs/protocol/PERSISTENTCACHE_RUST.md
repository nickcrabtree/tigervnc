# PersistentCache Protocol - Rust Implementation Guide

**Date**: 2025-10-24  
**Status**: Design and implementation guidance for Rust VNC viewer  
**C++ Reference**: `/home/nickc/code/tigervnc/PERSISTENTCACHE_DESIGN.md`

## Overview

### Purpose

PersistentCache is an RFB protocol extension that provides **content-addressable caching** using stable cryptographic hashes. Unlike ContentCache (which uses server-assigned cache IDs), PersistentCache enables:

- **Cross-session persistence**: Cache survives client restarts
- **Cross-server compatibility**: Same visual content cached regardless of VNC server
- **Bandwidth reduction**: 97-99% reduction for repeated content (even better than ContentCache)

### Relationship to ContentCache

| Feature | ContentCache | PersistentCache |
|---------|--------------|-----------------|
| Cache key | Server-assigned ID (u64) | Content hash (SHA-256, 16 bytes) |
| Persistence | Session-only | Disk-backed |
| Cross-server | No | Yes |
| Protocol | `-320` pseudo-encoding | `-321` pseudo-encoding |
| Encodings | -512, -511 | 102, 103 |

**Negotiation**: Client advertises both `-321` and `-320` in SetEncodings. Server prefers PersistentCache if available, falls back to ContentCache otherwise.

## Protocol Summary

### Constants

```rust
// rfb-encodings/src/lib.rs
pub const ENCODING_PERSISTENT_CACHED_RECT: i32 = 102;
pub const ENCODING_PERSISTENT_CACHED_RECT_INIT: i32 = 103;
pub const PSEUDO_ENCODING_PERSISTENT_CACHE: i32 = -321;

// rfb-protocol/src/messages/types.rs
pub const MSG_TYPE_PERSISTENT_CACHE_QUERY: u8 = 254;
pub const MSG_TYPE_PERSISTENT_CACHE_HASH_LIST: u8 = 253;
```

### Wire Format

#### PersistentCachedRect (Encoding 102)

Server → Client: Reference cached content by hash

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
│ Payload                             │
├─────────────────────────────────────┤
│ hashLen: u8 (always 16)             │
│ hashBytes: [u8; 16]                 │
│ flags: u16 (reserved, must be 0)    │
└─────────────────────────────────────┘
```

**Client behavior**:
- Lookup `hashBytes` in cache
- On hit: Blit cached pixels to framebuffer
- On miss: Queue `PersistentCacheQuery` for this hash

#### PersistentCachedRectInit (Encoding 103)

Server → Client: Send full data + hash for caching

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
│ Payload                             │
├─────────────────────────────────────┤
│ hashLen: u8 (always 16)             │
│ hashBytes: [u8; 16]                 │
│ innerEncoding: i32                  │
│   (Tight, ZRLE, etc.)               │
│ payloadLen: u32                     │
│ payloadBytes: [u8; payloadLen]      │
└─────────────────────────────────────┘
```

**Client behavior**:
- Decode `payloadBytes` using `innerEncoding` decoder
- Store decoded pixels in cache indexed by `hashBytes`
- Blit to framebuffer

#### PersistentCacheQuery (Message Type 254)

Client → Server: Request missing hashes

```
┌─────────────────────────────────────┐
│ type: u8 = 254                      │
│ count: u16                          │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: u8 (always 16)           │
│   hashBytes: [u8; 16]               │
└─────────────────────────────────────┘
```

**Batching strategy**: Accumulate 5-10 misses before sending query to reduce roundtrips.

#### PersistentHashList (Message Type 253)

Client → Server: Advertise known hashes (optional)

```
┌─────────────────────────────────────┐
│ type: u8 = 253                      │
│ sequenceId: u32                     │
│ totalChunks: u16                    │
│ chunkIndex: u16                     │
│ count: u16                          │
├─────────────────────────────────────┤
│ For each of count:                  │
│   hashLen: u8 (always 16)           │
│   hashBytes: [u8; 16]               │
└─────────────────────────────────────┘
```

**Usage**: Send after initial framebuffer update, chunked in batches of 1000 hashes.

## Rust Module Structure

### Integration Points

```
rfb-protocol/              # Low-level protocol primitives
├── src/
│   ├── content_hash.rs    # SHA-256 hashing utility (NEW)
│   └── messages/
│       ├── client.rs      # Query and HashList writers (MODIFY)
│       └── types.rs       # Message type constants (MODIFY)

rfb-encodings/             # Encoding/decoding implementations
├── src/
│   ├── lib.rs             # Encoding constants (MODIFY)
│   ├── persistent_cache.rs            # GlobalClientPersistentCache (NEW)
│   ├── persistent_cached_rect.rs      # Encoding 102 decoder (NEW)
│   └── persistent_cached_rect_init.rs # Encoding 103 decoder (NEW)

rfb-client/                # High-level client orchestration
├── src/
│   ├── decoder_registry.rs  # Register new decoders (MODIFY)
│   └── connection.rs         # Negotiation, query batching (MODIFY)
```

## Content Hashing

### SHA-256 Implementation

```rust
// rfb-protocol/src/content_hash.rs
use sha2::{Sha256, Digest};

pub fn compute_rect_hash(
    pixels: &[u8],
    width: usize,
    height: usize,
    stride_pixels: usize,
    bytes_per_pixel: usize,
) -> [u8; 16] {
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

### Critical: Stride Handling

**GOTCHA**: Stride is in **pixels**, not bytes!

```rust
// ❌ WRONG - uses stride directly as bytes
let row_bytes = stride;  // BUG!

// ✅ CORRECT - multiply by bytes per pixel
let stride_bytes = stride_pixels * bytes_per_pixel;
```

This was the source of a critical bug in the C++ implementation (Oct 7, 2025) that caused hash collisions and visual corruption.

### Hash Stability

**Requirement**: Hashes must match C++ implementation exactly for cross-compatibility.

Test vector validation:
```rust
#[test]
fn test_hash_matches_cpp() {
    let pixels = vec![0xFF; 64 * 64 * 4];  // RGBA, all white
    let hash = compute_rect_hash(&pixels, 64, 64, 64, 4);
    
    // Expected hash from C++ ContentHash::computeRect
    let expected = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0,
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
    ];
    assert_eq!(hash, expected);
}
```

## GlobalClientPersistentCache

### ARC Eviction Algorithm

**ARC (Adaptive Replacement Cache)** balances recency (T1) and frequency (T2).

#### Data Structures

```rust
use indexmap::IndexMap;  // For LRU ordering
use std::collections::{HashMap, HashSet};

pub struct GlobalClientPersistentCache {
    // Main storage: hash → cached pixel data
    cache: HashMap<[u8; 16], CachedEntry>,
    
    // ARC lists (most recent at front)
    t1: IndexMap<[u8; 16], ()>,  // Recently used once
    t2: IndexMap<[u8; 16], ()>,  // Frequently used
    b1: HashSet<[u8; 16]>,       // Ghost: evicted from T1
    b2: HashSet<[u8; 16]>,       // Ghost: evicted from T2
    
    // Adaptive parameter: target T1 size in bytes
    p: usize,
    
    // Size tracking
    max_size_bytes: usize,
    t1_size_bytes: usize,
    t2_size_bytes: usize,
    
    // Statistics
    hits: u64,
    misses: u64,
    evictions: u64,
}

pub struct CachedEntry {
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: usize,
}
```

#### ARC Algorithm Logic

```rust
impl GlobalClientPersistentCache {
    pub fn get(&mut self, hash: &[u8; 16]) -> Option<&CachedEntry> {
        if let Some(entry) = self.cache.get(hash) {
            self.hits += 1;
            
            // Promotion logic
            if self.t1.contains_key(hash) {
                // Hit in T1: Move to T2 (frequency promotion)
                self.t1.remove(hash);
                self.t2.insert(*hash, ());
            } else if self.t2.contains_key(hash) {
                // Hit in T2: Move to front (LRU refresh)
                self.t2.remove(hash);
                self.t2.insert(*hash, ());
            }
            
            Some(entry)
        } else {
            self.misses += 1;
            
            // Ghost hit logic
            if self.b1.contains(hash) {
                // Ghost hit in B1: Increase p (favor recency)
                self.p = (self.p + self.cache[hash].byte_size()).min(self.max_size_bytes);
            } else if self.b2.contains(hash) {
                // Ghost hit in B2: Decrease p (favor frequency)
                self.p = self.p.saturating_sub(self.cache[hash].byte_size());
            }
            
            None
        }
    }
    
    pub fn insert(&mut self, hash: [u8; 16], entry: CachedEntry) {
        let size = entry.byte_size();
        
        // Make room if needed
        while self.t1_size_bytes + self.t2_size_bytes + size > self.max_size_bytes {
            self.evict_one();
        }
        
        // Insert into T1 (recency list)
        self.cache.insert(hash, entry);
        self.t1.insert(hash, ());
        self.t1_size_bytes += size;
    }
    
    fn evict_one(&mut self) {
        // ARC replacement policy
        if self.t1_size_bytes > self.p {
            // Evict from T1
            if let Some((hash, _)) = self.t1.pop() {
                let size = self.cache[&hash].byte_size();
                self.cache.remove(&hash);
                self.t1_size_bytes -= size;
                self.b1.insert(hash);  // Add ghost
                self.evictions += 1;
            }
        } else {
            // Evict from T2
            if let Some((hash, _)) = self.t2.pop() {
                let size = self.cache[&hash].byte_size();
                self.cache.remove(&hash);
                self.t2_size_bytes -= size;
                self.b2.insert(hash);  // Add ghost
                self.evictions += 1;
            }
        }
    }
}
```

### Size Accounting

**CRITICAL**: Track sizes in **bytes**, not entry count!

```rust
impl CachedEntry {
    pub fn byte_size(&self) -> usize {
        self.pixels.len() + std::mem::size_of::<Self>()
    }
}
```

## File Format and Disk I/O

### Cache File Location

```rust
use directories::BaseDirs;

fn cache_file_path() -> Result<PathBuf> {
    let cache_dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else if let Some(base) = BaseDirs::new() {
        base.cache_dir().to_path_buf()
    } else {
        bail!("Cannot determine cache directory");
    };
    
    Ok(cache_dir.join("tigervnc").join("persistentcache.dat"))
}
```

### File Format

```
┌────────────────────────────────────────┐
│ Header (64 bytes)                      │
├────────────────────────────────────────┤
│ magic: u32 = 0x50435643 ("PCVC")      │
│ version: u32 = 1                       │
│ totalEntries: u64                      │
│ totalBytes: u64                        │
│ created: u64 (Unix timestamp)          │
│ lastAccess: u64 (Unix timestamp)       │
│ _reserved: [u8; 24]                    │
└────────────────────────────────────────┘
│ Entry Records (variable length)        │
│  For each entry:                       │
│    hashLen: u8 (always 16)             │
│    hash: [u8; 16]                      │
│    width: u16                          │
│    height: u16                         │
│    stridePixels: u16                   │
│    pixelFormat: [u8; 24]               │
│    lastAccessTime: u32                 │
│    pixelDataLen: u32                   │
│    pixelData: [u8; pixelDataLen]       │
└────────────────────────────────────────┘
│ Checksum (32 bytes)                    │
│  SHA-256 of all above data             │
└────────────────────────────────────────┘
```

### Load/Save Implementation

```rust
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

impl GlobalClientPersistentCache {
    pub fn load_from_disk() -> Result<Self> {
        let path = cache_file_path()?;
        
        if !path.exists() {
            return Ok(Self::new(DEFAULT_SIZE_MB));
        }
        
        let mut file = File::open(&path)?;
        
        // Read and validate header
        let magic = file.read_u32::<BigEndian>()?;
        if magic != 0x50435643 {
            warn!("Invalid cache magic, starting fresh");
            return Ok(Self::new(DEFAULT_SIZE_MB));
        }
        
        let version = file.read_u32::<BigEndian>()?;
        if version != 1 {
            warn!("Unsupported cache version {}, starting fresh", version);
            return Ok(Self::new(DEFAULT_SIZE_MB));
        }
        
        let total_entries = file.read_u64::<BigEndian>()?;
        // ... read remaining header fields
        
        // Read entries
        let mut cache = Self::new(DEFAULT_SIZE_MB);
        for _ in 0..total_entries {
            let hash_len = file.read_u8()?;
            assert_eq!(hash_len, 16);
            
            let mut hash = [0u8; 16];
            file.read_exact(&mut hash)?;
            
            let width = file.read_u16::<BigEndian>()?;
            let height = file.read_u16::<BigEndian>()?;
            // ... read entry fields
            
            cache.insert(hash, entry);
        }
        
        // TODO: Verify checksum
        
        Ok(cache)
    }
    
    pub fn save_to_disk(&self) -> Result<()> {
        let path = cache_file_path()?;
        
        // Create directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let mut file = File::create(&path)?;
        
        // Write header
        file.write_u32::<BigEndian>(0x50435643)?;  // Magic
        file.write_u32::<BigEndian>(1)?;           // Version
        file.write_u64::<BigEndian>(self.cache.len() as u64)?;
        // ... write remaining header
        
        // Write entries
        for (hash, entry) in &self.cache {
            file.write_u8(16)?;
            file.write_all(hash)?;
            file.write_u16::<BigEndian>(entry.width as u16)?;
            file.write_u16::<BigEndian>(entry.height as u16)?;
            // ... write entry fields
            file.write_all(&entry.pixels)?;
        }
        
        // TODO: Compute and write checksum
        
        Ok(())
    }
}
```

### Corruption Handling

```rust
pub fn load_from_disk() -> Result<Self> {
    match Self::try_load_from_disk() {
        Ok(cache) => Ok(cache),
        Err(e) => {
            warn!("Cache load failed: {}, starting fresh", e);
            
            // Preserve corrupt file as .bak
            let path = cache_file_path()?;
            if path.exists() {
                let bak_path = path.with_extension("dat.bak");
                let _ = std::fs::rename(&path, &bak_path);
            }
            
            Ok(Self::new(DEFAULT_SIZE_MB))
        }
    }
}
```

## Client Protocol Handling

### Decoder Implementation

See implementation plan in `IMPLEMENTATION_PLAN.md` for full decoder pseudocode.

**Key points**:
- Encoding 102: Read hash, lookup cache, blit or queue query
- Encoding 103: Read hash + inner encoding, decode, cache, blit
- Query batching: Accumulate 5-10 misses before flushing

### Negotiation Order

```rust
fn build_set_encodings(&self) -> Vec<i32> {
    vec![
        // Standard encodings
        ENCODING_TIGHT,
        ENCODING_ZRLE,
        ENCODING_HEXTILE,
        // ... others
        
        // Pseudo-encodings (order matters!)
        PSEUDO_ENCODING_PERSISTENT_CACHE,  // -321 (prefer this)
        PSEUDO_ENCODING_CONTENT_CACHE,     // -320 (fallback)
        PSEUDO_ENCODING_LAST_RECT,
        PSEUDO_ENCODING_DESKTOP_SIZE,
    ]
}
```

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_arc_promotion() {
    let mut cache = GlobalClientPersistentCache::new(10);
    let hash1 = [0x01; 16];
    let entry = create_test_entry(64, 64);
    
    cache.insert(hash1, entry);
    assert!(cache.t1.contains_key(&hash1));
    
    // Second access should promote to T2
    cache.get(&hash1);
    assert!(!cache.t1.contains_key(&hash1));
    assert!(cache.t2.contains_key(&hash1));
}

#[test]
fn test_stride_bytes_calculation() {
    let stride_pixels = 80;
    let bytes_per_pixel = 4;
    let stride_bytes = stride_pixels * bytes_per_pixel;
    assert_eq!(stride_bytes, 320);  // Not 80!
}
```

### Integration Tests

Mock server sequences testing cache hit/miss flows.

### Cross-Session Tests

```rust
#[tokio::test]
async fn test_cross_session() {
    let cache_file = temp_cache_file();
    
    // Session 1
    {
        let mut client = connect_with_cache(&cache_file).await?;
        // Populate cache
        client.receive_updates(100).await?;
        client.shutdown().await?;  // Triggers save
    }
    
    // Session 2
    {
        let mut client = connect_with_cache(&cache_file).await?;
        let stats = client.cache_stats();
        assert!(stats.entries > 0);  // Loaded from disk
        // Verify immediate hits
    }
}
```

### WARP Safety

**CRITICAL**: When testing with TigerVNC server:
- ✅ **SAFE**: Use `Xnjcvnc :2` (test server at display :2)
- ❌ **FORBIDDEN**: Do NOT touch `Xtigervnc :1` or `:3` (production servers)

## Performance Considerations

### Optimization Tips

1. **Pre-allocate buffers**: Reuse pixel buffers where possible
2. **Streaming hashing**: Use `Digest::update` incrementally
3. **Avoid copying**: Use references and slices
4. **Batch queries**: Reduce roundtrips with batching

### Performance Targets

| Operation | Target | Typical |
|-----------|--------|---------|
| Hash computation (800×600) | <1ms | ~0.5ms |
| Cache lookup | <0.1ms | ~0.05ms |
| Disk load (10K entries) | <200ms | ~150ms |
| Disk save (10K entries) | <200ms | ~180ms |

## Troubleshooting and Logging

### Logging Categories

```rust
// Enable verbose logging
RUST_LOG=rfb_encodings::persistent_cache=debug

// Key log points
debug!("PersistentCache: lookup hash={:02x?} result={}", hash, hit);
debug!("PersistentCache: insert hash={:02x?} size={}KB", hash, size/1024);
debug!("PersistentCache: evicted {} entries, freed {}KB", count, freed/1024);
debug!("PersistentCache: query batch size={}", batch.len());
debug!("PersistentCache: stats hits={} misses={} rate={:.1}%", hits, misses, hit_rate);
```

### Common Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| Hash mismatch with C++ | Stride bug | Multiply stride by bytes_per_pixel |
| Frequent evictions | Cache too small | Increase max_size_mb |
| Corruption on load | Checksum failure | Implement checksum verification |
| No cross-session hits | Save not triggered | Call save_to_disk on shutdown |

## References

### C++ Implementation

- Design doc: `/home/nickc/code/tigervnc/PERSISTENTCACHE_DESIGN.md`
- ContentHash: `/home/nickc/code/tigervnc/common/rfb/ContentHash.h`
- GlobalClientPersistentCache: `/home/nickc/code/tigervnc/common/rfb/GlobalClientPersistentCache.{h,cxx}`
- ARC algorithm: `/home/nickc/code/tigervnc/ARC_ALGORITHM.md`

### RFB Protocol

- RFC 6143: The Remote Framebuffer Protocol
- TigerVNC extensions: https://tigervnc.org/doc/protocol-extensions.txt

### Dependencies

- `sha2`: Pure Rust SHA-256 implementation
- `indexmap`: Ordered HashMap for LRU behavior
- `byteorder`: Binary I/O with network byte order
- `directories`: Cross-platform XDG directory support

---

**Last Updated**: 2025-10-24  
**Implementation Status**: Planning phase, ready for development  
**Next Steps**: Begin with Task PC-1 (Protocol constants)
