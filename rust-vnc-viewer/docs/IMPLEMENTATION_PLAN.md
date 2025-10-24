# Implementation Plan - Desktop-Focused VNC Viewer

Implementation roadmap for fullscreen and multi-monitor support in the Rust VNC viewer.

## Overview

Focus on delivering excellent **fullscreen** and **multi-monitor** support for desktop VNC workflows. Features outside this scope are explicitly out-of-scope per [SEP-0001](SEP/SEP-0001-out-of-scope.md).

## Module Architecture

### Proposed Module Structure

```
njcvncviewer-rs/src/
├── main.rs                         # Entry point, CLI parsing, app bootstrap
├── app.rs                          # Main application state and event loop
├── cli.rs                          # Extended CLI parsing (--fullscreen, --monitor, --scale)
├── display/
│   ├── mod.rs                      # DisplayManager trait and Monitor model
│   ├── winit_backend.rs            # Winit-based monitor enumeration implementation
│   └── monitor_selection.rs        # Primary/index/name selection logic
├── fullscreen/
│   ├── mod.rs                      # FullscreenController and state management
│   ├── transitions.rs              # Enter/exit/toggle logic with state preservation
│   └── hotkeys.rs                  # F11, Ctrl+Alt+Arrow, Ctrl+Alt+0-9 navigation
├── scaling/
│   ├── mod.rs                      # Scaling policies (fit/fill/1:1)
│   ├── calculations.rs             # Viewport and aspect ratio mathematics
│   └── dpi.rs                      # DPI handling for mixed environments
└── config.rs                       # Configuration management (CLI + env vars)
```

### Files to Remove/Avoid

**Explicitly excluded** to maintain focus:
- ❌ `src/touch.rs` or `src/gestures/` - Touch support out-of-scope
- ❌ `src/ui/settings/` or `src/profiles/` - Settings UI out-of-scope
- ❌ `src/screenshot.rs` or recording features - Use OS tools instead
- ❌ Configuration UI components - CLI-only configuration

### Core Types and Traits

```rust
// display/mod.rs
pub trait DisplayManager {
    fn list_monitors(&self) -> Vec<Monitor>;
    fn primary_monitor(&self) -> Option<Monitor>;
    fn get_monitor_by_index(&self, index: usize) -> Option<Monitor>;
    fn get_monitor_by_name(&self, name_substring: &str) -> Option<Monitor>;
}

pub struct Monitor {
    pub handle: MonitorHandle,
    pub index: usize,
    pub name: String,
    pub resolution: (u32, u32),
    pub scale_factor: f64,
    pub is_primary: bool,
    pub position: (i32, i32),
}

// fullscreen/mod.rs
pub struct FullscreenController {
    current_state: FullscreenState,
    windowed_state: Option<WindowedState>,
    display_manager: Arc<dyn DisplayManager>,
}

#[derive(Debug, Clone)]
pub enum FullscreenState {
    Windowed,
    Fullscreen { monitor: Monitor, mode: FullscreenMode },
}

#[derive(Debug, Clone)]
pub enum FullscreenMode {
    Borderless,
    Exclusive,
}

// scaling/mod.rs
#[derive(Debug, Clone, Copy)]
pub enum ScalingPolicy {
    Fit,      // Scale to fit with letterboxing
    Fill,     // Scale to fill (may crop/stretch)
    Native,   // 1:1 pixel mapping
}

pub struct ScalingCalculator {
    policy: ScalingPolicy,
    keep_aspect: bool,
}
```

## Task Breakdown

### Task 1: CLI Enhancement (0.5-1 day)

**Scope**: Extend CLI argument parsing for fullscreen/multi-monitor options

**Files**: 
- `src/cli.rs` - New CLI parser with fullscreen/monitor options
- `src/main.rs` - Integration with app initialization

**CLI Options to Add**:
```bash
--fullscreen, -F           # Start in fullscreen mode
--monitor, -m SELECTOR     # Monitor: primary|index|name
--scale POLICY             # Scaling: fit|fill|1:1 
--keep-aspect BOOL         # Preserve aspect ratio (default: true)
--cursor MODE              # Cursor: local|remote (default: local)
```

**Implementation**:
```rust
#[derive(Parser, Debug)]
pub struct CliArgs {
    // ... existing args
    
    #[arg(short = 'F', long)]
    pub fullscreen: bool,
    
    #[arg(short = 'm', long, value_name = "SELECTOR")]
    pub monitor: Option<String>,
    
    #[arg(long, value_enum, default_value = "fit")]
    pub scale: ScalingPolicy,
    
    #[arg(long, default_value = "true")]
    pub keep_aspect: bool,
}
```

**Tests**:
- CLI parsing for all new flags and combinations
- Error handling for invalid monitor selectors
- Environment variable integration

### Task 2: Monitor Enumeration (1 day)

**Scope**: Cross-platform monitor detection and selection

**Files**:
- `src/display/mod.rs` - DisplayManager trait and Monitor types
- `src/display/winit_backend.rs` - Winit implementation
- `src/display/monitor_selection.rs` - Selection logic

**Monitor Selection Algorithm**:
1. **Primary**: First choice if `primary` specified
2. **Index**: Zero-based index in deterministic order
3. **Name**: Case-insensitive substring match
4. **Fallback**: Primary monitor if target not found

**Deterministic Ordering**:
- Primary monitor always index 0
- Secondary monitors sorted by position (left→right, top→bottom)
- Virtual displays sorted last

**Implementation**:
```rust
impl DisplayManager for WinitDisplayManager {
    fn list_monitors(&self) -> Vec<Monitor> {
        let mut monitors: Vec<_> = self.event_loop
            .available_monitors()
            .enumerate()
            .map(|(i, handle)| Monitor {
                handle: handle.clone(),
                index: i,
                name: handle.name().unwrap_or_else(|| format!("Monitor {}", i)),
                resolution: handle.size().into(),
                scale_factor: handle.scale_factor(),
                is_primary: i == 0, // Winit primary detection
                position: handle.position().into(),
            })
            .collect();
            
        // Sort: primary first, then by position
        monitors.sort_by_key(|m| (if m.is_primary { 0 } else { 1 }, m.position));
        monitors
    }
}
```

**Tests**:
- Mock DisplayManager with 1/2/3 monitor configurations
- Primary detection in various scenarios
- Name/index selection edge cases

### Task 3: Fullscreen Implementation (1-2 days)

**Scope**: Reliable fullscreen entry/exit with state management

**Files**:
- `src/fullscreen/mod.rs` - FullscreenController
- `src/fullscreen/transitions.rs` - State transition logic  
- `src/fullscreen/hotkeys.rs` - Keyboard shortcut handling

**State Management**:
```rust
struct WindowedState {
    size: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
    decorations: bool,
}

impl FullscreenController {
    pub fn enter_fullscreen(
        &mut self, 
        window: &Window, 
        target_monitor: Option<Monitor>
    ) -> Result<(), FullscreenError> {
        // 1. Store current windowed state
        let windowed_state = WindowedState {
            size: window.inner_size(),
            position: window.outer_position()?,
            decorations: window.decorations(),
        };
        
        // 2. Select target monitor
        let monitor = target_monitor
            .or_else(|| self.display_manager.primary_monitor())
            .ok_or(FullscreenError::NoMonitorAvailable)?;
            
        // 3. Configure fullscreen
        window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor.handle.clone()))));
        
        // 4. Update state
        self.current_state = FullscreenState::Fullscreen { 
            monitor: monitor.clone(), 
            mode: FullscreenMode::Borderless 
        };
        self.windowed_state = Some(windowed_state);
        
        info!("Entered fullscreen on monitor: {}", monitor.name);
        Ok(())
    }
}
```

**Hotkey Integration**:
- F11: Primary fullscreen toggle
- Ctrl+Alt+F: Alternative toggle
- Ctrl+Alt+←/→: Move to prev/next monitor
- Ctrl+Alt+0-9: Jump to monitor by index
- Esc: Exit fullscreen (optional)

**Tests**:
- State transition correctness
- Monitor switching without artifacts
- State preservation across transitions

### Task 4: Scaling Implementation (1 day)

**Scope**: Viewport scaling with aspect ratio preservation

**Files**:
- `src/scaling/mod.rs` - ScalingCalculator and policies
- `src/scaling/calculations.rs` - Mathematical viewport calculations
- `src/scaling/dpi.rs` - DPI scaling for mixed environments

**Scaling Algorithm**:
```rust
impl ScalingCalculator {
    pub fn calculate_viewport(
        &self,
        remote_size: (u32, u32),
        window_size: (u32, u32),
        dpi_scale: f64,
    ) -> ViewportInfo {
        let effective_window = (
            (window_size.0 as f64 / dpi_scale) as u32,
            (window_size.1 as f64 / dpi_scale) as u32,
        );
        
        match self.policy {
            ScalingPolicy::Fit => self.calculate_fit(remote_size, effective_window),
            ScalingPolicy::Fill => self.calculate_fill(remote_size, effective_window),
            ScalingPolicy::Native => self.calculate_native(remote_size, effective_window),
        }
    }
    
    fn calculate_fit(&self, remote: (u32, u32), window: (u32, u32)) -> ViewportInfo {
        let scale_x = window.0 as f32 / remote.0 as f32;
        let scale_y = window.1 as f32 / remote.1 as f32;
        let scale = scale_x.min(scale_y);
        
        let scaled_size = (
            (remote.0 as f32 * scale) as u32,
            (remote.1 as f32 * scale) as u32,
        );
        
        let offset = (
            (window.0 - scaled_size.0) / 2,
            (window.1 - scaled_size.1) / 2,
        );
        
        ViewportInfo { scale, offset, scaled_size }
    }
}
```

**Tests**:
- Scaling calculations for various aspect ratios
- DPI handling with different scale factors
- Letterboxing and centering accuracy

### Task 5: Integration and Testing (0.5-1 day)

**Scope**: End-to-end integration and comprehensive testing

**Integration Points**:
- CLI → App initialization with fullscreen/monitor preferences
- App → FullscreenController for state management
- App → ScalingCalculator for viewport updates
- Event handling for hotkeys and monitor changes

**Test Strategy**:

**Unit Tests**:
```rust
#[test]
fn test_monitor_selection_by_index() {
    let manager = MockDisplayManager::with_monitors(3);
    assert_eq!(manager.get_monitor_by_index(1).unwrap().index, 1);
    assert!(manager.get_monitor_by_index(99).is_none());
}

#[test] 
fn test_scaling_fit_calculation() {
    let calc = ScalingCalculator::new(ScalingPolicy::Fit, true);
    let viewport = calc.calculate_viewport((800, 600), (1920, 1080), 1.0);
    assert_eq!(viewport.scale, 1.8); // 1080/600
    assert_eq!(viewport.offset, (280, 0)); // Centered horizontally
}
```

**Integration Tests**:
- Window creation and fullscreen transitions
- Monitor enumeration on test systems
- CLI argument parsing end-to-end

**Manual QA** (following WARP safety rules):
- Single monitor: F11 toggle, scaling modes
- Dual monitors: Ctrl+Alt+Arrow navigation, index selection
- Mixed DPI: Scaling correctness across different DPI monitors
- Server testing: Only use Xnjcvnc :2, never production :1 or :3

## Dependencies

### Required Crates
```toml
# Window management and monitor APIs
winit = "0.28"                      # Cross-platform windowing
egui = "0.27"                       # GUI framework  
eframe = "0.27"                     # egui application framework

# CLI and configuration
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# Logging and error handling
tracing = "0.1"
anyhow = "1.0"
thiserror = "1.0"
```

### Platform Dependencies
- **X11**: EWMH support for fullscreen and monitor detection
- **Wayland**: wl_output protocol for monitor enumeration
- **winit**: Abstraction layer over platform differences

## Error Handling

### Error Types
```rust
#[derive(Debug, thiserror::Error)]
pub enum FullscreenError {
    #[error("No monitor available for fullscreen")]
    NoMonitorAvailable,
    
    #[error("Monitor '{0}' not found")]
    MonitorNotFound(String),
    
    #[error("Failed to enter fullscreen: {0}")]
    TransitionFailed(String),
    
    #[error("Platform not supported: {0}")]
    PlatformUnsupported(String),
}
```

### Graceful Degradation
1. **Monitor not found**: Log warning, use primary monitor
2. **Exclusive fullscreen unsupported**: Fall back to borderless
3. **Monitor disconnected**: Detect and move to available monitor
4. **DPI detection failed**: Use 1.0 scale factor with warning

## Success Criteria

### Milestone M1 (Fullscreen)
- [ ] F11 toggle works reliably on X11 and Wayland
- [ ] CLI `--fullscreen` starts in fullscreen mode
- [ ] Scaling policies (fit/fill/1:1) render correctly
- [ ] DPI-aware scaling on high-resolution monitors
- [ ] Smooth transitions without visual artifacts
- [ ] State preservation across fullscreen/windowed transitions

### Milestone M2 (Multi-monitor)
- [ ] Accurate enumeration of 2-4 monitor setups
- [ ] CLI `--monitor primary|0|1|name` selection works
- [ ] Hotkey navigation (Ctrl+Alt+Arrow, Ctrl+Alt+0-9)
- [ ] Mixed DPI environments handled gracefully
- [ ] Monitor disconnect/reconnect recovery
- [ ] Clear error messages for invalid monitor selections

### Code Quality
- [ ] Zero clippy warnings
- [ ] Comprehensive unit test coverage (>90%)
- [ ] Integration tests for all major workflows
- [ ] Clear error messages and logging
- [ ] Performance: <200ms fullscreen transitions, <1ms scaling calculations

## Timeline

| Task | Duration | Dependencies | 
|------|----------|--------------|
| CLI Enhancement | 0.5-1 day | - |
| Monitor Enumeration | 1 day | CLI complete |
| Fullscreen Implementation | 1-2 days | Monitor enumeration |
| Scaling Implementation | 1 day | Fullscreen basics |
| Integration & Testing | 0.5-1 day | All tasks |

**Total Estimate**: 4-6.5 days for both M1 and M2 milestones.

---

**Next Steps**: Begin with Task 1 (CLI Enhancement) and proceed sequentially. Each task builds on the previous, enabling incremental testing and validation.

## PersistentCache Protocol Implementation (M3)

Implementation roadmap for the PersistentCache protocol in the Rust VNC viewer, following the completed C++ implementation.

### Overview

PersistentCache extends ContentCache with **content-addressable hashing** to enable:
- **Cross-session persistence**: Cache survives client restarts
- **Cross-server compatibility**: Same content cached regardless of server
- **Stable references**: SHA-256 hashes instead of server-assigned IDs

### Protocol Summary

**Pseudo-encoding**: `-321` (indicates PersistentCache support)  
**Encodings**: 
- `102` (PersistentCachedRect): Hash reference to cached content
- `103` (PersistentCachedRectInit): Full data + hash for caching

**Client messages**:
- `254` (PersistentCacheQuery): Request missing hashes
- `253` (PersistentHashList): Advertise known hashes (optional)

**Negotiation**: Client sends `-321` and `-320` in SetEncodings; server prefers PersistentCache if available.

### Module Structure

```
rfb-protocol/
├── src/
│   ├── content_hash.rs           # NEW: SHA-256 hashing utility
│   └── messages/
│       ├── client.rs             # ADD: PersistentCacheQuery, PersistentHashList writers
│       └── types.rs              # ADD: Message type constants 254, 253

rfb-encodings/
├── src/
│   ├── lib.rs                    # ADD: Constants for encodings 102, 103, pseudo-encoding -321
│   ├── persistent_cache.rs       # NEW: GlobalClientPersistentCache with ARC
│   ├── persistent_cached_rect.rs # NEW: Decoder for encoding 102
│   └── persistent_cached_rect_init.rs # NEW: Decoder for encoding 103

rfb-client/
├── src/
│   └── decoder_registry.rs       # MODIFY: Register new decoders
│   └── connection.rs             # MODIFY: Protocol negotiation, query batching
```

### Task Breakdown

### Task PC-1: Protocol Constants and Types (0.5 day)

**Scope**: Add protocol foundation without behavior changes

**Files**:
- `rfb-encodings/src/lib.rs`
- `rfb-protocol/src/messages/types.rs`

**Implementation**:
```rust
// rfb-encodings/src/lib.rs
pub const ENCODING_PERSISTENT_CACHED_RECT: i32 = 102;
pub const ENCODING_PERSISTENT_CACHED_RECT_INIT: i32 = 103;
pub const PSEUDO_ENCODING_PERSISTENT_CACHE: i32 = -321;

// rfb-protocol/src/messages/types.rs
pub const MSG_TYPE_PERSISTENT_CACHE_QUERY: u8 = 254;
pub const MSG_TYPE_PERSISTENT_CACHE_HASH_LIST: u8 = 253;
```

**Tests**:
- Constant values match C++ encodings.h
- Module compiles without warnings

### Task PC-2: Content Hash Utility (0.5 day)

**Scope**: SHA-256 hashing with correct stride handling

**File**: `rfb-protocol/src/content_hash.rs`

**Implementation**:
```rust
use sha2::{Sha256, Digest};

/// Compute 16-byte hash of rectangle pixel data.
/// CRITICAL: stride is in pixels, multiply by bytes_per_pixel!
pub fn compute_rect_hash(
    pixels: &[u8],
    width: usize,
    height: usize,
    stride_pixels: usize,
    bytes_per_pixel: usize,
) -> [u8; 16] {
    let mut hasher = Sha256::new();
    let stride_bytes = stride_pixels * bytes_per_pixel;
    let row_bytes = width * bytes_per_pixel;
    
    for y in 0..height {
        let row_start = y * stride_bytes;
        let row_end = row_start + row_bytes;
        hasher.update(&pixels[row_start..row_end]);
    }
    
    let result = hasher.finalize();
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&result[..16]);
    hash
}
```

**Tests**:
- Hash matches C++ ContentHash::computeRect for known inputs
- Stride handling: verify pixels vs bytes multiplication
- Deterministic: same input → same hash

### Task PC-3: GlobalClientPersistentCache (2 days)

**Scope**: In-memory cache with ARC eviction (disk persistence in PC-5)

**File**: `rfb-encodings/src/persistent_cache.rs`

**Data Structures**:
```rust
use indexmap::IndexMap;
use std::collections::HashSet;

pub struct GlobalClientPersistentCache {
    // Main storage: hash → pixel data
    cache: HashMap<[u8; 16], CachedEntry>,
    
    // ARC lists (most recent at front)
    t1: IndexMap<[u8; 16], ()>,  // Recently used once
    t2: IndexMap<[u8; 16], ()>,  // Frequently used
    b1: HashSet<[u8; 16]>,       // Ghost: evicted from T1
    b2: HashSet<[u8; 16]>,       // Ghost: evicted from T2
    
    // ARC parameter: target T1 size in bytes
    p: usize,
    
    // Configuration
    max_size_bytes: usize,
    
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

**Methods**:
```rust
impl GlobalClientPersistentCache {
    pub fn new(max_size_mb: usize) -> Self;
    pub fn has(&self, hash: &[u8; 16]) -> bool;
    pub fn get(&mut self, hash: &[u8; 16]) -> Option<&CachedEntry>;
    pub fn insert(&mut self, hash: [u8; 16], entry: CachedEntry);
    pub fn get_all_hashes(&self) -> Vec<[u8; 16]>;
    pub fn stats(&self) -> CacheStats;
    
    // ARC algorithm helpers
    fn replace(&mut self, hash: &[u8; 16], size: usize);
    fn move_to_t2(&mut self, hash: &[u8; 16]);
    fn evict_lru_from_t1(&mut self);
    fn evict_lru_from_t2(&mut self);
}
```

**ARC Algorithm** (adapted from C++ implementation):
1. **Cache hit in T1**: Move to T2 (frequency promotion)
2. **Cache hit in T2**: Move to front (LRU refresh)
3. **Ghost hit in B1**: Increase p, evict from T2, add to T2
4. **Ghost hit in B2**: Decrease p, evict from T1, add to T2
5. **Cache miss**: Add to T1, evict if needed

**Tests**:
- Basic insert/lookup operations
- ARC promotions: T1 → T2 on second access
- Ghost hits adjust parameter p correctly
- Size limits enforced: eviction when full
- Statistics accurate (hits, misses, size)

### Task PC-4: Client Protocol Messages (1 day)

**Scope**: Write and read protocol messages

**Files**:
- `rfb-protocol/src/messages/client.rs`

**PersistentCacheQuery Writer**:
```rust
pub async fn write_persistent_cache_query<W: AsyncWrite + Unpin>(
    writer: &mut W,
    hashes: &[[u8; 16]],
) -> Result<()> {
    writer.write_u8(MSG_TYPE_PERSISTENT_CACHE_QUERY).await?;
    writer.write_u16(hashes.len() as u16).await?;
    
    for hash in hashes {
        writer.write_u8(16).await?;  // Hash length
        writer.write_all(hash).await?;
    }
    
    Ok(())
}
```

**PersistentHashList Writer**:
```rust
pub async fn write_persistent_hash_list<W: AsyncWrite + Unpin>(
    writer: &mut W,
    sequence_id: u32,
    total_chunks: u16,
    chunk_index: u16,
    hashes: &[[u8; 16]],
) -> Result<()> {
    writer.write_u8(MSG_TYPE_PERSISTENT_CACHE_HASH_LIST).await?;
    writer.write_u32(sequence_id).await?;
    writer.write_u16(total_chunks).await?;
    writer.write_u16(chunk_index).await?;
    writer.write_u16(hashes.len() as u16).await?;
    
    for hash in hashes {
        writer.write_u8(16).await?;
        writer.write_all(hash).await?;
    }
    
    Ok(())
}
```

**Tests**:
- Wire format matches C++ implementation
- Endianness correct (network byte order)
- Handles empty hash lists
- Batching works with 1, 10, 1000 hashes

### Task PC-5: Decoders (1 day)

**Scope**: Implement encoding 102 and 103 decoders

**File**: `rfb-encodings/src/persistent_cached_rect.rs`

**PersistentCachedRect Decoder (102)**:
```rust
pub struct PersistentCachedRectDecoder {
    cache: Arc<Mutex<GlobalClientPersistentCache>>,
    pending_queries: Arc<Mutex<Vec<[u8; 16]>>>,
}

impl Decoder for PersistentCachedRectDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_PERSISTENT_CACHED_RECT
    }
    
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        _pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read hash
        let hash_len = stream.read_u8().await?;
        assert_eq!(hash_len, 16);
        let mut hash = [0u8; 16];
        stream.read_exact(&mut hash).await?;
        
        let _flags = stream.read_u16().await?;  // Reserved
        
        // Try cache lookup
        let mut cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.get(&hash) {
            // Cache hit: blit from cache
            blit_cached_pixels(cached, rect, buffer)?;
        } else {
            // Cache miss: queue query
            self.pending_queries.lock().unwrap().push(hash);
            // Optionally fill with placeholder color
        }
        
        Ok(())
    }
}
```

**File**: `rfb-encodings/src/persistent_cached_rect_init.rs`

**PersistentCachedRectInit Decoder (103)**:
```rust
pub struct PersistentCachedRectInitDecoder {
    cache: Arc<Mutex<GlobalClientPersistentCache>>,
    inner_decoders: Arc<DecoderRegistry>,
}

impl Decoder for PersistentCachedRectInitDecoder {
    fn encoding_type(&self) -> i32 {
        ENCODING_PERSISTENT_CACHED_RECT_INIT
    }
    
    async fn decode<R: AsyncRead + Unpin>(
        &self,
        stream: &mut RfbInStream<R>,
        rect: &Rectangle,
        pixel_format: &PixelFormat,
        buffer: &mut dyn MutablePixelBuffer,
    ) -> Result<()> {
        // Read hash
        let hash_len = stream.read_u8().await?;
        assert_eq!(hash_len, 16);
        let mut hash = [0u8; 16];
        stream.read_exact(&mut hash).await?;
        
        // Read inner encoding and payload
        let inner_encoding = stream.read_i32().await?;
        let payload_len = stream.read_u32().await?;
        
        // Decode inner payload
        let decoder = self.inner_decoders.get(inner_encoding)?;
        decoder.decode(stream, rect, pixel_format, buffer).await?;
        
        // Extract pixels from buffer and cache
        let pixels = extract_pixels_from_buffer(buffer, rect)?;
        let entry = CachedEntry {
            pixels,
            format: pixel_format.clone(),
            width: rect.width as u32,
            height: rect.height as u32,
            stride_pixels: rect.width,
        };
        
        self.cache.lock().unwrap().insert(hash, entry);
        
        Ok(())
    }
}
```

**Tests**:
- Mock server sends encoding 102/103 sequences
- Cache hit path blits correctly
- Cache miss path queues queries
- Init path decodes and caches
- Round-trip: Init → ref → hit

### Task PC-6: Client Integration (1 day)

**Scope**: Wire decoders and negotiation into connection

**Files**:
- `rfb-client/src/decoder_registry.rs`
- `rfb-client/src/connection.rs`

**Decoder Registration**:
```rust
// In decoder_registry.rs
pub fn new_with_persistent_cache(
    cache: Arc<Mutex<GlobalClientPersistentCache>>,
) -> Self {
    let mut registry = Self::new();
    
    // ... existing decoders
    
    // Add PersistentCache decoders
    registry.register(Box::new(PersistentCachedRectDecoder::new(
        cache.clone(),
    )));
    registry.register(Box::new(PersistentCachedRectInitDecoder::new(
        cache.clone(),
        Arc::new(registry.clone()),
    )));
    
    registry
}
```

**SetEncodings Negotiation**:
```rust
// In connection.rs
fn build_set_encodings(&self) -> Vec<i32> {
    vec![
        // Standard encodings
        ENCODING_TIGHT,
        ENCODING_ZRLE,
        // ... others
        
        // Pseudo-encodings (prefer PersistentCache)
        PSEUDO_ENCODING_PERSISTENT_CACHE,  // -321 (try first)
        PSEUDO_ENCODING_CONTENT_CACHE,      // -320 (fallback)
    ]
}
```

**Query Batching**:
```rust
impl Connection {
    async fn flush_pending_queries(&mut self) -> Result<()> {
        let queries = self.pending_queries.lock().unwrap().drain(..).collect::<Vec<_>>();
        
        if !queries.is_empty() {
            write_persistent_cache_query(&mut self.stream, &queries).await?;
        }
        
        Ok(())
    }
    
    // Call after each framebuffer update
    pub async fn on_frame_complete(&mut self) -> Result<()> {
        self.flush_pending_queries().await?;
        Ok(())
    }
}
```

**Tests**:
- SetEncodings includes both -321 and -320
- Query batching works across multiple misses
- Flush triggers after frame complete
- HashList sent after initial handshake

### Task PC-7: Disk Persistence (1-2 days)

**Scope**: Load/save cache to `~/.cache/tigervnc/persistentcache.dat`

**Dependencies**:
```toml
directories = "5"   # XDG cache directory
byteorder = "1"     # Binary I/O
```

**File Format**:
```rust
// Header (64 bytes)
struct CacheFileHeader {
    magic: u32,              // 0x50435643 ("PCVC")
    version: u32,            // 1
    total_entries: u64,
    total_bytes: u64,
    created: u64,            // Unix timestamp
    last_access: u64,
    _reserved: [u8; 24],
}

// Entry format
struct CacheEntry {
    hash_len: u8,            // Always 16
    hash: [u8; 16],
    width: u16,
    height: u16,
    stride_pixels: u16,
    pixel_format: [u8; 24],  // Serialized PixelFormat
    last_access_time: u32,
    pixel_data_len: u32,
    pixel_data: Vec<u8>,
}

// Trailing checksum (32 bytes)
// SHA-256 of entire file content
```

**Implementation**:
```rust
impl GlobalClientPersistentCache {
    pub fn load_from_disk() -> Result<Self> {
        let path = Self::cache_file_path()?;
        
        if !path.exists() {
            return Ok(Self::new(DEFAULT_SIZE_MB));
        }
        
        let mut file = File::open(&path)?;
        
        // Read header
        let header = Self::read_header(&mut file)?;
        if header.magic != 0x50435643 {
            warn!("Invalid cache file magic, starting fresh");
            return Ok(Self::new(DEFAULT_SIZE_MB));
        }
        
        // Read entries
        let mut cache = Self::new(DEFAULT_SIZE_MB);
        for _ in 0..header.total_entries {
            let entry = Self::read_entry(&mut file)?;
            cache.insert(entry.hash, entry.cached_pixels);
        }
        
        // Verify checksum
        let expected = Self::read_checksum(&mut file)?;
        // TODO: Compute and verify
        
        Ok(cache)
    }
    
    pub fn save_to_disk(&self) -> Result<()> {
        let path = Self::cache_file_path()?;
        
        // Create directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let mut file = File::create(&path)?;
        
        // Write header
        Self::write_header(&mut file, self)?;
        
        // Write entries
        for (hash, entry) in &self.cache {
            Self::write_entry(&mut file, hash, entry)?;
        }
        
        // Write checksum
        Self::write_checksum(&mut file)?;
        
        Ok(())
    }
    
    fn cache_file_path() -> Result<PathBuf> {
        let cache_dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            PathBuf::from(xdg)
        } else if let Some(home) = dirs::home_dir() {
            home.join(".cache")
        } else {
            bail!("Cannot determine cache directory");
        };
        
        Ok(cache_dir.join("tigervnc").join("persistentcache.dat"))
    }
}
```

**Error Handling**:
- Corruption: Log warning, start with empty cache, preserve `.bak`
- Missing directory: Create automatically
- Checksum mismatch: Discard and start fresh

**Tests**:
- Round-trip: save → load yields identical cache
- Corruption recovery: invalid magic → fresh cache
- Directory creation works
- Checksum verification (when implemented)

### Task PC-8: Testing and Validation (1 day)

**Unit Tests** (`rfb-protocol/tests/content_hash.rs`):
```rust
#[test]
fn test_hash_matches_cpp() {
    // Known test vector from C++ implementation
    let pixels = vec![0xFF; 64 * 64 * 4];
    let hash = compute_rect_hash(&pixels, 64, 64, 64, 4);
    
    let expected = [ /* from C++ */ ];
    assert_eq!(hash, expected);
}

#[test]
fn test_stride_handling() {
    // Stride=80 pixels, but only 64 pixels wide
    let mut pixels = vec![0x00; 80 * 64 * 4];
    // Fill first 64 pixels of each row with 0xFF
    for y in 0..64 {
        for x in 0..64 {
            let offset = (y * 80 + x) * 4;
            pixels[offset..offset+4].fill(0xFF);
        }
    }
    
    let hash = compute_rect_hash(&pixels, 64, 64, 80, 4);
    // Should only hash the first 64 pixels of each row
}
```

**Integration Tests** (`rfb-client/tests/persistent_cache.rs`):
```rust
#[tokio::test]
async fn test_cache_hit_flow() {
    let mut mock_server = MockVncServer::new();
    let mut client = VncClient::connect(mock_server.addr()).await?;
    
    // Server sends Init with hash H1
    mock_server.send_persistent_cached_rect_init(
        rect, hash_h1, ENCODING_TIGHT, payload
    ).await?;
    
    client.receive_update().await?;
    assert!(client.cache().has(&hash_h1));
    
    // Server sends Rect with same hash H1
    mock_server.send_persistent_cached_rect(rect, hash_h1).await?;
    
    client.receive_update().await?;
    // Should hit cache without querying
    assert_eq!(mock_server.received_queries().len(), 0);
}
```

**Cross-Session Test**:
```rust
#[tokio::test]
async fn test_cross_session_persistence() {
    let cache_file = temp_cache_file();
    
    // Session 1: Connect and populate cache
    {
        let mut client = VncClient::connect_with_cache(addr, &cache_file).await?;
        // Receive updates, populate cache
        client.receive_updates(100).await?;
        client.shutdown().await?;  // Triggers save
    }
    
    // Session 2: Reconnect with same cache
    {
        let mut client = VncClient::connect_with_cache(addr, &cache_file).await?;
        let stats = client.cache().stats();
        assert!(stats.entries > 0);  // Loaded from disk
        
        // Should see immediate hits for unchanged content
        client.receive_update().await?;
        assert!(stats.hits > 0);
    }
}
```

**Performance Benchmarks**:
```rust
#[bench]
fn bench_hash_computation(b: &mut Bencher) {
    let pixels = vec![0xFF; 800 * 600 * 4];
    b.iter(|| {
        compute_rect_hash(&pixels, 800, 600, 800, 4)
    });
    // Target: <1ms per 800x600 rect
}

#[bench]
fn bench_disk_save_load(b: &mut Bencher) {
    let cache = create_cache_with_entries(10000);
    b.iter(|| {
        cache.save_to_disk().unwrap();
        GlobalClientPersistentCache::load_from_disk().unwrap()
    });
    // Target: <200ms for 10K entries
}
```

**Manual QA** (with Xnjcvnc :2 per WARP.md safety rules):
1. Connect to test server, verify negotiation logs show `-321`
2. Observe ContentCache hits transitioning to PersistentCache hits
3. Restart client, verify cache loaded from disk
4. Confirm cross-session hits in logs
5. Check `~/.cache/tigervnc/persistentcache.dat` file size reasonable

### Dependencies Summary

**Add to relevant Cargo.toml files**:
```toml
[dependencies]
# Hashing
sha2 = "0.10"

# Binary I/O
byteorder = "1"

# Ordered maps for ARC
indexmap = "2"

# XDG cache directory
directories = "5"

# Optional: OpenSSL as alternative to sha2
openssl = { version = "0.10", optional = true, features = ["vendored"] }
```

### Success Criteria

- [ ] **Protocol negotiation**: Client sends `-321` and `-320`, server prefers PersistentCache
- [ ] **Hash computation**: Matches C++ ContentHash::computeRect exactly
- [ ] **ARC eviction**: Maintains configured size limits with proper promotions
- [ ] **Disk persistence**: Survives restarts without data loss or corruption
- [ ] **Cross-session hits**: Verified with test server (Xnjcvnc :2)
- [ ] **Performance**: Hash <1ms per rect, disk I/O <200ms for typical cache
- [ ] **Code quality**: Zero clippy warnings, comprehensive test coverage
- [ ] **Documentation**: Inline docs for all public APIs

### Timeline

| Task | Duration | Dependencies |
|------|----------|--------------|
| PC-1: Protocol constants | 0.5 day | - |
| PC-2: ContentHash utility | 0.5 day | PC-1 |
| PC-3: GlobalClientPersistentCache | 2 days | PC-1, PC-2 |
| PC-4: Client protocol messages | 1 day | PC-1 |
| PC-5: Decoders | 1 day | PC-3, PC-4 |
| PC-6: Client integration | 1 day | PC-5 |
| PC-7: Disk persistence | 1-2 days | PC-3 |
| PC-8: Testing & validation | 1 day | All tasks |

**Total Estimate**: 7-9 days for complete PersistentCache implementation.

---

**See Also**: [ROADMAP.md](ROADMAP.md), [CLI Usage](cli/USAGE.md), [Fullscreen & Multi-Monitor Spec](spec/fullscreen-and-multimonitor.md), [SEP-0001](SEP/SEP-0001-out-of-scope.md), [PersistentCache Rust Guide](protocol/PERSISTENTCACHE_RUST.md)
