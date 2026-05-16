# Legacy roadmap pointer
> **2026-05-16 rebaseline:** Rust PersistentCache is now implemented and full-suite verified around **64-bit cache IDs (`u64`)** for PersistentCachedRect, PersistentCachedRectInit, query, eviction, and seed paths. Older references below to 16-byte hashes, `[u8; 16]`, or hash-wire-format semantics are historical context until rewritten. Use `rust-vnc-viewer/docs/CONVERGENCE_GATES.md` as the current checklist.


`IMPLEMENTATION_ROADMAP.md` is no longer the canonical roadmap for the Rust viewer.

Use these documents instead:

- `docs/ROADMAP.md` for the active implementation roadmap.
- `docs/protocol/persistent_cache.md` for the Rust viewer PersistentCache notes.

This stub stays in place so older references do not silently break while the documentation is being consolidated.
