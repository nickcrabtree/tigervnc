# Sub-Region Cache Lookup Design

## Executive Summary

This document proposes an enhancement to the TigerVNC caching system to improve performance when windows are moved, revealing previously-obscured content. The current caching mechanism fails to recognise that exposed pixels are identical to content already in the viewer's cache, resulting in unnecessary network traffic and perceived sluggishness.

## Problem Statement

When a user drags a window across the screen, the underlying content is progressively exposed. Despite this content being unchanged and already present in the viewer's cache from previous frames, the current system treats each exposed region as new data and retransmits it over the network.

This causes:
- **Increased latency**: Each mouse movement triggers network round-trips
- **Wasted bandwidth**: Identical pixels are sent repeatedly
- **Sluggish user experience**: Window dragging feels unresponsive, especially over high-latency connections

## Background: How the Current Cache Works

The existing PersistentCache system works by:

1. **Hashing rectangles**: When the server sends a region to the viewer, it computes a cryptographic hash of the pixel data
2. **Storing by hash**: The viewer stores the pixels indexed by this hash
3. **Lookup by hash**: On subsequent updates, if the server finds the same hash in its "known" set, it sends a small reference instead of the full pixel data

### Why This Fails for Window Movement

The critical limitation is that **hashes depend on rectangle boundaries**. 

Consider this scenario:
- Frame 1: Server sends a 400×300 pixel region at position [100,100]. Hash = ABC123.
- Frame 2: User drags a window, exposing a 10×300 sliver at position [200,100].

The exposed sliver contains pixels that are part of the original cached region, but:
- The sliver has different boundaries (10×300 vs 400×300)
- Different boundaries produce a different hash
- Different hash = cache miss
- Result: The 10×300 sliver is retransmitted despite being identical to pixels already in cache

This problem compounds: each mouse movement exposes another thin sliver, each with unique boundaries, each causing a cache miss.

## Thought Process: Approaches Considered

### Approach 1: Grid-Aligned Tiles (Rejected)

**Idea**: Divide the screen into a fixed grid (e.g., 64×64 tiles). Always hash and cache at tile boundaries. When content is exposed, the tile boundaries would be consistent, enabling cache hits.

**Why it doesn't work**:
- Exposed slivers during window dragging are typically 1-10 pixels wide
- A 64×64 tile would never fit entirely within such a thin sliver
- Even with smaller tiles, the fundamental mismatch between arbitrary damage regions and fixed grids remains problematic
- Edge tiles would still have inconsistent boundaries

### Approach 2: Screen-Location Mapping with Sub-Region References (Proposed)

**Idea**: Track which cache entry covers each screen location. When a region is exposed, look up the covering cache entry and tell the viewer to blit a sub-region from its existing cache.

**Key insight**: The viewer already has the pixels. We just need to tell it *where within its cache* those pixels are, and *where on screen* to place them.

## Proposed Design

### Conceptual Overview

The enhancement adds two new capabilities:

1. **Server-side pixel mirror**: The server maintains a copy of what it believes is in the viewer's cache, including the pixel data and the screen locations each cache entry covers.

2. **Sub-region references**: A new protocol message that says "take pixels from cache entry N, starting at offset (ox, oy), and place them at screen position (x, y)".

### How It Would Work

**When sending new content to the viewer:**
1. Server sends the pixels with a cache ID (as it does today)
2. Server stores a copy of those pixels in its local mirror
3. Server records: "Screen region [x,y to x+w,y+h] is covered by cache entry N"

**When a region is exposed (e.g., window moved):**
1. Server receives damage notification for exposed region [200,100 to 210,400]
2. Server looks up spatial index: "Is this region covered by any cache entry?"
3. Finds: "Yes, cache entry N covers [100,100 to 500,400]"
4. Computes offset: ox = 200-100 = 100, oy = 100-100 = 0
5. **Verification step** (critical for correctness):
   - Extracts the 10×300 sub-region from its pixel mirror (what the viewer should have)
   - Extracts the same sub-region from the current framebuffer (what's actually there now)
   - Compares the two
6. If pixels match: Sends "use cache N at offset (100,0) for screen position (200,100)"
7. If pixels differ: Content changed while obscured; uses normal encoding path

### Why Verification Is Necessary

The underlying content might change while obscured:
- A video could be playing in a background window
- An application could update its display
- A web page could auto-refresh

Without verification, the server might instruct the viewer to display stale cached content, causing visual corruption. The hash comparison ensures we only use cached content when it's actually correct.

## Protocol Changes

A new message type would be needed:

**CachedRectWithOffset**
- Screen position: x, y
- Dimensions: width, height  
- Cache ID: 8 bytes (existing format)
- Offset within cache entry: ox, oy

This is a modest extension to the existing CachedRect message, adding only 4 bytes for the offset coordinates.

## Alternatives Considered

### Alternative A: Tile-Based Caching at Multiple Scales

Store content at multiple tile sizes (e.g., 16×16, 64×64, 256×256). When looking up, try to find the best-fitting cached tile.

**Pros:**
- No new protocol messages needed
- Simpler mental model

**Cons:**
- Significant memory overhead (storing same content at multiple scales)
- Still fails for arbitrary sliver shapes
- Complex cache management with multiple overlapping entries

### Alternative B: Content-Addressable Screen Buffer

Maintain a full-screen buffer where each pixel is tagged with its cache ID. On expose, directly look up which cache entry each pixel belongs to.

**Pros:**
- Pixel-perfect accuracy
- Fast lookups

**Cons:**
- Extremely high memory usage (8 bytes per pixel for cache ID alone)
- Complex bookkeeping when content overlaps
- Doesn't scale to high resolutions

### Alternative C: CopyRect Enhancement

Extend the existing CopyRect encoding to support copying from cache rather than from screen positions.

**Pros:**
- Builds on existing, well-understood encoding
- Viewers already handle CopyRect efficiently

**Cons:**
- CopyRect assumes source pixels are currently on screen
- Would require significant semantic changes to CopyRect
- Potential confusion with existing CopyRect behaviour

## Pros and Cons of the Proposed Design

### Advantages

1. **Significant bandwidth reduction**: Exposed content uses tiny references (~24 bytes) instead of full pixel data (potentially megabytes)

2. **Reduced latency**: Less data to transmit means faster screen updates during window movement

3. **Leverages existing cache**: No changes needed to viewer-side cache storage; we're just using what's already there more intelligently

4. **Correctness guaranteed**: The verification step ensures we never display stale content

5. **Graceful degradation**: If verification fails or the region isn't in the spatial index, falls back to normal encoding with no ill effects

### Disadvantages

1. **Server memory overhead**: The server must store a copy of all cached pixels, roughly doubling memory usage for cache-related data

2. **Complexity**: Adds a new subsystem (spatial index + pixel mirror) that must be kept in sync with the viewer's cache state

3. **Protocol extension**: Requires viewer updates to understand the new message type; older viewers won't benefit

4. **Eviction coordination**: When the server evicts entries from its mirror cache, it loses the ability to reference those regions, even if the viewer still has them

5. **Computational overhead**: The verification step requires hashing potentially every exposed sliver, which could be CPU-intensive during rapid window movement

## Open Questions

1. **Cache sizing**: How large should the server-side mirror be? Should it match the viewer's cache size, or be smaller (accepting more misses)?

2. **Eviction policy**: Should server and viewer use the same eviction policy? What happens if they diverge?

3. **Hash granularity**: Should we hash the entire sub-region, or use a faster approximate comparison?

4. **Batching**: During rapid window movement, should we batch multiple slivers into a single message?

5. **Backward compatibility**: How should we handle mixed environments with old and new viewers?

## Next Steps

1. Gather feedback on this design from stakeholders
2. Prototype the spatial index to validate lookup performance
3. Measure memory overhead of the server-side pixel mirror
4. Design the exact wire format for CachedRectWithOffset
5. Implement and benchmark against the current system

## Conclusion

The proposed sub-region cache lookup mechanism addresses a real performance problem in VNC usage: the inefficiency of window movement operations. By tracking screen-location-to-cache mappings and enabling sub-region references, we can dramatically reduce bandwidth and latency for a common user interaction pattern.

The main trade-off is increased server memory usage and system complexity. Whether this trade-off is worthwhile depends on the deployment environment—high-latency or bandwidth-constrained connections would benefit most, while local or high-speed connections might not notice the difference.
