# PersistentCache in the Rust Viewer

This document explains how the Rust viewer negotiates and uses the TigerVNC
**PersistentCache** protocol and how to enable it at runtime.

> **Parity note:** this document describes the current Rust behaviour. The
> current C++ viewer/server implementation has already moved to a unified cache
> path centred on PersistentCache negotiation, while Rust still contains
> separate ContentCache and PersistentCache code paths.

## Enabling PersistentCache

PersistentCache is **disabled by default**. In the current Rust
implementation, enabling it causes the viewer to advertise the pseudo-encoding
`-321` and add the PersistentCache encodings (`102` and `103`) **before**
ContentCache in the `SetEncodings` list. This matches the current Rust code,
but it does **not** yet represent full parity with the newer C++ unified-cache
model.

```rust
let mut cfg = rfb_client::config::Config::default();
cfg.connection.host = "localhost".into();
cfg.persistent_cache.enabled = true;
cfg.persistent_cache.size_mb = 256; // disk + memory accounting

let encs = cfg.effective_encodings();
assert_eq!(encs[3], -321); // PersistentCache pseudo before ContentCache
```

## What to expect in logs

At the end of a session, the viewer prints a cache summary and per-protocol
statistics similar to the C++ viewer.

```text
Cache summary:
 PersistentCache: 12.345 MiB bandwidth saving (67.8% reduction)

Client-side PersistentCache statistics:
 Protocol operations (PersistentCachedRect received):
 Lookups: 123, Hits: 100 (81.3%)
 Misses: 23, Queries sent: 23
 ARC cache performance:
 Total entries: 456, Total bytes: 7890123
 Cache hits: 100, Cache misses: 23, Evictions: 5
 T1 (recency): 12 entries, T2 (frequency): 34 entries
 B1 (ghost-T1): 8 entries, B2 (ghost-T2): 9 entries
```

## Developer notes

- Encoding ordering and related tests live in `rfb-client/src/config.rs`.
- PersistentCache decoders and client-cache logic live under `rfb-encodings/`.
- The event loop wires miss drains, eviction drains, and message writers in
  `rfb-client/src/event_loop.rs`.
- Use this file together with `docs/ROADMAP.md` when planning Rust/C++ cache
  parity work; the current Rust negotiation order is still transitional.
