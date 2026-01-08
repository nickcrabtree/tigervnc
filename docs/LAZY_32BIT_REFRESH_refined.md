# Lazy 32-bit Refresh (Refined)

## 1. Problem statement
When network bandwidth is limited, the viewer’s AutoSelect behavior may switch the negotiated wire pixel format to a reduced depth (commonly 8bpp rgb332, sometimes 16bpp). This preserves interactivity, but it permanently degrades the client’s on-screen quality for any pixels that arrive at reduced depth: converting 8bpp→32bpp at the viewer cannot restore the missing color information.

This repository already has a “lossless refresh” mechanism intended to use idle bandwidth to improve quality over time. However, as long as the lossless refresh sends pixels in the **current negotiated pixel format**, it cannot repair reduced-depth color loss.

**Goal of Lazy 32-bit Refresh**: keep the realtime path low-depth when needed, but use idle time to deliver **canonical truecolor pixels** for cache population/upgrades so that subsequent cache hits can render at high quality even on low-bandwidth links.

## 2. Current state in this repo (important context)
This codebase already contains server-side tracking for reduced-depth updates and a lossless refresh scheduler:

- `common/rfb/EncodeManager.cxx` tracks `reducedDepthRegion` for any content last sent with negotiated `pf.bpp < 24`.
- When the client upgrades pixel format to >=24bpp, `EncodeManager::handlePixelFormatChange()` schedules an immediate refresh of `reducedDepthRegion` by injecting it into the existing lossless refresh machinery (`lossyRegion` / `pendingRefreshRegion`).
- `VNCSConnectionST::writeLosslessRefresh()` budgets refresh traffic based on estimated bandwidth and time until next real update.

What is **not** implemented today:

- A way to send refresh payloads at canonical 32bpp/24-depth while the connection is negotiated at 8/16bpp.
- A way for cache-init messages to carry a pixel format (so the receiver knows how to decode and store the canonical upgrade payload).
- Strict feature gating so vanilla TigerVNC peers never see unknown cache/canonical extensions.

This document describes a refined design that builds on the existing reduced-depth tracking and lossless refresh scheduling.

## 3. Goals
1. **Progressive quality enhancement**: use idle bandwidth to upgrade cached content to canonical truecolor.
2. **Cache-first quality**: prefer high-quality cached content over newly received low-depth content when possible.
3. **Bandwidth efficiency**: avoid spending lossless refresh budget on sending reduced-depth data when the objective is quality repair.
4. **Interoperability**: must behave correctly with vanilla TigerVNC servers/viewers (no cache extensions unless explicitly negotiated).
5. **Robustness**: treat all network input as untrusted; bounds/overflow checks and decompression limits are mandatory.

## 4. Non-goals
- Supporting legacy “persistentcache/contentcache” deployments in the wild.
- Minimizing CPU/disk usage. The design explicitly trades CPU + disk for bandwidth and visual quality.

## 5. Key definitions and invariants
### 5.1 Canonical high-quality pixel format (protocol requirement)
All lazy upgrade payloads that claim “native/canonical format” MUST use a single canonical PixelFormat:

- `bpp = 32`
- `depth = 24`
- `trueColor = 1`
- 8 bits per component (RGB888), alpha byte ignored/padding
- endianness/max/shift fields set consistently per RFB PixelFormat rules

**Why canonical (not “server framebuffer PF”):**
- avoids platform-specific framebuffer formats and reduces the test matrix
- ensures the cache identity and upgrade semantics are stable

### 5.2 “Reduced depth” definition
Any negotiated depth `< 24` is considered reduced depth for the purposes of tracking and upgrade (covers both 8bpp and 16bpp cases).

### 5.3 Cache identity (cacheId)
The cache ID MUST be derived from canonical pixels:

- `cacheId = Hash(canonical 32bpp/24-depth pixels of rect)`

Invariants:

- A rect sent at 8/16bpp and later upgraded at canonical 32bpp uses the **same** cacheId iff the canonical pixels are identical.
- If pixels change, the hash changes; upgrades must not overwrite unrelated content.

## 6. High-level behavior
### 6.1 Realtime path (unchanged)
- Client and server continue to use negotiated pixel format selected by AutoSelect.
- CopyRect behavior remains unchanged.

### 6.2 Idle upgrade path (new)
During idle time, for regions known to have been sent at reduced depth (`reducedDepthRegion`):

- Server sends **canonical** pixel payloads via cache-init messages.
- Client decodes and stores canonical pixels in PersistentCache.
- Future cache hits can render in high quality even if the wire format remains reduced depth.

## 7. Interoperability (“play nice”) rules
This feature is strictly opt-in.

1. If the peer does not negotiate PersistentCache (and/or the unified cache protocol used in this fork), this feature is disabled and must have **zero** behavior changes.
2. Any extension to cache-init headers MUST be gated behind a dedicated capability encoding so that peers that don’t understand it never see it.
3. The server must never send cache-init messages unless the client has advertised the relevant pseudo-encodings in `SetEncodings`.

## 8. Protocol design
### 8.1 Preferred approach: extend cache-init with an explicit PF (v2 header)
Extend `PersistentCachedRectInit` (and/or the unified cache-init message used by this fork) with:

- a `flags` byte
- an optional PixelFormat field when `native_format` (canonical) flag is set

Conceptual layout:

- rect header
- encoding = `encodingPersistentCachedRectInit` (or current cache-init encoding)
- cacheId (U64)
- flags (U8)
  - bit 0: `NATIVE_FORMAT_FLAG` (meaning: PixelFormat follows and payload is encoded in that PF)
  - bits 1–7: reserved, must be 0
- if `NATIVE_FORMAT_FLAG` set: PixelFormat (16 bytes)
- payloadEncoding (S32)
- payload bytes

### 8.2 Capability negotiation (required)
Define a pseudo-encoding that indicates:

- client can parse the v2 header
- client can decode payloads using the supplied PixelFormat
- client enforces “canonical PF only” when the flag is set

Suggested value (as in earlier drafts):

- `pseudoEncodingNativeFormatCache = -327`

Negotiation rule:

- Client advertises `-327` in `SetEncodings` if it supports v2.
- Server only sends v2 headers if `-327` was advertised.
- If PersistentCache is negotiated but `-327` is not, server may send legacy v1 cache-init (no flags byte) and MUST NOT send canonical upgrades.

### 8.3 Canonical PF validation (hard requirement)
When `NATIVE_FORMAT_FLAG` is set:

- PixelFormat must match the canonical PF exactly.
- Any mismatch is a protocol violation (recommended handling: fail closed / disconnect).

This prevents accidental “native PF drift” and makes cache correctness testable.

## 9. Server-side design
### 9.1 Region tracking (already present)
Continue tracking:

- `reducedDepthRegion`: regions last sent at depth < 24
- `lossyRegion`: regions that may be visually lossy due to JPEG/other lossy encoding

### 9.2 Refresh ordering
During lossless refresh:

1. Upgrade `reducedDepthRegion` first (canonical PF via cache-init, only when negotiated).
2. Then refresh `lossyRegion` (policy-dependent; can remain negotiated PF, or be treated as canonical later).

Rationale: reduced-depth color shift is more objectionable than typical JPEG artifacts.

### 9.3 “Idle” and scheduling
Lossless refresh is currently opportunistic based on bandwidth and next update time. For lazy upgrades, also add an explicit responsiveness guard:

- Only send canonical upgrades when the connection is “idle” and there are no pending interactive updates.
- Initial conservative definition suggestion:
  - no pointer/key events for ~250–500ms AND
  - no pending server-core region AND
  - next real update is not imminent

Exact policy is tunable; the key is that canonical upgrades must not harm interactivity.

### 9.4 Payload generation at canonical PF
When upgrading reduced-depth regions:

- Convert from server framebuffer PF into canonical PF.
- Encode canonical pixels using the chosen payload encoding (ZRLE is a good default in strict lossless mode).
- Send as cache-init with `NATIVE_FORMAT_FLAG` and canonical PixelFormat.

Important: “canonical PF” is not necessarily equal to `pb->getPF()`.

### 9.5 Feature gating
All of the following must be true before sending canonical upgrades:

- client negotiated PersistentCache (or this fork’s cache protocol)
- client advertised `pseudoEncodingNativeFormatCache (-327)`
- server policy enables persistent cache and lazy upgrades

Otherwise:

- lossless refresh remains the existing behavior (refresh in negotiated PF).

## 10. Client-side design
### 10.1 Parsing and decode behavior
On receiving cache-init:

- If legacy v1: decode using negotiated connection PF (existing behavior).
- If v2 and `NATIVE_FORMAT_FLAG` set:
  - parse PixelFormat
  - validate it is canonical
  - decode payload in that PF
  - store canonical pixels in PersistentCache

### 10.2 Cache upgrade semantics
Cache stores the highest-quality entry for a given cacheId:

- if canonical entry already exists, ignore (or refresh LRU timestamp)
- if reduced-depth entry exists, replace with canonical

Because cacheId is defined over canonical pixels, canonical upgrades naturally align with cache identity.

### 10.3 Retrieval and display
- Prefer canonical cached entries when satisfying cache hits.
- Convert on retrieval if the destination framebuffer PF differs.

## 11. Robustness and safety requirements
Even in a test fork, messages are untrusted.

Client must validate:

- rect bounds are sane
- all size computations use 64-bit and check overflow (`width * height * bytesPerPixel`)
- bytesPerPixel derived from PixelFormat is valid
- decompression output is bounded to the expected pixel buffer size
- reserved flag bits are zero

Failure behavior:

- On invalid message: fail closed (disconnect is simplest) or safely ignore the rect; do not proceed with ambiguous parse state.

## 12. Metrics and observability
Server:

- pixels in `reducedDepthRegion`
- canonical upgrades sent / bytes
- time spent idle (opportunity measure)

Client:

- cache entries by quality (reduced-depth vs canonical)
- upgrades received
- cache hits by quality
- format conversion count on retrieval

## 13. Implementation phases (refined)
Phase 1: Foundation (already present)

- Track reduced-depth sends (`reducedDepthRegion`)
- Trigger refresh on pixel format upgrade (`handlePixelFormatChange()`)

Phase 2: Canonical upgrade path (server)

- Add canonical conversion + encode path for refresh payloads
- Teach lossless refresh to prioritize and drain `reducedDepthRegion` via canonical cache-init when negotiated

Phase 3: Protocol extension + negotiation

- Add v2 header (flags + optional PF)
- Add `pseudoEncodingNativeFormatCache (-327)` and gate v2

Phase 4: Client decode + cache upgrade

- Parse v2 gated by -327
- Validate canonical PF strictly
- Decode and store canonical entries

Phase 5: Testing and hardening

- bandwidth-limited tests (8bpp and 16bpp)
- verify reduced-depth regions upgrade during idle
- vanilla interoperability tests: ensure no cache/canonical messages are sent when not negotiated
- negative tests for bounds/overflow/decompression limits

## 14. Open questions / follow-ups
1. **Which message(s) are authoritative for cache-init in this fork?**
   This repo has both ContentCache and PersistentCache concepts and a “unified cache engine” in some areas. The canonical upgrade feature should attach to the actual on-wire cache-init path used in practice.
2. **Should lossyRegion also be upgraded canonically?**
   The primary motivation is reduced depth, but a similar approach could converge JPEG regions to bit-perfect canonical content over time.
3. **Idle definition ownership**
   Decide whether “idle” is purely server-side (based on input events seen by the server) or whether the viewer should participate (e.g., via a hint).
