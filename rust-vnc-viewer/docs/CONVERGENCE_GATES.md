# Rust Viewer / C++ Reference Convergence Gates

> Current as of 2026-05-16. This file supersedes stale PersistentCache checklist wording that describes 16-byte hash wire formats.

## Verified baseline

- PersistentCache wire format is now 64-bit cache-ID (`u64`) based in the Rust viewer.
- Full Rust viewer workspace suite passed on quartz/Nicks-Mac using `cargo test --manifest-path rust-vnc-viewer/Cargo.toml --workspace --all-targets --all-features -- --nocapture`.
- Implementation checkpoint diffstat: 12 files changed, 131 insertions, 145 deletions.
- Implementation checkpoint commit: `aaa3e986 rust viewer: unify PersistentCache IDs to u64`.

## Gate 1 — Documentation rebaseline

- [x] Record verified `u64` implementation baseline.
- [ ] Rewrite stale 16-byte hash / `[u8; 16]` sections in PersistentCache docs.
- [ ] Mark old hash-based wire-format examples as historical or remove them.

## Gate 2 — C++ reference protocol parity

- [ ] Run Rust viewer against the C++ server with PersistentCache enabled.
- [ ] Capture Rust and C++ traces for hit, miss/query, init, seed, eviction, and fallback.
- [ ] Compare message order, encoding IDs, byte sizes, query batches, and eviction behaviour.

## Gate 3 — Cross-session PersistentCache

- [ ] Session A populates cache, exits, and saves disk state.
- [ ] Session B loads disk state and receives PersistentCache hits.
- [ ] Verify bandwidth reduction and no visual differences.

## Gate 4 — Pixel/screenshot parity

- [ ] Capture reference frames with PersistentCache disabled.
- [ ] Capture frames with PersistentCache enabled.
- [ ] Assert pixel-identical output for repeated-content and shifted-content cases.

## Gate 5 — Fallback and degradation

- [ ] Server supports only ContentCache: select `-320` fallback.
- [ ] Disk path unavailable/corrupt: no crash; fallback or skip bad records.
- [ ] Evicted IDs are reported and not reused incorrectly.

## Gate 6 — Performance and bandwidth

- [ ] Compare no-cache, ContentCache, first-session PersistentCache, and second-session PersistentCache.
- [ ] Track bandwidth, CPU/decode time, memory, disk load/save duration.
