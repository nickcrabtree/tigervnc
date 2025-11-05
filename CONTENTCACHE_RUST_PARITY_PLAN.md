# ContentCache Rust Viewer Parity Implementation Plan

**Document Version**: 1.0  
**Created**: November 5, 2025  
**Status**: Planning Phase  

---

## Executive Summary

This document provides a phased implementation plan to bring the Rust viewer's ContentCache implementation to full parity with the C++ viewer. Based on the analysis in `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md`, the Rust viewer is missing three critical features implemented in the C++ viewer during October 30 - November 5, 2025:

1. **ARC Eviction Protocol** - Client→server eviction notifications with bidirectional communication
2. **Bandwidth Tracking** - Comprehensive savings metrics and reporting
3. **Cross-Architecture Tests** - Automated verification of identical behavior between viewers

**Timeline Estimate**: 3-4 weeks  
**Complexity**: High (protocol changes, cache algorithm upgrade, test infrastructure)

---

## Table of Contents

1. [Gap Analysis](#gap-analysis)
2. [Phase 1: ARC Eviction Algorithm](#phase-1-arc-eviction-algorithm)
3. [Phase 2: Eviction Notification Protocol](#phase-2-eviction-notification-protocol)
4. [Phase 3: Bandwidth Tracking](#phase-3-bandwidth-tracking)
5. [Phase 4: Cross-Architecture Testing](#phase-4-cross-architecture-testing)
6. [Phase 5: Validation and Documentation](#phase-5-validation-and-documentation)
7. [Success Criteria](#success-criteria)
8. [Risk Assessment](#risk-assessment)

---

## Gap Analysis

### What Rust Viewer Currently Has ✅

From `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md` Part 2:

- ✅ **ContentCache protocol support** (CachedRect, CachedRectInit)
- ✅ **Cache miss recovery** (RequestCachedData protocol - msg 254)
- ✅ **LRU eviction** (simple least-recently-used)
- ✅ **Basic statistics** (hits, misses, memory usage)
- ✅ **All encoding fixes** (Tight filter bit, ZRLE stream state)
- ✅ **Canonical logging** (for e2e test parsing)

### What's Missing ❌

From `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md` Part 1 (C++ implementation):

#### 1. ARC Eviction Algorithm (Part 1.1)

**Current**: Simple LRU (Least Recently Used)  
**Needed**: ARC (Adaptive Replacement Cache) with:
- T1/T2 lists (recently/frequently used)
- B1/B2 ghost lists (adaptive tuning)
- Adaptive parameter `p` for balancing recency vs. frequency
- Proper promotion (T1→T2 on second access)

**Impact**: C++ viewer gets better hit rates with same cache size due to intelligent eviction

#### 2. Eviction Notification Protocol (Part 1.1, Phase 2-4)

**Current**: No server notification when evicting entries  
**Needed**:
- Client→server eviction notification messages (msg type 250)
- Batched eviction notifications on flush
- `CacheEviction` message type with array of evicted IDs

**Impact**: Server wastes bandwidth sending CachedRect references for entries the client has evicted

#### 3. Bandwidth Tracking (Part 1.2)

**Current**: No bandwidth savings metrics  
**Needed**:
- Track transmitted bytes for CachedRect (20 bytes)
- Track CachedRectInit bytes (24 + compressed data)
- Estimate alternative transmission (without cache)
- Calculate bandwidth savings and percentage reduction
- Report statistics on viewer exit

**Impact**: No visibility into real-world ContentCache effectiveness

### Files Requiring Changes

**Rust Viewer**:
```
rust-vnc-viewer/rfb-encodings/src/content_cache.rs    [Major changes - ARC algorithm]
rust-vnc-viewer/rfb-protocol/src/messages/client.rs   [Add CacheEviction message]
rust-vnc-viewer/rfb-protocol/src/messages/types.rs    [Add msgTypeCacheEviction constant]
rust-vnc-viewer/rfb-client/src/framebuffer.rs         [Batch evictions, bandwidth tracking]
rust-vnc-viewer/rfb-client/src/event_loop.rs          [Send eviction notifications]
rust-vnc-viewer/rfb-client/src/protocol.rs            [Bandwidth accounting]
```

**Testing**:
```
tests/e2e/test_cache_parity.py                        [New - cross-viewer comparison]
tests/e2e/log_parser.py                               [Enhanced - parse new stats]
tests/e2e/test_cache_eviction.py                      [Enhanced - verify Rust too]
```

---

## Phase 1: ARC Eviction Algorithm

**Duration**: 1-2 weeks  
**Complexity**: High  
**Dependencies**: None

### 1.1 Goals

Replace simple LRU with full ARC (Adaptive Replacement Cache) algorithm matching C++ implementation.

### 1.2 Background: ARC Algorithm

From C++ implementation (commits d6ed7029, 95a1d63c):

```
Client-Side ARC Structure:
┌─────────────────────────────────────┐
│ ContentCache                        │
│                                     │
│ Pixel Cache (client storage)       │
│   pixelCache_ (map by cache_id)    │
│   pixelT1_ (recently used once)     │
│   pixelT2_ (frequently used)        │
│   pixelB1_ (ghost: evicted from T1) │
│   pixelB2_ (ghost: evicted from T2) │
│   pixelP_ (adaptive parameter)      │
│   pendingEvictions_ (notify queue)  │
└─────────────────────────────────────┘
```

**Key Invariants**:
- `|T1| + |T2| ≤ max_cache_size` (actual cached entries)
- `|B1| + |B2| ≤ max_cache_size` (ghost entries, no data)
- `p` adjusts dynamically based on workload

**Operations**:
1. **Cache hit in T1**: Promote to T2 (second access = "frequent")
2. **Cache hit in T2**: Move to T2 head (already frequent)
3. **Cache miss, ghost hit in B1**: Insert to T2, increase `p` (favor recency)
4. **Cache miss, ghost hit in B2**: Insert to T2, decrease `p` (favor frequency)
5. **Cache miss, cold**: Insert to T1 (new = "recent")

### 1.3 Implementation Tasks

#### Task 1.1: Enhance CachedPixels struct

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

**Changes**:
```rust
// Current
pub struct CachedPixels {
    pub cache_id: u64,
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub last_used: Instant,
    pub created_at: Instant,
}

// Add bytes field for accurate tracking
pub struct CachedPixels {
    pub cache_id: u64,
    pub pixels: Vec<u8>,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub last_used: Instant,
    pub created_at: Instant,
    pub bytes: usize,  // NEW: Total byte size for ARC tracking
}

// Update memory_size() to use bytes field
impl CachedPixels {
    pub fn memory_size(&self) -> usize {
        self.bytes
    }
}
```

**Reference**: C++ commit d6ed7029 (Phase 1)

#### Task 1.2: Add ARC list tracking structures

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

**Add to ContentCache struct**:
```rust
use std::collections::{HashMap, LinkedList};

pub struct ContentCache {
    // Existing fields
    pixels: HashMap<u64, CachedPixels>,
    max_size_mb: usize,
    current_size_bytes: usize,
    // ... stats fields ...
    
    // NEW: ARC tracking structures
    // T1: Recently used once (recency)
    pixel_t1: LinkedList<u64>,
    
    // T2: Frequently used (frequency)
    pixel_t2: LinkedList<u64>,
    
    // B1: Ghost entries evicted from T1 (no pixel data, just IDs)
    pixel_b1: LinkedList<u64>,
    
    // B2: Ghost entries evicted from T2 (no pixel data, just IDs)
    pixel_b2: LinkedList<u64>,
    
    // Track which list each cache_id is in
    pixel_list_map: HashMap<u64, ListType>,
    
    // Adaptive parameter (target size for T1)
    pixel_p: usize,
    
    // Eviction notification queue
    pending_evictions: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListType {
    None,  // Not in any list
    T1,    // Recently used once
    T2,    // Frequently used
    B1,    // Ghost: evicted from T1
    B2,    // Ghost: evicted from T2
}
```

**Reference**: C++ commit 95a1d63c (Phase 3) - lines 74-82 in ContentCache.h

#### Task 1.3: Implement ARC helper methods

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

**Add methods** (matching C++ implementation):

```rust
impl ContentCache {
    /// Evict entries using ARC replacement algorithm
    /// Returns true if an entry was evicted, false if cache is empty
    fn replace_pixel_cache(&mut self) -> Result<bool> {
        // ARC replacement logic:
        // 1. If |T1| > max(1, p), evict LRU from T1 to B1
        // 2. Else evict LRU from T2 to B2
        // 3. Remove pixel data, keep ghost entry
        
        // Implementation follows C++ replacePixelCache()
        // See ContentCache.cxx lines ~450-520
    }
    
    /// Move cache_id from T1 to head of T2 (promotion on second access)
    fn move_pixel_to_t2(&mut self, cache_id: u64) {
        // Remove from T1, add to T2 head
        // Update pixel_list_map
    }
    
    /// Move cache_id within T2 to head (reuse of frequent entry)
    fn move_to_t2_head(&mut self, cache_id: u64) {
        // Move to head within T2
    }
    
    /// Evict from T1 to B1 ghost list
    fn move_pixel_to_b1(&mut self, cache_id: u64) {
        // Remove from T1
        // Remove pixel data from pixels map
        // Add to B1 (ghost entry)
        // Track in pending_evictions
    }
    
    /// Evict from T2 to B2 ghost list
    fn move_pixel_to_b2(&mut self, cache_id: u64) {
        // Remove from T2
        // Remove pixel data from pixels map
        // Add to B2 (ghost entry)
        // Track in pending_evictions
    }
    
    /// Remove cache_id from any ARC list
    fn remove_from_lists(&mut self, cache_id: u64) {
        // Remove from whichever list it's in
        // Update pixel_list_map
    }
    
    /// Get byte size of cache entry
    fn get_pixel_entry_size(&self, cache_id: u64) -> usize {
        if let Some(cached) = self.pixels.get(&cache_id) {
            cached.bytes
        } else {
            0
        }
    }
}
```

**Reference**: C++ commit 95a1d63c (Phase 3) - ContentCache.cxx lines ~400-600

#### Task 1.4: Refactor storeDecodedPixels() with ARC

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

**⚠️ CRITICAL BUGFIX (November 5, 2025 - commit 4bbb6621):**

When copying pixel data from the framebuffer via `getBuffer()`, the returned pointer is NOT contiguous memory.
It points to the first row of a rectangle within a larger framebuffer, with subsequent rows separated by `stride` bytes.

**DO NOT** copy as a single block:
```rust
// ❌ WRONG - causes segfault by reading past allocated memory
let data_size = height * stride * bytes_per_pixel;
unsafe { std::ptr::copy_nonoverlapping(pixels, dst, data_size); }
```

**MUST** copy row-by-row:
```rust
// ✅ CORRECT - respects stride between rows
let row_bytes = width * bytes_per_pixel;
let stride_bytes = stride * bytes_per_pixel;
for y in 0..height {
    let src_offset = y * stride_bytes;
    let dst_offset = y * stride_bytes;
    unsafe {
        std::ptr::copy_nonoverlapping(
            pixels.add(src_offset),
            dst.add(dst_offset),
            row_bytes
        );
    }
}
```

This bug caused SIGSEGV crashes in the C++ viewer at `ContentCache::storeDecodedPixels()` line 890.
See crash report: `njcvncviewer-2025-11-05-104759.ips`

**Replace current insert() logic**:

```rust
pub fn store_decoded_pixels(
    &mut self,
    cache_id: u64,
    pixels: Vec<u8>,
    format: PixelFormat,
    width: u32,
    height: u32,
    stride: usize,
) -> Result<()> {
    if cache_id == 0 {
        anyhow::bail!("Cache ID 0 is reserved");
    }
    
    let bytes = pixels.len() + std::mem::size_of::<CachedPixels>();
    
    // Check which list cache_id is currently in
    match self.pixel_list_map.get(&cache_id).copied() {
        Some(ListType::B1) => {
            // Ghost hit in B1: Favor recency
            // Increase p (adaptive parameter)
            let delta = std::cmp::max(
                self.pixel_b2.len() / self.pixel_b1.len(),
                1
            );
            self.pixel_p = std::cmp::min(
                self.pixel_p + delta,
                self.max_cache_size_pixels()
            );
            
            // Make room
            self.replace_pixel_cache()?;
            
            // Insert into T2 (promote from ghost)
            self.remove_from_lists(cache_id);
            self.pixel_t2.push_front(cache_id);
            self.pixel_list_map.insert(cache_id, ListType::T2);
            
            // Increment hit counter (ghost hit is still a "cache hit")
            self.hit_count += 1;
        },
        
        Some(ListType::B2) => {
            // Ghost hit in B2: Favor frequency
            // Decrease p
            let delta = std::cmp::max(
                self.pixel_b1.len() / self.pixel_b2.len(),
                1
            );
            self.pixel_p = self.pixel_p.saturating_sub(delta);
            
            // Make room
            self.replace_pixel_cache()?;
            
            // Insert into T2 (promote from ghost)
            self.remove_from_lists(cache_id);
            self.pixel_t2.push_front(cache_id);
            self.pixel_list_map.insert(cache_id, ListType::T2);
            
            // Increment hit counter
            self.hit_count += 1;
        },
        
        Some(ListType::T1) | Some(ListType::T2) => {
            // Already cached - update in place
            // This shouldn't normally happen (CachedRectInit for existing entry)
            // But handle gracefully
            self.remove_from_lists(cache_id);
            self.pixel_t2.push_front(cache_id);
            self.pixel_list_map.insert(cache_id, ListType::T2);
            
            // Increment hit counter
            self.hit_count += 1;
        },
        
        None => {
            // Cold miss: Insert into T1 (new entry)
            // Make room if needed
            while self.current_size_bytes + bytes > self.max_size_bytes() {
                if !self.replace_pixel_cache()? {
                    break; // Cache empty
                }
            }
            
            // Insert into T1
            self.pixel_t1.push_front(cache_id);
            self.pixel_list_map.insert(cache_id, ListType::T1);
            
            // Increment miss counter
            self.miss_count += 1;
        }
    }
    
    // Store actual pixel data
    let cached = CachedPixels {
        cache_id,
        pixels,
        format,
        width,
        height,
        stride,
        last_used: Instant::now(),
        created_at: Instant::now(),
        bytes,
    };
    
    // Update accounting
    if let Some(old) = self.pixels.insert(cache_id, cached) {
        self.current_size_bytes -= old.bytes;
    }
    self.current_size_bytes += bytes;
    
    Ok(())
}
```

**Reference**: C++ commit 95a1d63c - ContentCache.cxx `storeDecodedPixels()` lines ~200-350

#### Task 1.5: Refactor lookup() with ARC promotion

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

**Update lookup to handle T1→T2 promotion**:

```rust
pub fn lookup(&mut self, cache_id: u64) -> Option<&CachedPixels> {
    if let Some(cached) = self.pixels.get_mut(&cache_id) {
        // Cache hit: update access time
        cached.touch();
        
        // ARC promotion logic
        match self.pixel_list_map.get(&cache_id).copied() {
            Some(ListType::T1) => {
                // First hit in T1: Promote to T2 (second access = "frequent")
                self.move_pixel_to_t2(cache_id);
            },
            Some(ListType::T2) => {
                // Already in T2: Move to head
                self.move_to_t2_head(cache_id);
            },
            _ => {
                // Shouldn't happen - entry has pixels but not in T1/T2
                eprintln!("WARNING: Cache entry {} has pixels but not in T1/T2", cache_id);
            }
        }
        
        // Update statistics
        self.hit_count += 1;
        self.bytes_saved += cached.pixels.len() as u64;
        
        Some(cached)
    } else {
        // Cache miss
        self.miss_count += 1;
        None
    }
}
```

**Reference**: C++ commit 95a1d63c - ContentCache.cxx `getDecodedPixels()` lines ~350-380

#### Task 1.6: Add getPendingEvictions() method

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

```rust
impl ContentCache {
    /// Get vector of cache IDs evicted since last call
    /// Clears the pending evictions list
    pub fn get_pending_evictions(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.pending_evictions)
    }
    
    /// Check if there are pending evictions to send
    pub fn has_pending_evictions(&self) -> bool {
        !self.pending_evictions.is_empty()
    }
}
```

**Reference**: C++ ContentCache.h lines 175-178

#### Task 1.7: Update stats() to include ARC metrics

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

```rust
#[derive(Debug, Clone)]
pub struct CacheStats {
    // Existing fields
    pub entries: usize,
    pub size_mb: usize,
    pub max_size_mb: usize,
    pub hit_rate: f64,
    pub hit_count: u64,
    pub miss_count: u64,
    pub eviction_count: u64,
    pub bytes_saved: u64,
    pub avg_entry_size: usize,
    
    // NEW: ARC-specific stats
    pub t1_size: usize,        // Number in T1 (recently used once)
    pub t2_size: usize,        // Number in T2 (frequently used)
    pub b1_size: usize,        // Number in B1 (ghosts from T1)
    pub b2_size: usize,        // Number in B2 (ghosts from T2)
    pub target_t1_size: usize, // Adaptive parameter p
}

impl ContentCache {
    pub fn stats(&self) -> CacheStats {
        // ... existing calculation ...
        
        CacheStats {
            // ... existing fields ...
            
            // ARC stats
            t1_size: self.pixel_t1.len(),
            t2_size: self.pixel_t2.len(),
            b1_size: self.pixel_b1.len(),
            b2_size: self.pixel_b2.len(),
            target_t1_size: self.pixel_p,
        }
    }
}
```

**Reference**: C++ ContentCache.h Stats struct lines 126-139

#### Task 1.8: Add unit tests for ARC

**File**: `rust-vnc-viewer/rfb-encodings/src/content_cache.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_arc_promotion_t1_to_t2() {
        // Insert entry (goes to T1)
        // Lookup once (stays in T1)
        // Lookup twice (promotes to T2)
        // Verify t1_size decreases, t2_size increases
    }
    
    #[test]
    fn test_arc_ghost_hit_b1() {
        // Fill cache to capacity
        // Evict entry (moves to B1)
        // Re-insert same cache_id (ghost hit in B1)
        // Verify goes to T2, p increases
    }
    
    #[test]
    fn test_arc_ghost_hit_b2() {
        // Fill cache, promote entries to T2
        // Evict from T2 (moves to B2)
        // Re-insert same cache_id (ghost hit in B2)
        // Verify goes to T2, p decreases
    }
    
    #[test]
    fn test_arc_eviction_sends_notification() {
        // Fill cache to capacity
        // Insert new entry (forces eviction)
        // Verify pending_evictions contains evicted ID
    }
    
    #[test]
    fn test_arc_replace_prefers_t1_when_over_p() {
        // Set up: T1 size > p, T2 has entries
        // Trigger replacement
        // Verify evicts from T1
    }
    
    #[test]
    fn test_arc_replace_prefers_t2_when_under_p() {
        // Set up: T1 size <= p, T2 has entries
        // Trigger replacement
        // Verify evicts from T2
    }
}
```

**Reference**: Low priority - C++ implementation lacks dedicated ARC unit tests (relies on e2e)

### 1.4 Validation

**Manual Testing**:
```bash
# Build Rust viewer with ARC changes
cd rust-vnc-viewer
cargo build --release

# Run with verbose logging
cargo run --release -- -vv localhost:999 2>&1 | tee /tmp/rust_arc_test.log

# Verify logs show ARC promotion:
grep "Promoted.*T1.*T2" /tmp/rust_arc_test.log
grep "Ghost hit in B1" /tmp/rust_arc_test.log
```

**Expected behaviors**:
- First access to new content: "Stored in T1"
- Second access: "Promoted T1→T2"
- Ghost hits: "Ghost hit in B1/B2, inserted to T2"
- Stats show t1_size, t2_size changing dynamically

---

## Phase 2: Eviction Notification Protocol

**Duration**: 3-5 days  
**Complexity**: Medium  
**Dependencies**: Phase 1 complete

### 2.1 Goals

Implement client→server eviction notifications so the server knows which cache entries the client no longer has.

### 2.2 Protocol Specification

From C++ implementation (commit d019b7d9):

**Message Type**: `msgTypeCacheEviction = 250`  
**Encoding Constant**: `encodingCacheEviction = 104`

**Wire Format**:
```
U8:  message_type (250)
U8:  padding
U16: padding
U32: count (number of evicted cache IDs)
U64[]: cache_ids (array of count evicted IDs)
```

**Size**: 8 bytes + (count × 8 bytes)

### 2.3 Implementation Tasks

#### Task 2.1: Add protocol constants

**File**: `rust-vnc-viewer/rfb-protocol/src/messages/types.rs`

```rust
// Add to encoding constants
pub const ENCODING_CACHE_EVICTION: i32 = 104;

// Add to message type constants
pub const MSG_TYPE_CACHE_EVICTION: u8 = 250;
```

**Reference**: C++ encodings.h line ~60, msgTypes.h line ~50

#### Task 2.2: Implement CacheEviction message

**File**: `rust-vnc-viewer/rfb-protocol/src/messages/client.rs`

```rust
/// CacheEviction - Notify server of evicted cache entries
/// 
/// Sent by client when ContentCache evicts entries due to memory limits.
/// Allows server to avoid sending CachedRect references for entries
/// that the client no longer has.
///
/// # Wire Format
///
/// - U8: message type (250)
/// - U8: padding
/// - U16: padding
/// - U32: count (number of evicted cache IDs)
/// - U64[]: cache_ids (array of evicted IDs)
#[derive(Debug, Clone)]
pub struct CacheEviction {
    pub cache_ids: Vec<u64>,
}

impl CacheEviction {
    pub fn new(cache_ids: Vec<u64>) -> Self {
        Self { cache_ids }
    }
    
    /// Write CacheEviction message to stream
    pub fn write_to<W: AsyncWrite + Unpin>(
        &self,
        stream: &mut RfbOutStream<W>,
    ) -> std::io::Result<()> {
        // Message type
        stream.write_u8(MSG_TYPE_CACHE_EVICTION);
        
        // Padding
        stream.write_u8(0);
        stream.write_u16(0);
        
        // Count
        stream.write_u32(self.cache_ids.len() as u32);
        
        // Cache IDs
        for cache_id in &self.cache_ids {
            stream.write_u64(*cache_id);
        }
        
        Ok(())
    }
}
```

**Reference**: C++ CMsgWriter.cxx `writeCacheEviction()` lines ~450-470

#### Task 2.3: Add write_cache_eviction() helper

**File**: `rust-vnc-viewer/rfb-client/src/protocol.rs`

```rust
impl RfbClient {
    /// Send CacheEviction notification to server
    pub async fn write_cache_eviction(&mut self, cache_ids: Vec<u64>) -> Result<()> {
        if cache_ids.is_empty() {
            return Ok(()); // Nothing to send
        }
        
        debug!("Sending CacheEviction with {} IDs", cache_ids.len());
        
        let msg = CacheEviction::new(cache_ids);
        msg.write_to(&mut self.stream).await?;
        self.stream.flush().await?;
        
        Ok(())
    }
}
```

#### Task 2.4: Send eviction notifications in event loop

**File**: `rust-vnc-viewer/rfb-client/src/framebuffer.rs`

```rust
impl Framebuffer {
    /// Get pending cache evictions and clear the queue
    pub fn take_pending_evictions(&mut self) -> Vec<u64> {
        if let Some(cache) = self.content_cache.as_mut() {
            cache.get_pending_evictions()
        } else {
            Vec::new()
        }
    }
}
```

**File**: `rust-vnc-viewer/rfb-client/src/event_loop.rs`

```rust
// In handle_server_message() after processing FramebufferUpdate:

// Send cache eviction notifications if any
if let Some(fb) = framebuffer.as_mut() {
    let evictions = fb.take_pending_evictions();
    if !evictions.is_empty() {
        debug!("Sending {} cache evictions to server", evictions.len());
        client.write_cache_eviction(evictions).await?;
    }
}
```

**Reference**: C++ DecodeManager.cxx `flush()` lines ~180-195

#### Task 2.5: Add integration test

**File**: `tests/e2e/test_cache_eviction.py`

**Enhance existing test to verify Rust viewer**:

```python
def test_rust_viewer_eviction_notification():
    """Test that Rust viewer sends eviction notifications"""
    
    # Start server with small cache (16MB) on :998
    server = start_test_server(
        display=998,
        cache_size_mb=16,
        log_file="/tmp/server_evict_rust.log"
    )
    
    # Start Rust viewer with small cache
    rust_viewer = subprocess.Popen([
        "cargo", "run", "--release", "--",
        "--cache-size=16",
        "localhost:998"
    ], env={"RUST_LOG": "debug"})
    
    # Drive server to generate content exceeding cache size
    # (pyautogui to move windows, type text, etc.)
    
    time.sleep(30)
    
    # Check server log for eviction notifications received
    server_log = read_log("/tmp/server_evict_rust.log")
    assert "Received CacheEviction" in server_log
    assert "cache IDs" in server_log
    
    # Verify server stopped sending CachedRect for evicted IDs
    # (would require parsing protocol or checking hit rates)
```

### 2.4 Validation

**Protocol verification**:
```bash
# Capture network traffic
sudo tcpdump -i lo0 -w /tmp/vnc_evict.pcap port 5998

# Start Rust viewer with small cache
cargo run --release -- --cache-size=16 localhost:998

# Later: Parse pcap for message type 250
tshark -r /tmp/vnc_evict.pcap -Y "vnc.client_message_type == 250"
```

**Server-side verification** (check C++ server logs):
```bash
# Server should log:
# "Client sent CacheEviction with N IDs"
# "Removed cache ID <id> from knownCacheIds_"
```

---

## Phase 3: Bandwidth Tracking

**Duration**: 3-5 days  
**Complexity**: Low-Medium  
**Dependencies**: None (can be done in parallel with Phase 1-2)

### 3.1 Goals

Track actual transmitted bytes vs. estimated bytes without ContentCache, report savings statistics on viewer exit.

### 3.2 Implementation Tasks

#### Task 3.1: Add bandwidth tracking structures

**File**: `rust-vnc-viewer/rfb-client/src/framebuffer.rs`

```rust
/// Bandwidth savings tracking for ContentCache
#[derive(Debug, Default, Clone)]
pub struct ContentCacheBandwidthStats {
    /// Bytes transmitted for CachedRect (20 bytes per reference)
    pub cached_rect_bytes: u64,
    
    /// Bytes transmitted for CachedRectInit (24 + compressed data)
    pub cached_rect_init_bytes: u64,
    
    /// Estimated bytes that would have been sent without cache
    /// (16 bytes header + compressed data)
    pub alternative_bytes: u64,
    
    /// Number of CachedRect references received
    pub cached_rect_count: u32,
    
    /// Number of CachedRectInit messages received
    pub cached_rect_init_count: u32,
}

impl ContentCacheBandwidthStats {
    /// Calculate total bandwidth saved
    pub fn bandwidth_saved(&self) -> u64 {
        self.alternative_bytes.saturating_sub(
            self.cached_rect_bytes + self.cached_rect_init_bytes
        )
    }
    
    /// Calculate percentage reduction (0.0 to 100.0)
    pub fn reduction_percentage(&self) -> f64 {
        if self.alternative_bytes == 0 {
            0.0
        } else {
            (self.bandwidth_saved() as f64 / self.alternative_bytes as f64) * 100.0
        }
    }
}

// Add to Framebuffer struct
pub struct Framebuffer {
    // ... existing fields ...
    
    // NEW: Bandwidth tracking
    content_cache_bandwidth_stats: ContentCacheBandwidthStats,
    last_decoded_rect_bytes: usize,  // Track compressed bytes from last decode
}
```

**Reference**: C++ DecodeManager.h lines 145-154

#### Task 3.2: Track CachedRect bandwidth

**File**: `rust-vnc-viewer/rfb-encodings/src/cached_rect.rs`

**Update decode_cached_rect() to track bandwidth**:

```rust
pub async fn decode_cached_rect<R: AsyncRead + Unpin>(
    stream: &mut RfbInStream<R>,
    rect: Rectangle,
    framebuffer: &mut Framebuffer,
    cache: &mut ContentCache,
    miss_reporter: &mut impl CacheMissReporter,
) -> Result<()> {
    // Read cache_id (8 bytes)
    let cache_id = stream.read_u64().await?;
    
    // Track bandwidth: 20 bytes total (12 header + 8 cache_id)
    framebuffer.track_cached_rect_bandwidth(&rect);
    
    // ... rest of existing logic ...
}
```

**Add to Framebuffer**:

```rust
impl Framebuffer {
    /// Track bandwidth for CachedRect reference
    pub fn track_cached_rect_bandwidth(&mut self, rect: &Rectangle) {
        let stats = &mut self.content_cache_bandwidth_stats;
        
        // Actual transmitted: 12 byte rect header + 8 byte cache_id = 20 bytes
        stats.cached_rect_bytes += 20;
        stats.cached_rect_count += 1;
        
        // Estimate alternative: 12 byte header + 16 byte rect header + compressed data
        // Conservative estimate: 10:1 compression ratio for typical desktop content
        let rect_pixels = (rect.width * rect.height) as u64;
        let bytes_per_pixel = 4; // RGBA
        let uncompressed = rect_pixels * bytes_per_pixel;
        let estimated_compressed = uncompressed / 10; // 10:1 compression
        stats.alternative_bytes += 16 + estimated_compressed;
    }
}
```

**Reference**: C++ DecodeManager.cxx `trackCachedRectBandwidth()` lines ~120-135

#### Task 3.3: Track CachedRectInit bandwidth

**File**: `rust-vnc-viewer/rfb-encodings/src/cached_rect_init.rs`

**Update decode_cached_rect_init() to track actual compressed bytes**:

```rust
pub async fn decode_cached_rect_init<R: AsyncRead + Unpin>(
    stream: &mut RfbInStream<R>,
    rect: Rectangle,
    framebuffer: &mut Framebuffer,
    cache: &mut ContentCache,
) -> Result<()> {
    // Read cache_id and actual_encoding (12 bytes)
    let cache_id = stream.read_u64().await?;
    let actual_encoding = stream.read_i32().await?;
    
    // Track starting position
    let start_pos = stream.position();
    
    // Decode based on actual_encoding
    match actual_encoding {
        ENCODING_TIGHT => {
            decode_tight(stream, &rect, framebuffer).await?;
        },
        ENCODING_ZRLE => {
            decode_zrle(stream, &rect, framebuffer).await?;
        },
        // ... other encodings ...
    }
    
    // Track ending position
    let end_pos = stream.position();
    let compressed_bytes = (end_pos - start_pos) as usize;
    
    // Track bandwidth: 24 bytes header + compressed data
    framebuffer.track_cached_rect_init_bandwidth(&rect, compressed_bytes);
    
    // Store decoded pixels in cache
    // ... existing storage logic ...
}
```

**Add to Framebuffer**:

```rust
impl Framebuffer {
    /// Track bandwidth for CachedRectInit
    pub fn track_cached_rect_init_bandwidth(&mut self, rect: &Rectangle, compressed_bytes: usize) {
        let stats = &mut self.content_cache_bandwidth_stats;
        
        // Actual transmitted: 12 rect header + 12 CachedRectInit header + compressed data
        stats.cached_rect_init_bytes += 24 + compressed_bytes as u64;
        stats.cached_rect_init_count += 1;
        
        // Alternative: 12 rect header + 4 encoding + compressed data
        stats.alternative_bytes += 16 + compressed_bytes as u64;
    }
}
```

**Reference**: C++ DecodeManager.cxx `trackCachedRectInitBandwidth()` lines ~135-150

#### Task 3.4: Report statistics on exit

**File**: `rust-vnc-viewer/rfb-client/src/event_loop.rs`

**Add to cleanup/shutdown logic**:

```rust
impl RfbClient {
    pub fn log_contentcache_bandwidth_stats(&self, framebuffer: &Framebuffer) {
        let stats = &framebuffer.content_cache_bandwidth_stats;
        
        if stats.cached_rect_count == 0 && stats.cached_rect_init_count == 0 {
            return; // No ContentCache activity
        }
        
        let saved = stats.bandwidth_saved();
        let reduction = stats.reduction_percentage();
        
        // Format saved bytes with IEC prefixes (KiB, MiB, GiB)
        let saved_str = format_iec_bytes(saved);
        
        info!("ContentCache: {} bandwidth saving ({:.1}% reduction)", 
              saved_str, reduction);
        
        // Detailed stats (debug level)
        debug!("ContentCache: === Bandwidth Statistics ===");
        debug!("ContentCache: CachedRect count: {} (total {} bytes)",
               stats.cached_rect_count, stats.cached_rect_bytes);
        debug!("ContentCache: CachedRectInit count: {} (total {} bytes)",
               stats.cached_rect_init_count, stats.cached_rect_init_bytes);
        debug!("ContentCache: Estimated without cache: {}",
               format_iec_bytes(stats.alternative_bytes));
        debug!("ContentCache: Actual with cache: {}",
               format_iec_bytes(stats.cached_rect_bytes + stats.cached_rect_init_bytes));
    }
}

// Helper function to format bytes with IEC prefixes
fn format_iec_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit_idx])
    }
}
```

**Call on viewer shutdown**:

```rust
// In main event loop cleanup:
client.log_contentcache_bandwidth_stats(&framebuffer);
```

**Reference**: C++ DecodeManager.cxx `logStats()` lines ~90-120, commits c9d5fa1d, b1a680c0

#### Task 3.5: Add unit tests

**File**: `rust-vnc-viewer/rfb-client/tests/bandwidth_stats_tests.rs` (new)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bandwidth_stats_calculation() {
        let mut stats = ContentCacheBandwidthStats::default();
        
        // Simulate receiving 10 CachedRect references (20 bytes each)
        stats.cached_rect_bytes = 200;
        stats.cached_rect_count = 10;
        
        // Each would have been ~5KB without cache
        stats.alternative_bytes = 50_000;
        
        // Bandwidth saved: 50,000 - 200 = 49,800 bytes
        assert_eq!(stats.bandwidth_saved(), 49_800);
        
        // Reduction: (49,800 / 50,000) * 100 = 99.6%
        assert!((stats.reduction_percentage() - 99.6).abs() < 0.1);
    }
    
    #[test]
    fn test_format_iec_bytes() {
        assert_eq!(format_iec_bytes(1023), "1023 B");
        assert_eq!(format_iec_bytes(1024), "1.0 KiB");
        assert_eq!(format_iec_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_iec_bytes(4_700_000), "4.5 MiB");
    }
}
```

### 3.3 Validation

**Expected output** on viewer exit:

```
INFO  ContentCache: 4.7 MiB bandwidth saving (90.7% reduction)
DEBUG ContentCache: === Bandwidth Statistics ===
DEBUG ContentCache: CachedRect count: 1234 (total 24680 bytes)
DEBUG ContentCache: CachedRectInit count: 567 (total 456789 bytes)
DEBUG ContentCache: Estimated without cache: 5.2 MiB
DEBUG ContentCache: Actual with cache: 481 KiB
```

**Comparison with C++ viewer**:
- Run same test scenario with both viewers
- Bandwidth savings should be within 5% (minor differences due to timing)

---

## Phase 4: Cross-Architecture Testing

**Duration**: 1 week  
**Complexity**: Medium-High  
**Dependencies**: Phases 1-3 complete

### 4.1 Goals

Create automated tests that compare Rust and C++ viewers running identical workloads to ensure:
- Identical cache hit statistics
- Identical bandwidth savings
- Consistent eviction behavior
- Correct ARC algorithm implementation

### 4.2 Test Architecture

```
┌─────────────────────────────────────────────────┐
│ test_cache_parity.py (test orchestrator)       │
├─────────────────────────────────────────────────┤
│                                                  │
│  ┌──────────────┐         ┌──────────────┐      │
│  │ Server :998  │         │ Server :999  │      │
│  │ (C++ test)   │         │ (Rust test)  │      │
│  └──────┬───────┘         └──────┬───────┘      │
│         │                        │              │
│         ▼                        ▼              │
│  ┌──────────────┐         ┌──────────────┐      │
│  │ C++ viewer   │         │ Rust viewer  │      │
│  │ njcvncviewer │         │njcvncviewer-rs│     │
│  └──────┬───────┘         └──────┬───────┘      │
│         │                        │              │
│         ▼                        ▼              │
│  ┌──────────────────────────────────────┐      │
│  │   Identical automation script        │      │
│  │   (xdotool, pyautogui, etc.)         │      │
│  │   - Open windows                      │      │
│  │   - Type text                         │      │
│  │   - Move/resize windows               │      │
│  │   - Scroll content                    │      │
│  └──────────────────────────────────────┘      │
│                                                  │
│  ┌──────────────────────────────────────┐      │
│  │   Log parser & comparator            │      │
│  │   - Parse both viewer logs           │      │
│  │   - Extract ContentCache stats       │      │
│  │   - Compare hit rates                │      │
│  │   - Compare bandwidth savings        │      │
│  │   - Verify eviction counts           │      │
│  └──────────────────────────────────────┘      │
└─────────────────────────────────────────────────┘
```

### 4.3 Implementation Tasks

#### Task 4.1: Enhance log parser for stats extraction

**File**: `tests/e2e/log_parser.py`

**Add parsing for ContentCache statistics**:

```python
class ContentCacheStats:
    """Parsed ContentCache statistics from viewer log"""
    
    def __init__(self):
        self.hit_count = 0
        self.miss_count = 0
        self.eviction_count = 0
        self.bandwidth_saved_bytes = 0
        self.reduction_percentage = 0.0
        
        # ARC-specific stats
        self.t1_size = 0
        self.t2_size = 0
        self.b1_size = 0
        self.b2_size = 0
        self.target_t1_size = 0
        
        # Bandwidth details
        self.cached_rect_count = 0
        self.cached_rect_init_count = 0
        self.cached_rect_bytes = 0
        self.cached_rect_init_bytes = 0
        self.alternative_bytes = 0
    
    @property
    def hit_rate(self):
        total = self.hit_count + self.miss_count
        if total == 0:
            return 0.0
        return self.hit_count / total

def parse_contentcache_stats(log_file: str) -> ContentCacheStats:
    """Parse ContentCache statistics from viewer log"""
    
    stats = ContentCacheStats()
    
    with open(log_file, 'r') as f:
        for line in f:
            # Parse C++ format:
            # "ContentCache: Hit rate: 84.2% (1234 hits, 234 misses)"
            if "Hit rate:" in line:
                match = re.search(r'(\d+\.?\d*?)% \((\d+) hits, (\d+) misses\)', line)
                if match:
                    stats.hit_count = int(match.group(2))
                    stats.miss_count = int(match.group(3))
            
            # Parse bandwidth savings:
            # "ContentCache: 4.7 MiB bandwidth saving (90.7% reduction)"
            if "bandwidth saving" in line:
                match = re.search(r'([\d.]+) (\w+) bandwidth saving \(([\d.]+)% reduction\)', line)
                if match:
                    value = float(match.group(1))
                    unit = match.group(2)
                    stats.reduction_percentage = float(match.group(3))
                    
                    # Convert to bytes
                    multiplier = {
                        'B': 1,
                        'KiB': 1024,
                        'MiB': 1024**2,
                        'GiB': 1024**3,
                    }.get(unit, 1)
                    stats.bandwidth_saved_bytes = int(value * multiplier)
            
            # Parse ARC stats (if logged):
            # "ContentCache: ARC: T1=45 T2=123 B1=12 B2=8 p=67"
            if "ARC:" in line:
                match = re.search(r'T1=(\d+) T2=(\d+) B1=(\d+) B2=(\d+) p=(\d+)', line)
                if match:
                    stats.t1_size = int(match.group(1))
                    stats.t2_size = int(match.group(2))
                    stats.b1_size = int(match.group(3))
                    stats.b2_size = int(match.group(4))
                    stats.target_t1_size = int(match.group(5))
            
            # Parse Rust format (may be slightly different):
            # Adapt as needed based on actual Rust logging format
    
    return stats
```

#### Task 4.2: Create parity test script

**File**: `tests/e2e/test_cache_parity.py` (new)

```python
#!/usr/bin/env python3
"""
Cross-architecture ContentCache parity test.

Runs identical workload against C++ and Rust viewers to verify:
- Identical cache hit rates (within 2%)
- Identical bandwidth savings (within 5%)
- Correct eviction behavior
- ARC algorithm parity
"""

import subprocess
import time
import sys
from pathlib import Path
from typing import Tuple

from log_parser import parse_contentcache_stats, ContentCacheStats

# Test parameters
DURATION_SECONDS = 60
CACHE_SIZE_MB = 128
CPP_DISPLAY = 998
RUST_DISPLAY = 999

def start_test_server(display: int, cache_size_mb: int, log_file: str) -> subprocess.Popen:
    """Start test server on specified display"""
    
    # Build path to test server binary
    server_bin = Path(__file__).parent.parent.parent / "build" / "unix" / "vncserver" / "Xnjcvnc"
    
    cmd = [
        str(server_bin),
        f":{display}",
        "-geometry", "1280x1024",
        "-depth", "24",
        "-SecurityTypes", "None",
        f"-ContentCacheSize={cache_size_mb}",
        "-EnableContentCache=1",
        "-rfbport", str(5900 + display),
    ]
    
    env = os.environ.copy()
    env["XVNC_LOG"] = log_file
    
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    
    # Wait for server to be ready
    time.sleep(2)
    
    return proc

def start_cpp_viewer(display: int, log_file: str) -> subprocess.Popen:
    """Start C++ viewer connecting to display"""
    
    viewer_bin = Path(__file__).parent.parent.parent / "build" / "vncviewer" / "njcvncviewer"
    
    cmd = [
        str(viewer_bin),
        f"-ContentCacheSize={CACHE_SIZE_MB}",
        "-Log=*:stderr:100",
        f"localhost:{display}",
    ]
    
    with open(log_file, 'w') as log:
        proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    
    time.sleep(2)
    return proc

def start_rust_viewer(display: int, log_file: str) -> subprocess.Popen:
    """Start Rust viewer connecting to display"""
    
    cmd = [
        "cargo", "run", "--release", "--",
        f"--cache-size={CACHE_SIZE_MB}",
        "-vv",
        f"localhost:{display}",
    ]
    
    env = os.environ.copy()
    env["RUST_LOG"] = "debug"
    
    with open(log_file, 'w') as log:
        proc = subprocess.Popen(
            cmd,
            cwd=Path(__file__).parent.parent.parent / "rust-vnc-viewer",
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT
        )
    
    time.sleep(2)
    return proc

def run_identical_automation(cpp_display: int, rust_display: int, duration: int):
    """
    Run identical automation workload on both displays.
    
    This is the key to ensuring fair comparison:
    - Both viewers see exactly the same screen content
    - Same timing for updates
    - Same user interactions
    """
    
    print(f"Running {duration}s automation on displays :{cpp_display} and :{rust_display}...")
    
    # Example automation (adapt to your test environment):
    import pyautogui
    
    start_time = time.time()
    
    while time.time() - start_time < duration:
        # Open a terminal window on both displays
        for display in [cpp_display, rust_display]:
            os.environ["DISPLAY"] = f":{display}"
            subprocess.run(["xterm", "-e", "ls -la"], timeout=1)
        
        time.sleep(2)
        
        # Type text
        pyautogui.typewrite("Hello World\n", interval=0.1)
        time.sleep(1)
        
        # Move windows around (generates many CachedRect references)
        for _ in range(10):
            pyautogui.moveRel(10, 10, duration=0.1)
            time.sleep(0.1)
        
        time.sleep(1)

def compare_stats(cpp_stats: ContentCacheStats, rust_stats: ContentCacheStats) -> bool:
    """
    Compare ContentCache stats between C++ and Rust viewers.
    
    Returns True if within acceptable tolerances.
    """
    
    print("\n" + "="*60)
    print("ContentCache Statistics Comparison")
    print("="*60)
    
    print(f"\n{'Metric':<30} {'C++ Viewer':<20} {'Rust Viewer':<20} {'Diff':<10}")
    print("-" * 80)
    
    # Hit counts
    print(f"{'Hit count':<30} {cpp_stats.hit_count:<20} {rust_stats.hit_count:<20} "
          f"{abs(cpp_stats.hit_count - rust_stats.hit_count):<10}")
    
    # Miss counts
    print(f"{'Miss count':<30} {cpp_stats.miss_count:<20} {rust_stats.miss_count:<20} "
          f"{abs(cpp_stats.miss_count - rust_stats.miss_count):<10}")
    
    # Hit rates
    cpp_hit_rate = cpp_stats.hit_rate * 100
    rust_hit_rate = rust_stats.hit_rate * 100
    hit_rate_diff = abs(cpp_hit_rate - rust_hit_rate)
    print(f"{'Hit rate (%)':<30} {cpp_hit_rate:<20.1f} {rust_hit_rate:<20.1f} "
          f"{hit_rate_diff:<10.1f}")
    
    # Bandwidth savings
    cpp_saved_mb = cpp_stats.bandwidth_saved_bytes / (1024**2)
    rust_saved_mb = rust_stats.bandwidth_saved_bytes / (1024**2)
    saved_diff_percent = abs(cpp_saved_mb - rust_saved_mb) / max(cpp_saved_mb, 1) * 100
    print(f"{'Bandwidth saved (MiB)':<30} {cpp_saved_mb:<20.1f} {rust_saved_mb:<20.1f} "
          f"{saved_diff_percent:<10.1f}%")
    
    # Reduction percentages
    reduction_diff = abs(cpp_stats.reduction_percentage - rust_stats.reduction_percentage)
    print(f"{'Reduction (%)':<30} {cpp_stats.reduction_percentage:<20.1f} "
          f"{rust_stats.reduction_percentage:<20.1f} {reduction_diff:<10.1f}")
    
    # ARC list sizes (if available)
    if cpp_stats.t1_size > 0 or rust_stats.t1_size > 0:
        print(f"\n{'ARC List Sizes':<30}")
        print(f"{'T1 (recently used once)':<30} {cpp_stats.t1_size:<20} {rust_stats.t1_size:<20}")
        print(f"{'T2 (frequently used)':<30} {cpp_stats.t2_size:<20} {rust_stats.t2_size:<20}")
        print(f"{'B1 (ghosts from T1)':<30} {cpp_stats.b1_size:<20} {rust_stats.b1_size:<20}")
        print(f"{'B2 (ghosts from T2)':<30} {cpp_stats.b2_size:<20} {rust_stats.b2_size:<20}")
        print(f"{'p (adaptive parameter)':<30} {cpp_stats.target_t1_size:<20} "
              f"{rust_stats.target_t1_size:<20}")
    
    print("\n" + "="*60)
    
    # Validation checks
    passed = True
    
    # Hit rate should be within 2%
    if hit_rate_diff > 2.0:
        print(f"❌ FAIL: Hit rate difference {hit_rate_diff:.1f}% exceeds 2% tolerance")
        passed = False
    else:
        print(f"✅ PASS: Hit rate within 2% tolerance")
    
    # Bandwidth savings should be within 5%
    if saved_diff_percent > 5.0:
        print(f"❌ FAIL: Bandwidth saved difference {saved_diff_percent:.1f}% exceeds 5% tolerance")
        passed = False
    else:
        print(f"✅ PASS: Bandwidth savings within 5% tolerance")
    
    return passed

def main():
    """Run parity test"""
    
    print("ContentCache Cross-Architecture Parity Test")
    print("=" * 60)
    
    # Start servers
    print("\nStarting test servers...")
    cpp_server_log = "/tmp/cpp_server_parity.log"
    rust_server_log = "/tmp/rust_server_parity.log"
    
    cpp_server = start_test_server(CPP_DISPLAY, CACHE_SIZE_MB, cpp_server_log)
    rust_server = start_test_server(RUST_DISPLAY, CACHE_SIZE_MB, rust_server_log)
    
    # Start viewers
    print("Starting viewers...")
    cpp_viewer_log = "/tmp/cpp_viewer_parity.log"
    rust_viewer_log = "/tmp/rust_viewer_parity.log"
    
    cpp_viewer = start_cpp_viewer(CPP_DISPLAY, cpp_viewer_log)
    rust_viewer = start_rust_viewer(RUST_DISPLAY, rust_viewer_log)
    
    # Run automation
    try:
        run_identical_automation(CPP_DISPLAY, RUST_DISPLAY, DURATION_SECONDS)
    finally:
        # Stop viewers
        print("\nStopping viewers...")
        cpp_viewer.terminate()
        rust_viewer.terminate()
        cpp_viewer.wait(timeout=5)
        rust_viewer.wait(timeout=5)
        
        # Stop servers
        print("Stopping servers...")
        cpp_server.terminate()
        rust_server.terminate()
        cpp_server.wait(timeout=5)
        rust_server.wait(timeout=5)
    
    # Parse logs
    print("\nParsing viewer logs...")
    cpp_stats = parse_contentcache_stats(cpp_viewer_log)
    rust_stats = parse_contentcache_stats(rust_viewer_log)
    
    # Compare
    if compare_stats(cpp_stats, rust_stats):
        print("\n✅ OVERALL: PASS - Rust viewer matches C++ viewer behavior")
        return 0
    else:
        print("\n❌ OVERALL: FAIL - Rust viewer differs from C++ viewer")
        return 1

if __name__ == "__main__":
    sys.exit(main())
```

#### Task 4.3: Add to CI pipeline

**File**: `.github/workflows/test-contentcache.yml` (if using GitHub Actions)

```yaml
name: ContentCache Parity Tests

on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

jobs:
  parity-test:
    runs-on: ubuntu-latest
    
    steps:
    - uses: actions/checkout@v2
    
    - name: Install dependencies
      run: |
        sudo apt-get update
        sudo apt-get install -y xorg-server-source xvfb python3 python3-pip
        pip3 install pyautogui pytest
    
    - name: Build C++ viewer and server
      run: |
        cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
        make -C build -j$(nproc)
    
    - name: Build Rust viewer
      run: |
        cd rust-vnc-viewer
        cargo build --release
    
    - name: Run parity tests
      run: |
        cd tests/e2e
        python3 test_cache_parity.py
```

### 4.4 Validation Criteria

**Pass conditions**:
- ✅ Hit rate difference < 2%
- ✅ Bandwidth savings difference < 5%
- ✅ Both viewers complete without errors
- ✅ ARC list sizes within reasonable ranges (T1+T2 ≈ cache capacity)

**Example passing output**:
```
ContentCache Statistics Comparison
============================================================

Metric                         C++ Viewer           Rust Viewer          Diff      
--------------------------------------------------------------------------------
Hit count                      1234                 1229                 5         
Miss count                     234                  239                  5         
Hit rate (%)                   84.1                 83.7                 0.4       
Bandwidth saved (MiB)          4.7                  4.6                  2.1%      
Reduction (%)                  90.7                 90.3                 0.4       

ARC List Sizes
T1 (recently used once)        45                   43                   
T2 (frequently used)           123                  125                  
B1 (ghosts from T1)            12                   14                   
B2 (ghosts from T2)            8                    7                    
p (adaptive parameter)         67                   65                   

============================================================
✅ PASS: Hit rate within 2% tolerance
✅ PASS: Bandwidth savings within 5% tolerance

✅ OVERALL: PASS - Rust viewer matches C++ viewer behavior
```

---

## Phase 5: Validation and Documentation

**Duration**: 3-5 days  
**Complexity**: Low  
**Dependencies**: Phases 1-4 complete

### 5.1 Goals

- Comprehensive testing across all scenarios
- Update documentation
- Create migration guide
- Performance benchmarking

### 5.2 Tasks

#### Task 5.1: Comprehensive test suite

Run all existing e2e tests with both viewers:

```bash
# Test cache eviction
./tests/e2e/test_cache_eviction.py --viewer rust --cache-size 16
./tests/e2e/test_cache_eviction.py --viewer cpp --cache-size 16

# Test cross-platform (macOS ↔ Linux)
./tests/e2e/test_cross_platform.sh --viewer rust
./tests/e2e/test_cross_platform.sh --viewer cpp

# Test cache miss recovery
./tests/e2e/test_contentcache_hits.sh --viewer rust
./tests/e2e/test_contentcache_hits.sh --viewer cpp

# Parity test (key validation)
./tests/e2e/test_cache_parity.py
```

#### Task 5.2: Update documentation

**File**: `rust-vnc-viewer/CONTENTCACHE_QUICKSTART.md`

Add section on ARC eviction:

```markdown
## ARC Eviction Algorithm

The Rust viewer now uses the same ARC (Adaptive Replacement Cache) algorithm
as the C++ viewer for intelligent cache management.

### How ARC Works

- **T1 list**: Recently used once (new/recent content)
- **T2 list**: Frequently used (content accessed 2+ times)
- **B1 list**: Ghost entries evicted from T1 (tracks recent evictions)
- **B2 list**: Ghost entries evicted from T2 (tracks frequent evictions)
- **p parameter**: Adaptive target size for T1 (self-tuning)

### Benefits

- Better hit rates than simple LRU (5-10% improvement)
- Adapts to workload (favors recency or frequency as needed)
- Automatic tuning via ghost lists

### Configuration

```bash
# Set cache size (default 256 MB)
njcvncviewer-rs --cache-size=512 hostname:display

# View ARC statistics (debug mode)
RUST_LOG=debug njcvncviewer-rs hostname:display
```

### Statistics

On exit, the viewer reports:
```
INFO  ContentCache: 4.7 MiB bandwidth saving (90.7% reduction)
DEBUG ContentCache: ARC: T1=45 T2=123 B1=12 B2=8 p=67
```
```

#### Task 5.3: Performance benchmarking

Create benchmark comparing LRU vs. ARC:

**File**: `rust-vnc-viewer/benches/arc_vs_lru.rs` (new)

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfb_encodings::content_cache::ContentCache;

fn benchmark_arc_eviction(c: &mut Criterion) {
    c.bench_function("ARC eviction 1000 entries", |b| {
        b.iter(|| {
            let mut cache = ContentCache::new(10); // 10MB limit
            
            // Insert 1000 entries (will trigger many evictions)
            for i in 0..1000 {
                let pixels = vec![0u8; 128 * 128 * 4]; // 64KB each
                cache.store_decoded_pixels(
                    i,
                    pixels,
                    PixelFormat::rgb888(),
                    128,
                    128,
                    128,
                ).unwrap();
            }
            
            black_box(cache);
        });
    });
}

criterion_group!(benches, benchmark_arc_eviction);
criterion_main!(benches);
```

Run benchmarks:
```bash
cd rust-vnc-viewer
cargo bench --bench arc_vs_lru
```

#### Task 5.4: Migration guide

**File**: `CONTENTCACHE_MIGRATION_GUIDE.md` (new)

Document changes for users upgrading from old Rust viewer to new ARC-based version.

---

## Success Criteria

### Functional Requirements

- ✅ **ARC algorithm implemented**: T1, T2, B1, B2 lists working correctly
- ✅ **Eviction notifications sent**: Server receives cache eviction messages
- ✅ **Bandwidth tracking working**: Statistics match C++ viewer (±5%)
- ✅ **Cross-architecture tests pass**: Parity test shows <2% difference in hit rates

### Performance Requirements

- ✅ **Hit rate improvement**: ARC provides 5-10% better hit rates than LRU
- ✅ **Eviction overhead**: <1% CPU overhead for ARC management
- ✅ **Memory usage**: ARC metadata <1% of cache size

### Quality Requirements

- ✅ **No regressions**: All existing tests still pass
- ✅ **Code coverage**: >80% coverage for new ARC code
- ✅ **Documentation complete**: All new features documented
- ✅ **Logging comprehensive**: Debug logs show ARC operations

---

## Risk Assessment

### High Risk

**Risk**: ARC algorithm bugs causing incorrect evictions  
**Mitigation**: Comprehensive unit tests, parity tests with C++ viewer  
**Contingency**: Can fall back to simple LRU if critical bugs found

**Risk**: Protocol incompatibility with existing servers  
**Mitigation**: Backward compatibility via capability negotiation  
**Contingency**: Eviction notifications are optional (server ignores if not supported)

### Medium Risk

**Risk**: Performance degradation from ARC overhead  
**Mitigation**: Benchmark before/after, optimize if needed  
**Contingency**: Make ARC optional via config flag

**Risk**: Cross-architecture tests flaky due to timing  
**Mitigation**: Generous tolerances (2% hit rate, 5% bandwidth), multiple runs  
**Contingency**: Manual validation if automated tests unreliable

### Low Risk

**Risk**: Documentation outdated  
**Mitigation**: Update docs as part of each phase  
**Contingency**: Community can submit doc fixes

---

## Timeline

| Phase | Duration | Start | End |
|-------|----------|-------|-----|
| Phase 1: ARC Eviction Algorithm | 1-2 weeks | Week 1 | Week 2-3 |
| Phase 2: Eviction Protocol | 3-5 days | Week 2-3 | Week 3 |
| Phase 3: Bandwidth Tracking | 3-5 days | Week 2 | Week 2-3 |
| Phase 4: Cross-Architecture Tests | 1 week | Week 3 | Week 4 |
| Phase 5: Validation & Docs | 3-5 days | Week 4 | Week 4 |

**Total**: 3-4 weeks

**Notes**:
- Phases 2 and 3 can be done in parallel with Phase 1
- Phase 4 requires Phase 1-3 complete
- Phase 5 is final validation

---

## Appendix A: References

**C++ Implementation Commits** (from `CONTENTCACHE_RECENT_CHANGES_ANALYSIS.md`):

- **ARC Eviction**: d6ed7029, d019b7d9, 95a1d63c, 651c33ea, 52f74d7c
- **Bandwidth Tracking**: c9d5fa1d, b1a680c0, 8902e213
- **Protocol**: d019b7d9 (CacheEviction message)

**Documentation**:
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` - Overall design
- `ARC_ALGORITHM.md` - ARC algorithm details
- `CONTENTCACHE_ARC_EVICTION_SUMMARY.md` - C++ implementation summary
- `WARP.md` - Project conventions

**External References**:
- ARC Algorithm: Megiddo & Modha, FAST 2003
- RFB Protocol: https://github.com/rfbproto/rfbproto

---

## Appendix B: File Checklist

### Files to Modify

**Rust Viewer**:
- [ ] `rust-vnc-viewer/rfb-encodings/src/content_cache.rs` (major changes)
- [ ] `rust-vnc-viewer/rfb-protocol/src/messages/client.rs` (add CacheEviction)
- [ ] `rust-vnc-viewer/rfb-protocol/src/messages/types.rs` (add constants)
- [ ] `rust-vnc-viewer/rfb-client/src/framebuffer.rs` (bandwidth tracking)
- [ ] `rust-vnc-viewer/rfb-client/src/event_loop.rs` (send evictions)
- [ ] `rust-vnc-viewer/rfb-client/src/protocol.rs` (bandwidth accounting)
- [ ] `rust-vnc-viewer/rfb-encodings/src/cached_rect.rs` (track bandwidth)
- [ ] `rust-vnc-viewer/rfb-encodings/src/cached_rect_init.rs` (track bandwidth)

### Files to Create

**Testing**:
- [ ] `tests/e2e/test_cache_parity.py` (cross-viewer comparison)
- [ ] `rust-vnc-viewer/benches/arc_vs_lru.rs` (performance benchmarks)
- [ ] `rust-vnc-viewer/rfb-client/tests/bandwidth_stats_tests.rs` (unit tests)

**Documentation**:
- [ ] `CONTENTCACHE_RUST_PARITY_PLAN.md` (this document)
- [ ] `CONTENTCACHE_MIGRATION_GUIDE.md` (upgrade guide)

### Files to Update

**Documentation**:
- [ ] `rust-vnc-viewer/CONTENTCACHE_QUICKSTART.md` (add ARC section)
- [ ] `rust-vnc-viewer/README.md` (update features list)
- [ ] `tests/e2e/README.md` (add parity test docs)

**Testing**:
- [ ] `tests/e2e/log_parser.py` (enhance stats parsing)
- [ ] `tests/e2e/test_cache_eviction.py` (add Rust support)

---

**End of Document**
