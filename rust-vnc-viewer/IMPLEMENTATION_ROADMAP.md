# Rust VNC Viewer Consolidation & Cache Implementation Roadmap

**Date**: 2025-10-27  
**Goal**: Consolidate two Rust viewers and implement ContentCache + PersistentCache protocols

## Phase 1: Consolidation (IN PROGRESS)

### ✅ **Step 1.1: Port Fullscreen Features** (STARTED)
- [x] Port `display.rs` module (monitor enumeration)
- [x] Port `fullscreen.rs` module (fullscreen controller) 
- [x] Add `--monitor` CLI option
- [ ] Update `main.rs` to initialize monitor enumeration
- [ ] Integrate fullscreen controller into `app.rs`
- [ ] Add keyboard shortcuts (F11, Ctrl+Alt+F, etc.)

### **Step 1.2: Consolidate Configuration Systems**
- [ ] Merge CLI argument structures from both viewers
- [ ] Unify TOML configuration files
- [ ] Consolidate environment variable handling
- [ ] Add ContentCache configuration options

### **Step 1.3: Remove `rvncviewer` Crate**  
- [ ] Verify all features ported successfully
- [ ] Remove from workspace Cargo.toml
- [ ] Update build system (Makefile, symlinks)
- [ ] Update documentation references

## Phase 2: ContentCache Integration

### **Step 2.1: Wire ContentCache into njcvncviewer-rs**
The ContentCache components exist but aren't connected:

**Files to modify:**
- [ ] `njcvncviewer-rs/src/app.rs` - Add ContentCache instance
- [ ] Update VNC client connection to use ContentCache
- [ ] Add capability negotiation (-320 pseudo-encoding)
- [ ] Wire CachedRect/CachedRectInit decoders into decode registry

**Key Integration Points:**
```rust
// In app.rs, add ContentCache to connection
let content_cache = Arc::new(Mutex::new(ContentCache::new(config.cache_size_mb)));

// In connection setup, register cache decoders
decoder_registry.register(ENCODING_CACHED_RECT, CachedRectDecoder::new(cache.clone()));
decoder_registry.register(ENCODING_CACHED_RECT_INIT, CachedRectInitDecoder::new(cache.clone()));

// Add capability negotiation
fn build_encodings_list() -> Vec<i32> {
    vec![
        ENCODING_TIGHT, ENCODING_ZRLE, // ... standard encodings
        PSEUDO_ENCODING_CONTENT_CACHE, // -320 for ContentCache capability
        PSEUDO_ENCODING_LAST_RECT,     // -224 etc.
    ]
}
```

### **Step 2.2: Add ContentCache Configuration**
- [ ] Add CLI options: `--cache-size`, `--disable-cache`
- [ ] Add TOML config section for cache settings
- [ ] Add statistics logging and periodic reports

### **Step 2.3: Test ContentCache Integration**
- [ ] Run against C++ server with ContentCache enabled
- [ ] Verify E2E test framework shows cache hits
- [ ] Compare bandwidth reduction with C++ viewer

## Phase 3: PersistentCache Protocol

### **Step 3.1: Implement Core PersistentCache Components**

**New files to create:**
```
rfb-encodings/src/persistent_cache/
├── mod.rs              # Public API, PersistentCache struct
├── hashing.rs          # SHA-256 content hashing (stride in pixels!)
├── arc.rs              # ARC cache with T1/T2/B1/B2 lists (byte-sized)
├── store.rs            # Disk persistence: ~/.cache/tigervnc/
├── wire.rs             # Protocol constants, 16-byte cache IDs
└── metrics.rs          # Statistics, periodic reporting
```

**Key Implementation Details:**
- **Cache ID**: SHA-256 truncated to 16 bytes (not 32)
- **Stride Handling**: CRITICAL - stride is in pixels, multiply by bytes_per_pixel
- **Capacity**: Track in bytes, not entry count
- **ARC Algorithm**: T1 (recent), T2 (frequent), B1/B2 (ghost lists)

### **Step 3.2: Protocol Integration**

**Capability Negotiation:**
```rust
// CRITICAL: Order matters! -321 BEFORE -320
vec![
    ENCODING_TIGHT, ENCODING_ZRLE, // standard encodings...
    PSEUDO_ENCODING_PERSISTENT_CACHE, // -321 (FIRST!)
    PSEUDO_ENCODING_CONTENT_CACHE,    // -320 (fallback)
    PSEUDO_ENCODING_LAST_RECT,        // -224
]
```

**New Encodings:**
- `102` - PersistentCachedRect (16-byte hash)
- `103` - PersistentCachedRectInit (16-byte hash + actual encoding + data)

### **Step 3.3: Disk Persistence**

**Cache Storage:**
- Location: `~/.cache/tigervnc/rust-viewer/persistentcache-v1.dat`
- Format: Custom binary with checksums
- Features: Corruption recovery, versioning, size limits

**Startup/Shutdown:**
- Load from disk on startup
- Periodic saves during operation  
- Graceful shutdown save

## Phase 4: Testing & Validation

### **Step 4.1: Unit Testing**
- [ ] Test ContentCache hit/miss scenarios
- [ ] Test PersistentCache hashing (verify stride handling)
- [ ] Test ARC eviction algorithm
- [ ] Test disk persistence and corruption recovery

### **Step 4.2: Integration Testing**
- [ ] E2E ContentCache tests (should show >90% hit rates)
- [ ] E2E PersistentCache tests (cross-session persistence)
- [ ] Performance comparison with C++ implementation
- [ ] Multi-monitor fullscreen testing

### **Step 4.3: Performance Validation**

**Expected Results:**
- **ContentCache**: 97-99% bandwidth reduction for repeated content  
- **PersistentCache**: Near-instant reconnects (99.9% cache hits from disk)
- **Fullscreen**: Smooth multi-monitor navigation with F11/hotkeys

## Success Criteria

### **Functional Requirements:**
- ✅ Single consolidated Rust viewer binary
- ✅ ContentCache protocol matching C++ performance
- ✅ PersistentCache with cross-session persistence
- ✅ Multi-monitor fullscreen support
- ✅ All existing features preserved

### **Performance Targets:**
- **Bandwidth**: 97-99% reduction (ContentCache) + 99.9% (PersistentCache)
- **Memory**: <2GB cache footprint (configurable)
- **Startup**: <500ms to load cache from disk
- **UI**: 60fps rendering with smooth fullscreen transitions

### **Quality Gates:**
- ✅ All existing tests passing
- ✅ E2E tests showing cache hit rates >90%
- ✅ No memory leaks or crashes during extended usage
- ✅ Graceful handling of corrupt cache files

## Timeline Estimate

**Phase 1 (Consolidation)**: 1-2 days
**Phase 2 (ContentCache)**: 2-3 days  
**Phase 3 (PersistentCache)**: 3-4 days
**Phase 4 (Testing)**: 1-2 days

**Total**: 7-11 days for complete implementation

## Critical Implementation Notes

### **⚠️ Gotchas to Avoid:**

1. **Stride is in pixels, not bytes!**
   ```rust
   // ❌ WRONG
   let row_offset = y * stride;
   
   // ✅ CORRECT  
   let bytes_per_pixel = pixel_format.bits_per_pixel / 8;
   let row_offset = y * stride * bytes_per_pixel;
   ```

2. **Cache ID is 16 bytes (not 32 from SHA-256)**
   ```rust
   let hash = sha256(&pixel_data);
   let cache_id: [u8; 16] = hash[0..16].try_into().unwrap();
   ```

3. **Capability negotiation order matters**
   ```rust
   // Server picks FIRST supported capability
   vec![-321, -320, -224] // PersistentCache preferred over ContentCache
   ```

4. **ARC capacity in bytes, not entries**
   ```rust
   struct ARCCache {
       max_bytes: usize,           // NOT max_entries
       current_bytes: usize,       // Sum of all pixel data sizes
   }
   ```

## Next Actions

1. **Complete Phase 1.1**: Finish fullscreen integration
2. **Start Phase 2.1**: Wire ContentCache into njcvncviewer-rs
3. **Verify E2E tests**: Ensure Rust viewer shows cache activity
4. **Begin Phase 3.1**: Implement PersistentCache core components

This roadmap provides a systematic approach to achieving a production-ready consolidated Rust VNC viewer with state-of-the-art caching protocols.