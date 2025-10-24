# Rust VNC Viewer - Current Status and Implementation Plan

**Last Updated**: 2025-10-23  
**Status**: Phases 1–5 complete (client + display); GUI viewer functional; planning Phase 6–8

---

## Executive Summary

A functional Rust VNC viewer (`njcvncviewer-rs`) has been implemented with:
- ✅ Complete RFB protocol handshake (version/security negotiation)
- ✅ Working GUI (egui-based) with framebuffer display
- ✅ Basic encoding support (Raw, CopyRect)
- ✅ Mouse and keyboard input forwarding
- ✅ Modular architecture with 6 crates
- 🔄 **Next**: ContentCache protocol integration for 97-99% bandwidth reduction

---

## Current Implementation Status

### Workspace Architecture

```
rust-vnc-viewer/
├── rfb-common          ✅ Complete (geometry, config)
├── rfb-pixelbuffer     ✅ Complete (pixel formats, buffer management)
├── rfb-protocol        ✅ Complete (networking, I/O, messages, handshake)
├── rfb-encodings       ✅ Complete (Raw, CopyRect, RRE, Hextile, Tight, ZRLE)
├── rfb-client          ✅ Complete (async client, event loop)
├── rfb-display         ✅ Complete (rendering, scaling, viewport, cursor)
├── platform-input      ⚠️  Stub (Phase 6)
└── njcvncviewer-rs     ✅ Working viewer application
```

### Statistics

| Metric | Value |
|--------|-------|
| **Total LOC** | ~13,000+ |
| **Crates Complete** | 6 of 8 (Phases 1–5) |
| **Tests Passing** | 320+ (68 in rfb-display) |
| **Build Status** | ✅ Clean builds |
| **Functional Status** | ✅ Connects, displays, renders smoothly |

---

## Viewer Application (`njcvncviewer-rs`)

### Features Implemented ✅

**Connection Management**:
- Async tokio-based network handling
- RFB 3.3/3.8 protocol version negotiation
- Security negotiation (None type supported)
- Connection state machine with proper transitions
- Event-driven architecture using crossbeam channels

**GUI (egui/eframe)**:
- Window with menu bar (File, View)
- Status bar showing connection state and server info
- Framebuffer rendering with texture caching
- Zoom in/out functionality (25% to 400%)
- Scrollable viewport for oversized framebuffers

**Input Handling**:
- Mouse pointer events with button tracking
- Keyboard events with X11 keysym mapping
- Continuous framebuffer update requests

**Encoding Support**:
- Raw encoding decoder (uncompressed pixel data)
- CopyRect encoding decoder (efficient window/scroll operations)
- Decoder registry pattern (enum-based dispatch)

### Architecture

```
┌─────────────────────────────────────────────────────┐
│                  VncViewerApp                       │
│                  (egui::App)                        │
│  ┌───────────────────────────────────────────────┐  │
│  │ GUI Thread                                    │  │
│  │ - Render framebuffer                          │  │
│  │ - Handle user input                           │  │
│  │ - Send commands via channel                   │  │
│  └───────────────────────────────────────────────┘  │
│           │                          ▲               │
│           │ ConnectionCommand        │               │
│           ▼                          │ ConnectionEvent
│  ┌───────────────────────────────────────────────┐  │
│  │ Connection Thread (tokio runtime)            │  │
│  │ ┌─────────────────────────────────────────┐  │  │
│  │ │ ConnectionManager                       │  │  │
│  │ │ - Handshake                             │  │  │
│  │ │ - Message loop                          │  │  │
│  │ │ - Decoder dispatch                      │  │  │
│  │ │ - Pixel buffer management               │  │  │
│  │ └─────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

### Key Files

```
njcvncviewer-rs/src/
├── main.rs          - CLI parsing, app initialization
├── app.rs           - GUI rendering, input handling
└── connection.rs    - Network protocol, decoder dispatch
```

---

## ContentCache Protocol - Implementation Plan (Phase 8)

### Overview

The ContentCache system provides 97-99% bandwidth reduction for repeated content by:
1. Hashing pixel content to create unique identifiers
2. Maintaining synchronized caches on server and client
3. Sending 20-byte references instead of re-encoding content
4. Using ARC (Adaptive Replacement Cache) for intelligent eviction

### Protocol Messages

**Encoding Types** (in `rfb-protocol/src/messages/types.rs`):
```rust
pub const ENCODING_CACHED_RECT: i32 = 0xFFFFFE00;      // -512
pub const ENCODING_CACHED_RECT_INIT: i32 = 0xFFFFFE01; // -511

// Capability negotiation
pub const PSEUDO_ENCODING_CONTENT_CACHE: i32 = 0xFFFFFE10; // -496
```

**Message Structures**:

```rust
// 1. CachedRect - Reference to cached content (20 bytes total)
pub struct CachedRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: i32,  // ENCODING_CACHED_RECT
    pub cache_id: u64,  // Unique content identifier
}

// 2. CachedRectInit - Initial transmission with cache ID (20+ bytes)
pub struct CachedRectInit {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub encoding: i32,       // ENCODING_CACHED_RECT_INIT
    pub cache_id: u64,       // ID to store under
    pub actual_encoding: i32, // Real encoding (Tight, ZRLE, etc.)
    // Followed by encoded pixel data
}
```

---

## Implementation Tasks

### Phase 1: Protocol Support (Week 1)

**Task 1.1: Message Types** (2 hours)
- [ ] Add encoding constants to `rfb-protocol/src/messages/types.rs`
- [ ] Define `CachedRect` and `CachedRectInit` structs
- [ ] Implement `read_from()` and `write_to()` methods
- [ ] Add to `ServerMessage` enum
- [ ] Write unit tests (10+ tests)

**File**: `rfb-protocol/src/messages/cache.rs` (~300 LOC)

```rust
// rfb-protocol/src/messages/cache.rs
use crate::io::{RfbInStream, RfbOutStream};
use anyhow::{Context, Result};

pub struct CachedRect {
    pub cache_id: u64,
}

impl CachedRect {
    pub async fn read_from<R: tokio::io::AsyncRead + Unpin>(
        stream: &mut RfbInStream<R>,
    ) -> Result<Self> {
        let cache_id = stream.read_u64().await
            .context("Failed to read cache_id")?;
        Ok(Self { cache_id })
    }

    pub fn write_to<W: std::io::Write>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> Result<()> {
        stream.write_u64(self.cache_id)?;
        Ok(())
    }
}

// Similar for CachedRectInit...
```

**Task 1.2: Capability Negotiation** (1 hour)
- [ ] Add `PSEUDO_ENCODING_CONTENT_CACHE` to client encoding list
- [ ] Track capability in connection state
- [ ] Server-side check (future: when Rust server exists)

**File**: Update `njcvncviewer-rs/src/connection.rs` (~20 LOC change)

```rust
// In ConnectionManager::run_inner(), add to encodings:
let set_encodings = ClientMessage::SetEncodings(messages::SetEncodings {
    encodings: vec![
        ENCODING_RAW,
        ENCODING_COPY_RECT,
        PSEUDO_ENCODING_CONTENT_CACHE, // <-- Add this
    ],
});
```

---

### Phase 2: Client-Side Cache (Week 2)

**Task 2.1: ContentCache Data Structure** (4 hours)
- [ ] Create `rfb-encodings/src/content_cache.rs`
- [ ] Implement cache storage (HashMap<u64, CachedPixels>)
- [ ] Add insert/lookup/evict methods
- [ ] Implement LRU eviction policy (simple version)
- [ ] Add memory limits and statistics
- [ ] Write comprehensive unit tests

**File**: `rfb-encodings/src/content_cache.rs` (~500 LOC)

```rust
// rfb-encodings/src/content_cache.rs
use rfb_common::Rect;
use rfb_pixelbuffer::PixelFormat;
use std::collections::HashMap;

pub struct CachedPixels {
    pub cache_id: u64,
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub last_used: std::time::Instant,
}

pub struct ContentCache {
    pixels: HashMap<u64, CachedPixels>,
    max_size_mb: usize,
    current_size_bytes: usize,
    hit_count: u64,
    miss_count: u64,
}

impl ContentCache {
    pub fn new(max_size_mb: usize) -> Self {
        Self {
            pixels: HashMap::new(),
            max_size_mb,
            current_size_bytes: 0,
            hit_count: 0,
            miss_count: 0,
        }
    }

    pub fn insert(&mut self, cache_id: u64, pixels: CachedPixels) -> anyhow::Result<()> {
        // Evict if necessary to make room
        while self.current_size_bytes + pixels.pixels.len() 
              > self.max_size_mb * 1024 * 1024 {
            self.evict_lru()?;
        }

        self.current_size_bytes += pixels.pixels.len();
        self.pixels.insert(cache_id, pixels);
        Ok(())
    }

    pub fn lookup(&mut self, cache_id: u64) -> Option<&CachedPixels> {
        if let Some(cached) = self.pixels.get_mut(&cache_id) {
            cached.last_used = std::time::Instant::now();
            self.hit_count += 1;
            Some(cached)
        } else {
            self.miss_count += 1;
            None
        }
    }

    fn evict_lru(&mut self) -> anyhow::Result<()> {
        // Find oldest entry and remove it
        // ... implementation
        Ok(())
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.pixels.len(),
            size_mb: self.current_size_bytes / (1024 * 1024),
            hit_rate: self.hit_count as f64 / 
                     (self.hit_count + self.miss_count) as f64,
        }
    }
}
```

**Task 2.2: CachedRect Decoder** (2 hours)
- [ ] Implement decoder for `ENCODING_CACHED_RECT`
- [ ] Lookup cache ID and blit to framebuffer
- [ ] Handle cache miss (request refresh)
- [ ] Add to decoder registry

**File**: `rfb-encodings/src/cached_rect.rs` (~200 LOC)

```rust
// rfb-encodings/src/cached_rect.rs
use crate::{Decoder, MutablePixelBuffer, PixelFormat, Rectangle, RfbInStream};
use anyhow::{Context, Result};
use rfb_common::Rect;
use tokio::io::AsyncRead;

pub struct CachedRectDecoder {
    cache: std::sync::Arc<std::sync::Mutex<super::content_cache::ContentCache>>,
}

impl Decoder for CachedRectDecoder {
    fn encoding_type(&self) -> i32 {
        super::ENCODING_CACHED_RECT
    }

    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read cache ID
        let cache_id = stream.read_u64().await
            .context("Failed to read cache_id from CachedRect")?;

        // Lookup in cache
        let mut cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.lookup(cache_id) {
            // Cache hit - blit pixels to framebuffer
            let dest = Rect::new(
                rect.x as i32, 
                rect.y as i32,
                rect.width as u32,
                rect.height as u32
            );
            buffer.image_rect(dest, &cached.pixels, cached.stride)
                .context("Failed to blit cached pixels")?;
        } else {
            // Cache miss - need to request refresh
            // This will be handled by returning an error that triggers
            // a framebuffer update request
            anyhow::bail!(
                "Cache miss for ID {}: client needs refresh for rect ({},{} {}x{})",
                cache_id, rect.x, rect.y, rect.width, rect.height
            );
        }

        Ok(())
    }
}
```

**Task 2.3: CachedRectInit Decoder** (3 hours)
- [ ] Implement decoder for `ENCODING_CACHED_RECT_INIT`
- [ ] Read actual encoding type
- [ ] Dispatch to appropriate decoder
- [ ] Store decoded pixels in cache
- [ ] Handle nested decoding

**File**: `rfb-encodings/src/cached_rect_init.rs` (~250 LOC)

---

### Phase 3: Integration (Week 3)

**Task 3.1: Update Connection Manager** (2 hours)
- [ ] Add ContentCache instance to ConnectionManager
- [ ] Pass cache to decoders
- [ ] Handle cache miss errors (request refresh)
- [ ] Add cache statistics logging

**File**: Update `njcvncviewer-rs/src/connection.rs` (~50 LOC changes)

**Task 3.2: Update Decoder Registry** (1 hour)
- [ ] Add CachedRect and CachedRectInit decoders
- [ ] Pass cache reference to decoder constructors
- [ ] Handle Arc<Mutex<ContentCache>> threading

**File**: Update `njcvncviewer-rs/src/connection.rs` (~30 LOC changes)

```rust
// In DecoderImpl enum:
enum DecoderImpl {
    Raw(RawDecoder),
    CopyRect(CopyRectDecoder),
    CachedRect(CachedRectDecoder),
    CachedRectInit(CachedRectInitDecoder),
}

// In DecoderRegistry::new():
let cache = Arc::new(Mutex::new(ContentCache::new(2048))); // 2GB default

decoders.insert(
    ENCODING_CACHED_RECT,
    DecoderImpl::CachedRect(CachedRectDecoder::new(cache.clone()))
);
decoders.insert(
    ENCODING_CACHED_RECT_INIT,
    DecoderImpl::CachedRectInit(CachedRectInitDecoder::new(cache.clone()))
);
```

**Task 3.3: Configuration** (1 hour)
- [ ] Add ContentCache parameters to config
- [ ] Command-line flags (--cache-size, --disable-cache)
- [ ] Default values

**File**: Update `njcvncviewer-rs/src/main.rs` (~30 LOC changes)

---

### Phase 4: Testing and Optimization (Week 4)

**Task 4.1: Unit Tests** (4 hours)
- [ ] ContentCache operations (insert, lookup, evict)
- [ ] CachedRect decoder with mock cache
- [ ] CachedRectInit decoder with various encodings
- [ ] Cache miss recovery flow
- [ ] Memory limit enforcement

**Task 4.2: Integration Testing** (4 hours)
- [ ] Test with real VNC server (C++ TigerVNC with ContentCache)
- [ ] Verify cache hits reduce bandwidth
- [ ] Test cache miss recovery
- [ ] Test memory limits and eviction
- [ ] Performance benchmarking

**Task 4.3: Documentation** (2 hours)
- [ ] Update README with ContentCache support
- [ ] Add architecture diagrams
- [ ] Document configuration options
- [ ] Add troubleshooting guide

---

## Testing Strategy

### Unit Testing

```rust
#[tokio::test]
async fn test_cached_rect_decoder_hit() {
    let cache = Arc::new(Mutex::new(ContentCache::new(100)));
    
    // Pre-populate cache
    {
        let mut c = cache.lock().unwrap();
        c.insert(12345, CachedPixels {
            cache_id: 12345,
            pixels: vec![0xFF; 64 * 64 * 4],
            format: PixelFormat::rgb888(),
            width: 64,
            height: 64,
            stride: 64,
            last_used: std::time::Instant::now(),
        }).unwrap();
    }
    
    let decoder = CachedRectDecoder::new(cache);
    let mut buffer = ManagedPixelBuffer::new(1024, 768, PixelFormat::rgb888());
    
    // Create stream with cache ID
    let data = 12345u64.to_be_bytes();
    let mut stream = RfbInStream::new(std::io::Cursor::new(data.to_vec()));
    
    let rect = Rectangle {
        x: 100,
        y: 100,
        width: 64,
        height: 64,
        encoding: ENCODING_CACHED_RECT,
    };
    
    let result = decoder.decode(&mut stream, &rect, &PixelFormat::rgb888(), &mut buffer).await;
    assert!(result.is_ok());
    
    // Verify pixels were blitted to buffer
    // ...
}
```

### Integration Testing

**Test Scenario 1: Basic Cache Flow**
1. Connect to TigerVNC server with ContentCache enabled
2. Display static content (text document)
3. Switch windows and return
4. Verify CachedRect messages received
5. Measure bandwidth savings

**Test Scenario 2: Cache Miss Recovery**
1. Connect with small cache size (e.g., 10MB)
2. Display diverse content to fill cache
3. Return to earlier content (should be evicted)
4. Verify CachedRectInit re-transmission
5. Verify recovery is transparent to user

---

## Performance Targets

| Metric | Target | Current |
|--------|--------|---------|
| **Cache hit rate** | >80% for typical usage | N/A (not implemented) |
| **Bandwidth reduction** | 97-99% on cache hits | N/A |
| **Memory overhead** | <2GB default | N/A |
| **Cache lookup latency** | <1ms | N/A |
| **Decode latency** | <5ms for 64x64 tile | ~2ms (Raw) |

---

## Future Enhancements

### Phase 5: Advanced Encodings (Weeks 5-8)
- [ ] Tight encoding decoder (JPEG + zlib)
- [ ] ZRLE encoding decoder
- [ ] Hextile encoding decoder
- [ ] RRE encoding decoder

### Phase 6: ARC Algorithm (Week 9)
- [ ] Implement full ARC eviction policy
- [ ] T1/T2 lists for recency vs frequency
- [ ] B1/B2 ghost lists for adaptive tuning
- [ ] Performance comparison with LRU

### Phase 7: Advanced Features (Weeks 10-12)
- [ ] Touch gesture support (pinch zoom, swipe)
- [ ] Clipboard integration
- [ ] Connection profiles
- [ ] Full-screen mode improvements
- [ ] Multi-monitor support

---

## Building and Running

### Current Build

```bash
cd /home/nickc/code/tigervnc/rust-vnc-viewer

# Build everything
cargo build --package njcvncviewer-rs

# Run viewer (connects to display :2)
cargo run --package njcvncviewer-rs -- localhost:2

# Run with verbose logging
cargo run --package njcvncviewer-rs -- -vv localhost:2

# Run tests
cargo test --package rfb-encodings
```

### After ContentCache Implementation

```bash
# Run with ContentCache enabled (default)
cargo run --package njcvncviewer-rs -- localhost:2

# Run with custom cache size (4GB)
cargo run --package njcvncviewer-rs -- --cache-size 4096 localhost:2

# Run with ContentCache disabled (for comparison)
cargo run --package njcvncviewer-rs -- --disable-cache localhost:2

# Run with cache statistics logging
cargo run --package njcvncviewer-rs -- -vv --cache-stats localhost:2
```

---

## Timeline Estimate

| Week | Focus | Deliverables |
|------|-------|--------------|
| **Week 1** | Protocol messages | CachedRect/CachedRectInit types, capability negotiation |
| **Week 2** | Client cache | ContentCache struct, LRU eviction, decoders |
| **Week 3** | Integration | Connection manager updates, decoder registry, config |
| **Week 4** | Testing | Unit tests, integration tests, documentation |
| **Week 5-8** | Encodings | Tight, ZRLE, Hextile, RRE decoders |
| **Week 9** | ARC | Advanced eviction policy |
| **Week 10-12** | Polish | Touch, clipboard, profiles, full-screen |

**Total Estimate**: 12 weeks for full ContentCache + advanced features

---

## References

- [CONTENTCACHE_DESIGN_IMPLEMENTATION.md](../CONTENTCACHE_DESIGN_IMPLEMENTATION.md) - C++ implementation reference
- [CACHE_PROTOCOL_DESIGN.md](../CACHE_PROTOCOL_DESIGN.md) - Protocol specification
- [ARC_ALGORITHM.md](../ARC_ALGORITHM.md) - ARC eviction algorithm details
- [WARP.md](../WARP.md) - Build and test environment

---

## Success Criteria

### Minimum Viable ContentCache (Week 4)
- ✅ CachedRect and CachedRectInit messages implemented
- ✅ Client-side cache with LRU eviction
- ✅ Decoders integrated into viewer
- ✅ Cache hits successfully blit pixels
- ✅ Cache misses trigger re-transmission
- ✅ Unit tests passing (90%+ coverage)
- ✅ Integration test with C++ server succeeds
- ✅ Measurable bandwidth reduction (>90% on repetitive content)

### Full Feature Set (Week 12)
- All standard encodings supported
- ARC eviction policy operational
- Touch gestures working
- Clipboard functional
- Performance matches or exceeds C++ viewer
- Comprehensive documentation
- Production-ready code quality

---

**Status**: Ready to begin ContentCache implementation (Week 1 tasks)
