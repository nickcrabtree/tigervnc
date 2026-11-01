# Current cache protocol reference

Source inspection: 2026-11-01. This is the current wire reference for this checkout.
The [convergence checklist](../rust-vnc-viewer/docs/CONVERGENCE_GATES.md) owns delivery status and remaining work.
The [history and coverage map](CACHE_DOCS_HISTORY.md) classifies earlier designs and preserves their requirements.

## C++ wire contract

The C++ source has a hard cutover to a fixed **16-byte `CacheKey`**, with mandatory v2 INIT flags.
This supersedes the May 2026 u64 wire baseline for C++; it does not establish Rust parity.
`CacheKey` is a content-addressable 128-bit key over canonical pixels with dimensions in the hash domain.
Internal first-u64 bookkeeping remains in C++ and must not be confused with the wire identity.

All sizes below exclude transport framing and the enclosing FramebufferUpdate header.
Rectangle headers are 12 bytes; integers use network byte order; keys are 16 opaque bytes.

| Server rectangle | Encoding | Payload after rectangle header | Total bytes |
| --- | --- | --- | --- |
| PersistentCachedRect | 102 | Key + u16 ox, oy, cachedW, cachedH | 36 |
| PersistentCachedRectInit | 103 | Key + flags + optional PF + inner encoding + pixels | 33 + pixels; 49 with PF |
| CachedRectSeed | 105 | Key; snapshot existing framebuffer pixels | 28 |

Offset references use encoding **102**, including zero offsets; the source constant for WithOffset aliases 102.
INIT flags are u8, optional PF is 16 bytes, and the inner encoding is i32.
Older comments proposing a separate encoding 104 are not the current serializer contract.
INIT flag bit 0 selects a canonical PixelFormat; bits 1–7 are reserved and rejected.
The C++ reader rejects flagless INIT and unnegotiated or noncanonical native-format payloads.

| Client message | Type | Fields after type byte | Total bytes for N keys |
| --- | --- | --- | --- |
| PersistentCacheQuery | 253 | u16 count + keys | 3 + 16N |
| PersistentCacheHashList | 252 | u32 sequence + u16 totalChunks, chunkIndex, count + keys | 11 + 16N |
| PersistentCacheEviction | 249 | u8 padding + u16 count + keys | 4 + 16N |
| PersistentCacheHashReport | 247 | Canonical key + actual key | 33 |

C++ negotiation enables the unified path with `-321`; native-format upgrades require `-327`.
The viewer advertises `-321`, with `-327` when supported. Advertising only historical `-320` does not enable
the current server path. Memory-only versus disk-backed storage is a viewer policy, not a second wire protocol.
Legacy CachedRect handlers in DecodeManager reject use in favour of PersistentCachedRect.

Source authorities:
[constants](../common/rfb/encodings.h), [message types](../common/rfb/msgTypes.h),
[key](../common/rfb/CacheKey.h), [server writer](../common/rfb/SMsgWriter.cxx),
[client writer](../common/rfb/CMsgWriter.cxx), [client reader](../common/rfb/CMsgReader.cxx),
[server negotiation](../common/rfb/VNCSConnectionST.cxx), [viewer negotiation](../common/rfb/CConnection.cxx).

## Rust implementation boundary

Preserve the Rust **u64** baseline when describing its existing REF, query, eviction and seed paths.
It is not accurate to describe the whole checkout as using a single u64 wire format:

- REF reads only 8 bytes and substitutes zero offsets and destination dimensions: 20 bytes including the header.
- Seed reads a u64: 20 bytes including the header.
- INIT reads two u64 lanes (16 wire bytes), discards the low lane, and stores under the high u64.
  It first tries a flagless inner encoding, then heuristically tries v2. Flags=0 is ambiguous with legacy Raw.
  This does not match C++ mandatory-v2 parsing and canonical PF validation.
- Query writes type **254**, u16 count, then u64 IDs; C++ Query is type **253** with 16-byte keys.
- Eviction writes type 249, u16 count, then u64 IDs; C++ also includes padding and uses 16-byte entries.
- ARC, disk load/save, query and eviction plumbing exist; their presence is not proof of wire or pixel parity.

See [REF decoder](../rust-vnc-viewer/rfb-encodings/src/persistent_cached_rect.rs),
[INIT decoder](../rust-vnc-viewer/rfb-encodings/src/persistent_cached_rect_init.rs),
[seed decoder](../rust-vnc-viewer/rfb-encodings/src/persistent_cached_rect_seed.rs),
[message serializers](../rust-vnc-viewer/rfb-protocol/src/messages/client.rs),
[cache](../rust-vnc-viewer/rfb-encodings/src/persistent_cache.rs), and
[event loop](../rust-vnc-viewer/rfb-client/src/event_loop.rs).
No product implementation changes were made during this documentation audit.

## Ordering and correctness

Cache snapshots and replay must respect normal decode and CopyRect ordering; flush pending work before
unsynchronized framebuffer reads or writes. Stride is in **pixels**, so row offsets multiply stride by bytes per pixel.
C++ writes through `getBufferRW` require `commitBufferRW`. Preserve assertions, bounds checks, canonical PF checks,
and pixel-identical cache-on/cache-off acceptance criteria. Use isolated headless displays :998/:999 for validation;
production displays :1/:2/:3 and ports 5901/5902/5903 remain off limits.
