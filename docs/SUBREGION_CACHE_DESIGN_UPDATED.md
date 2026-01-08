# Sub-Region Cache Lookup Design (Updated)

## Executive Summary

This document proposes an enhancement to the TigerVNC **persistentcache/contentcache** work to improve performance and perceived responsiveness when windows are moved, revealing previously-obscured content. The current cache lookup model hashes *whole rectangles* and therefore misses when the newly exposed region is a sub-rectangle of previously cached content. This causes unnecessary retransmission, extra latency, and a “sticky” feel during window dragging. 

The update introduces a **sub-region cache reference** that allows the server to instruct the viewer to blit pixels from an existing cache entry using an offset, rather than resending identical pixels.

This design is written to:
- Interoperate cleanly with **vanilla TigerVNC** servers/viewers that do *not* implement persistentcache/contentcache (but do support CopyRect and AutoSelect2).
- Align with the **Lazy 32-bit Refresh** update: cache entries are ultimately upgraded to a **canonical 32bpp / 24-depth truecolor** format for high quality re-use.
- Follow general RFB protocol extension principles (new encodings and pseudo-encodings negotiated via `SetEncodings`).

---

## Problem Statement

When a user drags a window across the screen, underlying content is progressively exposed. Even if that content is unchanged and already present in the viewer’s cache from previous frames, the current rectangle-hash cache model treats each exposed region as new and retransmits it.

This causes:
- **Increased latency**: each pointer movement can trigger additional data transfer.
- **Wasted bandwidth**: identical pixels are sent repeatedly.
- **Sluggish UX**: window dragging feels unresponsive on high-latency or bandwidth-limited links.

---

## Background: How the Current Cache Works

The existing PersistentCache mechanism works by:
1. **Hashing rectangles**: when the server sends a region, it computes a hash of the rectangle’s pixel data.
2. **Storing by hash**: the viewer stores pixels indexed by `cacheId`.
3. **Lookup by hash**: on later updates, if the server believes the client already has the hash, it sends a small reference instead of full pixels.

### Why This Fails for Window Movement

Hashes depend on **rectangle boundaries**. Example:
- Frame 1: server sends a 400×300 region at [100,100]. `cacheId = ABC123`.
- Frame 2: window moves, exposing a 10×300 sliver at [200,100].

The sliver is a subset of cached pixels, but:
- different boundaries ⇒ different hashed content definition ⇒ different `cacheId`.
- server cannot reference the previously cached rectangle.
- result: the sliver is resent.

---

## Relationship to RFB and Existing Encodings

RFB display updates are a sequence of rectangles. Each rectangle includes an encoding type (e.g., Raw, CopyRect, ZRLE). Extensions are typically added as:
- **New encodings** (a new rectangle encoding type)
- **Pseudo-encodings** (requested via `SetEncodings` to declare support for extensions)

CopyRect is excellent when the *source pixels are already on screen*. It does not help when pixels are **not currently visible**, e.g., content revealed from behind a moved window.

This proposal adds a *new rectangle encoding* that copies pixels from the **client’s cache**, rather than from the currently displayed framebuffer.

---

## Canonical Pixel Format and Cache Identity (Alignment with Lazy 32-bit Refresh)

To reduce ambiguity and ensure stable cache identity across negotiated pixel formats (8bpp/16bpp/32bpp), this design assumes:

### Canonical High-Quality Pixel Format
All *high-quality* cache storage and upgrades use a **canonical** pixel format:
- **bpp = 32**
- **depth = 24**
- **trueColor = 1**
- **RGB888** with a padding byte (alpha ignored)

This is the same canonical format required by the Lazy 32-bit Refresh update.

### Cache ID (cacheId)
`cacheId` is defined to be the hash of the **canonical 32bpp/24-depth truecolor pixels** of the cached rectangle.
- If pixels (in canonical form) are identical, `cacheId` is identical, regardless of negotiated wire format.
- If pixels changed while obscured, the hash will differ.

**Important note:** The hash definition must incorporate rectangle dimensions (width/height) and pixel byte ordering as part of the canonical byte stream to avoid collisions between different shapes.

---

## Approaches Considered

### Approach 1: Grid-Aligned Tiles (Rejected)
Divide the screen into fixed tiles (e.g., 64×64) and always hash/cache at tile boundaries.
- Thin slivers during dragging rarely align with tile boundaries.
- Edge tiles still create boundary mismatches.

### Approach 2: Screen-Location Mapping with Sub-Region References (Proposed)
Track which cache entry covers each screen location. When a region is exposed, look up the covering cache entry and instruct the viewer to blit a sub-rectangle from its cache.

Key insight: the viewer already has the pixels — the server just needs to specify:
- which cache entry (`cacheId`)
- which sub-region within it (offset `ox, oy`)
- where to put it on screen (`x, y`, `w, h`)

---

## Proposed Design

### Components

1. **Server-side cache mirror (canonical pixels)**
   - Server stores canonical pixels for cached entries it has sent/initialized.
   - Server keeps a spatial index from screen locations → cache entries.

2. **Sub-region cache reference rectangle encoding**
   - A new rectangle encoding that instructs: “copy from cache entry `cacheId` at offset (`ox`,`oy`) into framebuffer at (`x`,`y`) with size (`w`,`h`).”

3. **Verification step for correctness**
   - Before sending a cache-offset reference, the server verifies that the cached pixels still match the current framebuffer content for that region.
   - This avoids displaying stale content when the obscured area changed (video playback, animations, timers, etc.).

### How It Works

#### When sending new content to the viewer
1. Server sends pixels using existing persistentcache init mechanisms.
2. Server stores canonical pixels for that entry in its mirror.
3. Server records coverage: the on-screen rectangle is covered by that `cacheId`.

#### When a region is exposed (e.g., window moved)
1. Server receives damage for exposed region `R`.
2. Server queries the spatial index for cache entries overlapping `R`.
3. For each overlapping entry, server computes candidate sub-rectangles that can be satisfied from cache.
4. **Verification:** compare canonical pixels from mirror vs canonical pixels from live framebuffer for the candidate sub-rectangle.
5. If match: send a cache-offset reference rectangle.
6. If mismatch or no coverage: fall back to normal encoding path.

### Why Verification Is Necessary
The underlying content can change while obscured. Without verification, the server might instruct the viewer to display stale cached pixels, causing visual corruption.

Verification is bounded to the exposed rectangles. For thin slivers, the comparison is typically cheap.

---

## Protocol Changes

### New Encoding: `pseudoEncodingCachedRectWithOffset`

This proposal defines a new rectangle encoding type for inclusion in standard RFB `FramebufferUpdate` messages.

- **Name:** `CachedRectWithOffset`
- **Type:** rectangle encoding (not a new top-level message)
- **Negotiation:** viewer requests support via `SetEncodings` including this encoding.

Suggested encoding number (example only): `-328`.
> Note: The exact numeric ID must be chosen to avoid collisions with existing encodings used in your tree. In experimental branches, a local negative pseudo-encoding value is acceptable; if upstreaming, follow the RFB/IANA registration process.

#### Wire Format (rectangle payload)
Within a `FramebufferUpdate`, each rectangle already carries:
- x, y, width, height (U16 each)
- encoding type (S32)

For `CachedRectWithOffset`, the rectangle payload is:

```
U64  cacheId
U16  ox
U16  oy
```

Semantics:
- `cacheId`: identifies the cached rectangle (canonical pixels).
- `ox, oy`: top-left offset within the cached rectangle from which to copy.
- destination is the rectangle header’s (x,y,w,h).

Constraints:
- `ox + width  <= cached_width`
- `oy + height <= cached_height`

The viewer must know `cached_width`/`cached_height` from when it stored the cache entry.

### Capability Negotiation (Play Nice with Vanilla)

To interoperate with vanilla TigerVNC peers:
- Viewers/servers that do not implement persistentcache/contentcache will **not** request these encodings.
- Therefore they will never receive such rectangles.

Rules:
1. Server may only send `CachedRectWithOffset` rectangles if the client requested the encoding in `SetEncodings`.
2. If the encoding is not requested, server must fall back to standard encodings (CopyRect/Raw/ZRLE/Tight/etc.).

This follows standard RFB extension behavior: unknown encodings are ignored by servers, and clients never see encodings they didn’t request.

---

## Interaction with PersistentCache and Lazy 32-bit Refresh

### Quality levels during window movement
- If a region is satisfied from a cache entry that is only present at reduced depth (8bpp/16bpp), using a cache-offset reference will reproduce that reduced quality.
- The Lazy 32-bit Refresh mechanism upgrades reduced-depth cache entries to canonical 32bpp during idle time.

Recommended policy:
1. Prefer cache-offset references whenever correct, because they eliminate bandwidth/latency.
2. If the referenced cache entry is reduced-depth, mark it as needing upgrade (or add to `reducedDepthRegion`) so that Lazy 32-bit Refresh will upgrade it later.
3. Once upgraded, subsequent cache-offset references will render at full quality.

### Server tracking needs
To support the above, the server should track (per cacheId):
- whether it has sent canonical pixels for that id yet
- whether the client is believed to have the canonical version

---

## Server-Side Data Structures

### 1) Cache Mirror
For each cache entry in the server’s mirror:
- `cacheId`
- `width, height`
- `canonical_pixels` (byte array)
- `quality_state` (reduced-depth vs canonical)
- metadata (timestamp, refcount/LRU, etc.)

### 2) Spatial Index
A mapping from screen coordinates → coverage by cache entries.

Implementation options:
- Region map keyed by rectangles (coarser, easier)
- Interval tree / R-tree for overlapping rectangle queries

The index needs to answer:
- “What cached entries overlap exposed region R?”
- “For each overlap, what is the maximal sub-rectangle we can satisfy?”

### 3) Mirror/Index Coherency
When new pixels arrive and are cached:
- Update mirror entry.
- Update spatial index coverage for that cacheId.

When screen content changes in an area:
- coverage must be updated to point to the new cacheId/entry.

---

## Viewer-Side Requirements

1. Viewer must be able to locate a cache entry by `cacheId` and know its stored dimensions.
2. Viewer must be able to blit a sub-rectangle from cached pixels into the framebuffer.
3. Viewer must validate bounds (`ox, oy, w, h`) before copying.
4. Viewer must handle pixel format conversion if the cached pixels are canonical and the framebuffer format differs.
   - In practice, most viewers render into a 32bpp framebuffer, so canonical copies are usually direct.

Failure behavior:
- If `cacheId` is not present, the viewer cannot render correct pixels. This is a protocol correctness issue, not a recoverable decode error.

---

## Eviction Coordination (Important Gotcha)

A key correctness risk is **cache eviction divergence**:
- Viewer may evict an entry that the server still believes exists.
- If the server then sends `CachedRectWithOffset(cacheId)`, the viewer cannot satisfy it.

To avoid this, one of the following must be true:

### Option 1 (Phase 1 / Simplest): Disable client eviction
- Configure the viewer’s persistent cache large enough that eviction does not occur during testing.
- Server mirror may still evict conservatively if it chooses, but must not reference evicted entries.

### Option 2 (Recommended for robustness): Add eviction notification
Introduce an extension where the viewer notifies the server when it evicts cache entries.
- Negotiated via a pseudo-encoding capability.
- Implemented as a new client-to-server message or as an existing channel extension (depending on your protocol framework).

This keeps server belief aligned with client reality.

### Option 3: Deterministic shared eviction policy
Server and viewer use identical cache sizing and deterministic eviction so they evict the same IDs at the same time.
- Harder to guarantee in practice (timing differences, disk state, crashes).

---

## Correctness and Security Hardening

Even in an experimental branch, treat network inputs as untrusted.

### Bounds and Overflow
- Validate rectangle dimensions (`w`, `h`) and offsets (`ox`, `oy`).
- Use 64-bit arithmetic for `w*h*bytesPerPixel` and check overflow.
- Ensure `ox+w` and `oy+h` do not exceed cached entry dimensions.

### Decompression Safety
This extension itself does not add new decompression, but it depends on the integrity of cached entries, which originate from decoders.
- Ensure existing decoder paths have size limits and safe allocations.

### Verification Cost Control
Verification is required for correctness, but the server should avoid pathological CPU use:
- Coalesce exposed damage into fewer rectangles.
- Prefer verifying larger rectangles rather than per-pixel operations.
- Optional: sample-based fast rejection (e.g., compare a few rows/columns first) before full compare.

---

## Pros and Cons

### Advantages
1. **Significant bandwidth reduction**: exposed content uses a small reference rather than full pixels.
2. **Reduced latency**: fewer bytes to transmit improves responsiveness during dragging.
3. **Leverages existing cache**: viewer-side storage largely unchanged; adds a blit path.
4. **Correctness maintained**: verification prevents stale cache display.
5. **Graceful degradation**: if not covered or verification fails, fall back to normal updates.

### Disadvantages / Risks
1. **Server memory overhead**: server mirror stores canonical pixels.
2. **Complexity**: spatial index + mirror must remain consistent with screen evolution.
3. **Protocol extension**: requires updated viewers/servers to benefit.
4. **Eviction coordination**: must prevent server referencing entries the viewer no longer has.
5. **Verification CPU**: heavy expose patterns can increase compare work.

---

## Implementation Plan (Suggested)

### Phase A: Minimal viable sub-region references
- Add encoding `CachedRectWithOffset` and negotiation.
- Implement viewer blit-from-cache path with bounds checks.
- Implement server spatial index and mirror.
- Implement server-side verification.

### Phase B: Integration with Lazy 32-bit Refresh
- Ensure cache entries converge to canonical format.
- Track reduced-depth usage and schedule upgrades.

### Phase C: Eviction coordination
- Add eviction notification or deterministic eviction strategy.
- Add metrics and debugging tooling.

---

## Metrics

### Server
- `subregion_cache_hits`: number of rects served via CachedRectWithOffset
- `subregion_cache_hit_pixels`: pixels served via offset blits
- `subregion_cache_verify_failures`: verification mismatches
- `mirror_bytes`: memory used by mirror
- `spatial_index_nodes`: size/complexity metric

### Viewer
- `cache_offset_blits`: number of offset blits performed
- `cache_offset_blit_pixels`: pixels rendered from cache via offsets
- `cache_offset_bounds_failures`: rejected messages due to invalid offsets

---

## Open Questions

1. **Best spatial index structure** for typical damage patterns.
2. **Verification optimization** to reduce CPU under extreme dragging.
3. **Eviction design**: notification vs shared deterministic policy.
4. **Batching/coalescing**: optimal grouping of slivers into rectangles.
5. **Interactions with CopyRect**: when both are possible, what is the best precedence?

---

## Conclusion

The sub-region cache lookup mechanism solves a real inefficiency in VNC usage: arbitrary exposed shapes during window movement lead to cache misses when caching is rectangle-boundary dependent. By tracking screen-location-to-cache mappings and adding an offset-based cache reference encoding, we can reduce bandwidth and latency dramatically for a common UI interaction.

The primary trade-offs are added server memory and implementation complexity, and the need for a clear strategy for cache eviction coherence.
