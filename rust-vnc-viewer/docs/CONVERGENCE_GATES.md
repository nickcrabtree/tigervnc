# Rust Viewer / C++ Reference Convergence Gates

Source/status audit: 2026-11-01. This is the single current C++/Rust cache parity checklist.
Use the [current wire reference](../../docs/CACHE_PROTOCOL_CURRENT.md) for serialization details and the
[history/coverage map](../../docs/CACHE_DOCS_HISTORY.md) for all original plans and acceptance criteria.

## Verified baseline

The 2026-05-16 checkpoint recorded Rust u64 PersistentCache IDs, a passing full Rust workspace suite on
quartz/Nicks-Mac, and 12 changed files (131 insertions, 145 deletions): implementation `aaa3e986`,
documentation `406cfe8d`. This is dated evidence, not a test run for today's checkout.

The recorded historical suite invocation was:

```bash
cargo test --manifest-path rust-vnc-viewer/Cargo.toml --workspace --all-targets --all-features -- --nocapture
```

The later C++ source uses 16-byte CacheKey and mandatory-v2 INIT. The June 7, 2026
[cutover resume](../../docs/persistentcache_cpp_cutover_resume.md) explicitly reported no build validation
because local cmake and remote access were unavailable. This audit found the source changes but no new
runtime evidence that closes those gates. November 2025 hit rates do not validate the later cutover.

Source-inspected delivery (not runtime acceptance):

- C++ unified engine, memory/disk policy, key-based serializers, offset replay and native-format upgrades exist.
  Legacy CachedRect handlers reject use; current negotiation is -321, with native upgrades gated by -327.
- Rust REF/seed and outbound query/eviction still use u64. INIT consumes 16 key bytes but keeps only the high
  lane and heuristically accepts legacy/v2; it does not implement C++'s strict mandatory-v2 parser.
- Rust shared ARC, disk load/save and event-loop query/eviction drains exist. The old “no disk layer” and
  “no eviction notification” TODOs are superseded as implementation claims, but acceptance remains open.
- C++ unit tests include eviction wire sizes in `tests/unit/persistentcache_protocol.cxx`, quality/cross-bpp
  tests and shift-tolerant scanning tests. The Rust offset test in `persistent_cached_rect.rs` is ignored.
  Test existence and checked plan boxes do not establish passing integration.

## Gate 1 — Documentation rebaseline

- [x] Identify the current wire reference, this checklist, and the historical coverage map.
- [x] Preserve the May u64 baseline and distinguish later C++ cutover and mixed Rust INIT implementation.
- [x] Classify old hash-vector/u64 plans as archived in place, preserving their requirements and evidence.

## Gate 2 — C++ reference protocol parity

- [ ] Build and validate the C++ v2 cutover using the repository runner; record revision, environment and logs.
- [ ] Establish a single Rust wire identity across REF, INIT, seed, query, eviction and disk storage;
  retain both key lanes if implementing the C++ target. Do not silently equate u64 with a 16-byte CacheKey.
- [ ] Match mandatory INIT flags, canonical PF validation, reserved-flag rejection and incremental parsing.
- [ ] Match Query type 253, key widths, HashList chunks and eviction padding/batching.
- [ ] Consume REF offsets/dimensions and validate bounds; resolve the ignored Rust offset test.
- [ ] Run Rust against the C++ server; capture paired hit, miss/query, init, seed, eviction and fallback traces.
- [ ] Compare order, encodings, byte sizes, query batches and eviction behaviour using
  `rfb-client::protocol_trace::TraceSummary`: negotiated entries, rectangles, batch sizes and REF/INIT byte totals.
- [ ] Validate C++ internal first-u64 bookkeeping against full-key wire identity; record collision/coherency risks.
- [ ] Close the removal tracker's reliable CI criterion for ephemeral and persistent policies.

Start with `tests/e2e/test_cpp_persistentcache.py` and `tests/e2e/test_rust_persistentcache.py`.
Follow with `test_cpp_cache_back_to_back.py`, `test_cache_parity.py`,
`test_persistent_cache_bandwidth.py` and `test_persistent_cache_eviction.py` in that directory.
Use `./run_tests.sh` per the development guide; no tests were run in this prose-only audit.

### 2026-11-01 — Baseline validation attempt

Revision `fc51232225ad71526c3f57ea38a27c9e09a781f2` on quartz, Linux x86_64,
Debug CMake build; GCC 15.2.0, CMake 4.2.3, cargo/rustc 1.96.0.
C++ build **failed** (exit 2): `unix/tx/TXButton.h:121:8: error: ‘string’ in namespace ‘std’ does not name a type`;
the compiler identifies missing `<string>`, followed by undeclared `text` errors at lines 59 and 81.
Rust workspace/all-targets/all-features **passed** (443 passed, 6 ignored).
C++ `ctest -R PersistentCache -LE e2e` **passed 30/30**, but used pre-existing binaries
(the four persistentcache executables date to July 27); this does not validate HEAD.
All six requested e2e scripts are **blocked/unrun**: `run_tests.sh` cannot select/order
standalone scripts and always runs the full standalone suite, irrespective of CTest filters.
No runner or source fixes were made. The first Gate 2 checkbox remains unchecked.

See [per-step results and exact failure evidence](../../docs/CUTOVER_BASELINE_2026-11-01.md).
Full durable logs: `/ncloud/Nick/tigervnc-evidence/2026-11-01-cutover/`
(`environment.txt`, `cpp-build.log`, `rust-workspace.log`, `cpp-persistentcache-ctest.log`,
`cpp-test-binary-times.txt`, `e2e-runner-inspection.log`).

## Gate 3 — Cross-session PersistentCache

- [ ] Session A populates cache, exits, and saves disk state.
- [ ] Session B loads disk state and receives hits; verify HashList/known-key advertisement interoperability.
- [ ] Verify bandwidth reduction and no visual differences, including clean shutdown and corrupt-record recovery.
- [ ] Validate file format/version, checksums, bounded capacity, index/compaction and write-through requirements
  retained in the historical Rust plan; do not infer completion from the simpler existing load/save functions.

The C++ two-session harness requires warm-phase misses = 0 and hits > 0.
The historical Rust plan proposes >80% reconnect hits; retain that as a target until measured.

## Gate 4 — Pixel/screenshot parity

- [ ] Capture cache-disabled reference frames and cache-enabled frames.
- [ ] Assert pixel-identical repeated-content and shifted-content output across checkpoints.
- [ ] Validate decode/CopyRect ordering, insert-after-encode/session knowledge, pixel stride and commitBufferRW.
- [ ] Verify canonical upgrades at reduced wire depth, quality preference, offset bounds and corrupt input handling.
- [ ] Validate subregion mirror/index coherency separately from implemented containing-tile offset lookup.

## Gate 5 — Fallback and degradation

- [ ] Validate historical ContentCache-only -320 fallback with a matching old server; the current C++ server
  requires -321. Do not treat the original fallback gate as established current interoperability.
- [ ] Disk path unavailable/corrupt: no crash; fallback or skip bad records.
- [ ] Evicted IDs are reported and not reused incorrectly; test ARC adaptation and bounded accounting.
- [ ] Validate vanilla-peer behaviour with no cache/native-format extensions negotiated.
- [ ] Preserve bounds/overflow/decompression limits and fail safely on malformed protocol input.

## Gate 6 — Performance and bandwidth

- [ ] Compare no-cache, historical ContentCache, first-session PersistentCache and second-session PersistentCache
  with compatible peers; record the negotiated format for every result.
- [ ] Track bandwidth, CPU/decode time, memory, disk load/save duration and exact wire accounting.
- [ ] Measure the preserved plan targets: hashing >2 GB/s, append >100 MB/s, memory overhead <15%,
  query latency <10 ms; protocol-guide lookup <0.1 ms and 10K-entry load/save <200 ms.
- [ ] Validate scan budgets, exact verification, coverage safeguards, cancellation and churn/adaptive scheduling
  against the DOCX proposal criteria; optional GPU work is not implied by source presence.

Unresolved follow-ups A (compatibility) and B (validation), recommendations and no-action defaults are recorded
in the [durable coverage map](../../docs/CACHE_DOCS_HISTORY.md#unresolved-work-and-audit-limits).
