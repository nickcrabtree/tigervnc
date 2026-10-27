# Agent guide: operations2

Read when selected by AGENTS.md, not at every startup. Paths and commands are
relative to the repository root. Detailed source sections are retained below.
Current AGENTS.md and shared policy supersede dated history: old quiet-window
times, expired governor lifts, amend/reset advice and older host assumptions do
not grant permission. For current quiet-window status consult
`~/scripts/cross_repo_notes/AGENT_POLICY.md`.

## Detailed reference

### Key Files

- `common/rfb/session cache.h/cxx`: session cache with ARC (Adaptive Replacement Cache) algorithm
- `common/rfb/PersistentCache.h/cxx`: PersistentCache with disk persistence
- `common/rfb/EncodeManager.cxx`: server-side integration (cache lookups, insertions)
- `common/rfb/DecodeManager.cxx`: client-side integration (cache retrieval, blitting)
- `common/rfb/encodings.h`: protocol constants and capability negotiation

## Rust viewer convergence / PersistentCache parity

Current convergence authority:

- `rust-vnc-viewer/docs/CONVERGENCE_GATES.md` is the current checklist for Rust viewer ↔ C++ reference convergence work.
- The current Rust PersistentCache implementation is 64-bit cache-ID (`u64`) based. Older references in planning docs to
  16-byte hashes, `[u8; 16]`, or hash-wire-format semantics should be treated as historical until rewritten.

Checkpoint commits:

- Implementation checkpoint: `aaa3e986 rust viewer: unify PersistentCache IDs to u64`.
- Documentation/checklist checkpoint: `406cfe8d docs: rebaseline Rust viewer convergence gates`.

Gate 2 starting point — C++ reference protocol parity:

- Start with `tests/e2e/test_cpp_persistentcache.py` and `tests/e2e/test_rust_persistentcache.py` as the smallest
  comparable C++ vs Rust PersistentCache pair.
- Useful follow-on harnesses: `tests/e2e/test_cpp_cache_back_to_back.py`, `tests/e2e/test_cache_parity.py`,
  `tests/e2e/test_persistent_cache_bandwidth.py`, `tests/e2e/test_persistent_cache_eviction.py`.
- Known viewer binaries/artifacts: C++ viewer `build/vncviewer/njcvncviewer`; Rust viewer artifacts under
  `rust-vnc-viewer/target/...`, including `njcvncviewer-rs` builds.

Protocol parity evidence to compare: `PersistentCachedRect`, `PersistentCachedRectInit`, `PersistentCacheQuery`,
`PersistentCacheEviction`, hit/miss counters, cache path / disk save-load markers, message order, encoding IDs, byte
sizes, query batches, and eviction behaviour.

## Related Documentation

- `PERSISTENTCACHE_DESIGN.md`: PersistentCache protocol specification
- `ARC_ALGORITHM.md`: Adaptive Replacement Cache algorithm details
- `tests/e2e/README.md`: End-to-end test suite documentation
- `README.rst`: General TigerVNC documentation
- `BUILDING.txt`: Detailed build instructions for all platforms

## Shared utility scripts

- `~/scripts/fix_markdown_blanks.py` — fixes MD022/MD032 markdownlint violations
  (missing blank lines around headings and list blocks) in any Markdown file.
  Code-fence aware and idempotent. Usage: `python ~/scripts/fix_markdown_blanks.py <file.md>`
  or `--check` to report without modifying.
