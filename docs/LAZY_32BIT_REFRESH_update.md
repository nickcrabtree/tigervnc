Lazy 32-bit Refresh Design (Updated)
Problem Statement
When network bandwidth is limited, the TigerVNC viewer’s AutoSelect feature switches to a low-depth pixel format (often 8bpp rgb332) to maintain responsiveness. This causes visible color degradation (e.g., purple/color-shifted regions). The current lossless refresh mechanism attempts to improve quality during idle time, but it sends refresh data at the current negotiated pixel format (e.g., 8bpp). Since conversion from 8bpp → display format cannot restore lost color information, the client’s framebuffer and cache remain degraded.
Current Behavior

Client measures throughput; if it drops below threshold, AutoSelect selects reduced pixel depth (e.g., 8bpp).
Client sends SetPixelFormat(8bpp) (or similar reduced depth).
Server sends updates at 8bpp.
During idle time, server sends “lossless refresh” — but still at 8bpp.
Client caches the 8bpp data.
When content is needed again, cache returns 8bpp data.
Visual quality remains degraded indefinitely.

Desired Behavior

Client uses reduced depth (8bpp or possibly 16bpp) for realtime responsiveness.
During idle time, server sends canonical 32bpp/24-depth truecolor data to populate/upgrade the cache.
Client stores that canonical high-quality data in PersistentCache.
When the same content is needed again, client prefers cached canonical data over newly received low-depth content.
Visual quality is excellent despite low bandwidth, without sacrificing interactivity.


Goals

Progressive Quality Enhancement: Use idle bandwidth to upgrade cached content to full color depth.
Cache-First Display: Prefer high-quality cached content over low-quality fresh content when possible.
Bandwidth Efficiency: Avoid “refreshing” low-depth content during lossless refresh when the goal is quality improvement.
Interoperability with Vanilla TigerVNC: Must behave correctly with servers/viewers that do not implement persistentcache/contentcache (but do implement CopyRect and AutoSelect2).
Robustness & Safety: Ensure bounds checks, overflow protection, and decompression safety.


Non-Goals

Supporting legacy “persistentcache/contentcache” deployments in the wild (there are none).
Minimizing CPU/disk usage: this protocol is intentionally designed to trade more CPU + disk for less bandwidth.


Key Definitions and Invariants
Canonical High-Quality Pixel Format (Protocol Requirement)
All native-format cache upgrades use a canonical PixelFormat:

bpp: 32
depth: 24
trueColor: 1
RGB: 8 bits per component (RGB888, alpha ignored/padding byte)
Endianness and shifts/max fields are set consistently per RFB PixelFormat rules.

This removes ambiguity about “native format” and prevents platform-specific framebuffer formats from expanding the test matrix.
Cache ID (cacheId)
cacheId is defined elsewhere as:

cacheId = Hash( canonical 32bpp / 24-depth truecolor pixels of the rect )

This invariant is fundamental:

A rect transmitted at 8bpp and later upgraded at canonical 32bpp must use the same cacheId if the underlying canonical pixels are identical.
If pixels change, the hash changes; upgrades must not overwrite unrelated content.


Architecture Overview
Current Data Flow
Plain Text
Problem: 8bpp→display conversion cannot restore missing colors. Cache and framebuffer retain degraded data.
Proposed Data Flow (Lazy 32-bit Refresh)
Plain Text

Interoperability Requirement (Vanilla TigerVNC)
“Play Nice” Rules


If peer does not advertise PersistentCache encodings, we must behave like vanilla TigerVNC:

Do not send persistentcache/contentcache pseudo-encodings.
Do not assume client/server supports cache init messages.
CopyRect and AutoSelect2 behave unchanged.



Persistentcache is opt-in via SetEncodings negotiation:

Only use persistentcache messages if the peer explicitly advertises the relevant pseudo-encodings.
If not negotiated, this feature is entirely disabled and has zero effect on vanilla behavior.




Protocol Design
We will implement Option A (extend PersistentCachedRectInit), with explicit negotiation so vanilla peers never see it.
Capability Negotiation
Define a pseudo-encoding indicating support for the extended PersistentCachedRectInit header:

pseudoEncodingNativeFormatCache = -327

Negotiation rule:

Client advertises -327 in SetEncodings if it can parse the v2 header and decode payload in the supplied PixelFormat.
Server sends v2 header only if -327 was advertised.
If -327 not advertised:

Server may still use legacy PersistentCache messages (if implemented and negotiated),
But MUST send the legacy header layout (no flags byte).


If PersistentCache itself is not advertised:

Server MUST NOT send any PersistentCache messages at all.



PersistentCachedRectInit v2 Layout
Legacy (v1):
Plain TextShow more lines
Proposed (v2):
Plain TextShow more lines
Protocol requirement: when native_format=1, the pixelFormat must be the canonical 32bpp/24-depth truecolor format.
Why Option A Is Safe Here

Breaking change is acceptable because persistentcache isn’t released “in the wild”.
Interop with vanilla is preserved because vanilla peers won’t advertise persistentcache encodings, and thus will never receive these messages.


Client-Side Changes
1) Pixel Format Handling in Decoder
When receiving PersistentCachedRectInit:

If v1: decode using connection PixelFormat.
If v2 with native_format=1: read PixelFormat, validate it is canonical, and decode using it.

Pseudo-code:
C++
2) Cache Storage Strategy
Since canonical native_format is mandated, cache upgrades are straightforward:

If entry exists at canonical format, ignore new canonical entry for the same cacheId (unless you want to refresh timestamp/LRU).
If entry exists at reduced depth, replace with canonical.

C++Show more lines
3) Cache Retrieval

Cache entries will usually be canonical.
Display uses framebuffer PF. Convert only if framebuffer PF is not identical.

C++Show more lines
4) 16bpp Considerations
AutoSelect may choose 16bpp on some links/configurations. The reduced-depth tracking must treat:

reduced depth: any negotiated depth < 24 (i.e., 8bpp or 16bpp)
canonical upgrade: always to canonical 32bpp/24-depth

This ensures the same mechanism corrects 16bpp banding and color loss as well as 8bpp rgb332.

Server-Side Changes
1) Lossless Refresh at Canonical Format
Modify lossless refresh so that when upgrading reduced-depth regions and persistentcache is negotiated, payload is sent in canonical PF (not negotiated PF).
Key points:

Canonical PF is not “framebuffer PF”; it’s the protocol requirement.
Convert server framebuffer pixels into canonical PF for hashing and for upgrade payload generation.

C++
2) Track “Client Needs Upgrade” Regions
Extend tracking to cover all reduced-depth cases:

reducedDepthRegion: any content last sent at depth < 24
lossyRegion: JPEG/lossy artifacts

Lossless refresh ordering:

Upgrade reducedDepthRegion first using canonical PF (persistentcache only).
Then refresh lossyRegion (may remain at negotiated PF depending on policy).

C++
3) Maintain CopyRect and AutoSelect2 Behavior

CopyRect is orthogonal and must remain valid regardless of persistentcache negotiation.
If persistentcache is off, CopyRect works as today.
If persistentcache is on, CopyRect should still be used where beneficial, but cache messages must not interfere with CopyRect semantics.

Implementation note: do not assume persistentcache exists when handling CopyRect paths—keep feature gating strict.

“Idle” and Scheduling
AutoSelect2 handles negotiated pixel format switching (e.g., 8bpp vs 32bpp). It does not define “idle”.
For this feature, define idle explicitly and independently, for example:

“Idle” means: no user input events for N milliseconds AND no pending interactive updates waiting to be sent.

The upgrade mechanism should only run when idle is true. The intent is to avoid harming responsiveness, even though CPU/disk usage is not a concern.
(Exact N can be tuned; start with something conservative like 250–500 ms.)

Robustness & Safety Requirements
Even in a test implementation, parsing untrusted network data requires hardening.
Bounds and Overflow Checks
Client must validate:

width, height are within sane limits.
width * height multiplication uses 64-bit and checks overflow.
bytesPerPixel derived from PixelFormat is valid and consistent.
Total decoded pixel buffer size width*height*bytesPerPixel is bounded.

PixelFormat Validation
When native_format=1:

PixelFormat must match canonical exactly.
Reserved flag bits must be zero.
Reject non-canonical PF immediately (protocol error).

Decompression Safety
For ZRLE (or any encoding):

Limit maximum decompressed size to expected pixel buffer size.
Ensure decompressor cannot allocate unbounded memory.
Validate all lengths read from the stream.

Failure Behavior

On invalid message: fail closed (disconnect or ignore rect safely depending on policy).
Never proceed with partial/ambiguous parse states.


Implementation Phases (Updated)
Phase 1: Foundation (Server-Side Tracking)
Status: Complete

 Track reducedDepthRegion for content sent at < 24 depth (8bpp/16bpp)
 Trigger refresh when pixel format upgrades via handlePixelFormatChange()
 lastSentBpp tracks pixel depth of updates

Phase 2: Server-Side Canonical Format Refresh
Estimated effort: 1–2 days

 Introduce canonical PF conversion path for refresh payloads
 Modify writeLosslessRefresh() to upgrade reducedDepthRegion via canonical PF when negotiated
 Prioritize reducedDepthRegion over lossyRegion
 Clear reducedDepthRegion after successful canonical send

Deliverable: Server can send canonical 32bpp/24-depth upgrade data during idle time (persistentcache clients only).
Phase 3: Protocol Extension (Breaking Change Acceptable)
Estimated effort: 1 day

 Add flags byte to PersistentCachedRectInit header (v2)
 Define NATIVE_FORMAT_FLAG (bit 0)
 When native_format=1, include 16-byte PixelFormat (canonical required)
 Add capability pseudo-encoding -327 to gate v2 parsing/sending
 Update SMsgWriter::writePersistentCachedRectInit()
 Update CMsgReader::readPersistentCachedRectInit()

Deliverable: Wire protocol carries canonical PixelFormat for cache init upgrades.
Phase 4: Client-Side Canonical Decode + Cache Upgrade
Estimated effort: 1–2 days

 Parse v2 header gated by -327
 Validate canonical PF strictly
 Decode payload using specified PF (canonical)
 Store decoded pixels in cache (replace reduced-depth entry)
 Convert on retrieval if framebuffer PF differs
 Delete existing cache on disk (no migration required)

Deliverable: Client displays canonical cached content regardless of current wire format.
Phase 5: Testing & Hardening
Estimated effort: 1–2 days

 Manual testing under bandwidth limits (8bpp and 16bpp cases)
 Verify reduced-depth regions upgrade during idle
 Verify cache HITs return canonical quality
 Verify behavior with vanilla TigerVNC peers: no persistentcache messages sent/expected
 Add bounds/overflow/decompression tests (negative/invalid inputs)


Design Decisions (Updated)
1. Upgrade Priority: Reduced Depth First
Decision: Upgrade regions sent at < 24 depth before JPEG/lossy regions.
Rationale: Reduced-depth color shift is more objectionable than JPEG artifacts.
2. Compatibility Scope
Decision: Breaking changes to persistentcache/contentcache are acceptable (not released).
Requirement: Must interoperate with vanilla TigerVNC servers/viewers by strictly gating persistentcache behavior behind SetEncodings negotiation.
3. Canonical Pixel Format for Upgrades (Protocol Requirement)
Decision: Native-format upgrades MUST use canonical 32bpp/24-depth truecolor.
Rationale: Eliminates ambiguity and reduces conversion/matrix complexity.
4. Cache Storage: Always Highest Quality
Decision: Cache is upgraded in-place (8/16bpp entries replaced by canonical).
Rationale: CacheId is defined on canonical pixels; upgrades align with that identity.
5. Hash Computation
Decision: cacheId is hash of canonical 32bpp/24-depth pixels.
Rationale: Same semantic content maps to same ID regardless of negotiated wire format.
6. CPU/Disk Tradeoff
Decision: Favor CPU and disk usage over bandwidth.
Rationale: Modern systems have abundant compute/storage; bandwidth (especially mobile) is the limiting resource. No additional compression complexity is required beyond existing encodings.
7. Idle is Separate from AutoSelect2
Decision: Define “idle” independently; AutoSelect2 controls pixel format switching but does not define idle scheduling.
8. Robustness
Decision: Add strict bounds checks, overflow protection, and decompression safety limits.

Metrics and Monitoring (Updated)
Server Metrics

reduced_depth_region_pixels: pixels last sent at depth < 24
native_format_upgrades_sent: count of canonical upgrade messages
native_format_upgrade_bytes: bytes sent for canonical upgrades
upgrade_idle_time_ms: time spent in idle state (useful to understand opportunities)

Client Metrics

cache_entries_by_quality: count of entries reduced-depth vs canonical
cache_upgrades_received: count of entries upgraded to canonical
cache_hits_by_quality: cache hits returning reduced-depth vs canonical
format_conversions: conversions on retrieval (should be low if framebuffer PF is also canonical)


References (Code Locations)

common/rfb/EncodeManager.cxx: server encoding and lossless refresh logic
common/rfb/VNCSConnectionST.cxx: connection handling / refresh triggering
vncviewer/DecodeManager.cxx: decoding and cache init handling
vncviewer/PersistentCache.cxx: cache storage, eviction, retrieval
CONTENTCACHE_DESIGN_IMPLEMENTATION.md: existing protocol documentation
