# Cache documentation history and coverage map

Audit: 2026-11-01. Start with the [current protocol](CACHE_PROTOCOL_CURRENT.md) and
[single parity/status checklist](../rust-vnc-viewer/docs/CONVERGENCE_GATES.md).

## Archive policy and navigation

The following plans are **archived in place**: their existing paths, headings, evidence, code examples,
decisions, unresolved questions and acceptance criteria remain intact. No files were deleted or renamed.
Their dated claims of “current”, “complete”, “pending” or u64/hash wire semantics describe their original baseline,
unless confirmed in the current references above. This catalogue supersedes their status and protocol authority.
The independent [2025-11-13 archive](../archive/2025-11-13/README.md) remains unchanged.

| Old path/section (relative to this index) | New authority/home | Classification and unresolved coverage |
| --- | --- | --- |
| [PersistentCache design](PERSISTENTCACHE_DESIGN.md) | [Wire reference](CACHE_PROTOCOL_CURRENT.md) | Historical split design and mixed later notes; variable hashLen and compatibility matrix superseded. Measurements retained below. |
| [ContentCache design](CONTENTCACHE_DESIGN_IMPLEMENTATION.md) | Wire reference; convergence gates 2–6 | Historical removed engine; ordering, stride, thresholds and corruption evidence retained. |
| [ContentCache Rust parity plan](CONTENTCACHE_RUST_PARITY_PLAN.md) | Convergence gates 1–6 | Historical phases; May u64 checkpoint retained, later mixed INIT state now explicit. ContentKey, ARC, stats and cross-architecture requirements remain backlog evidence. |
| [PersistentCache parity plan](PERSISTENTCACHE_PARITY_PLAN.md) | Convergence gates 2–6 | Historical split-engine migration; deduplication, insert-after-encode, notifications, metrics and safety requirements retained. |
| [Rust implementation plan](../rust-vnc-viewer/PERSISTENTCACHE_IMPLEMENTATION_PLAN.md) | Convergence gates 2–6 | Historical phases A–J, pseudocode, timelines and acceptance criteria; not an executable current roadmap. |
| [Rust protocol design](../rust-vnc-viewer/docs/protocol/PERSISTENTCACHE_RUST.md) | Wire reference; convergence gates 2–6 | Historical hash arrays and “no disk store/eviction” TODOs; actual disk and eviction plumbing now exists. |
| [Rust runtime guide](../rust-vnc-viewer/docs/protocol/persistent_cache.md) | Wire reference; convergence checklist | Runtime configuration background; no evidence of current C++ interoperability. |
| [ContentCache quickstart](../rust-vnc-viewer/CONTENTCACHE_QUICKSTART.md) | Wire reference; convergence checklist | Historical target exercises; corrected ID width versus rectangle length. |
| [Removal tracker](remove_contentcache_implementation.md) | Convergence checklist | C++ engine removal present; unchecked reliable CI remains open; later 16-byte/v2 cutover supersedes earlier u64 decisions. |
| [C++ cutover resume](persistentcache_cpp_cutover_resume.md) | Convergence gate 2 | June 7, 2026 evidence: builds unavailable and SSH failed; source present today does not prove those checks subsequently passed. Old branch/amend recipes grant no authority. |
| [Deprecated design][old-design], [completion note][old-complete] | Wire reference | Historical forwarding notes. |

[old-design]: CACHE_PROTOCOL_DESIGN.md
[old-complete]: CACHE_PROTOCOL_COMPLETE.md

## Proposal lineage and delivered portions

| Original | Preferred proposal | Source-inspected status and remaining work |
| --- | --- | --- |
| [Lazy refresh](LAZY_32BIT_REFRESH.md) | [Refined design](LAZY_32BIT_REFRESH_refined.md) | Original options/history. Refined design is the planning reference; its “not implemented today” text is dated. |
| [Lazy update](LAZY_32BIT_REFRESH_update.md) | Refined design | Intermediate expanded draft. Reduced-depth tracking, native-format refresh, v2 PF transport and capability gating exist in C++; end-to-end quality/budget/safety acceptance remains unverified here. |
| [Subregion original](SUBREGION_CACHE_DESIGN.md) | [Updated proposal](SUBREGION_CACHE_DESIGN_UPDATED.md) | Updated design is the planning reference, not proof of full delivery. Its Jan 2026 “no offsets” claim is superseded by C++ source. |
| Mirror/index and eviction | Updated proposal | Partial C++ offsets; full acceptance open (details below). |

C++ has same-position containing-tile offset lookup, writer and bounds-checked replay. Full arbitrary
moved-content mirror/spatial index, coherency and performance acceptance are not established by that partial
implementation; see convergence gates 2, 4–6.

Source evidence: [EncodeManager](../common/rfb/EncodeManager.cxx) contains native-format idle refresh,
shift-tolerant scanning and `tryPersistentCacheLookup` containing-tile offset lookup.
[DecodeManager](../common/rfb/DecodeManager.cxx) bounds-checks offset replay.
[ShiftTolerantScan](../common/rfb/cache/ShiftTolerantScan.cxx) is implemented; it is distinct from the full
mirror/spatial-index proposal. Proposal wire alternatives yield to the current serializer contract.

The related DOCX files were inspected as specifications, not classified by filename:

- [Design specification](TigerVNC_Shift_Tolerant_Cache_Scan_Design_Spec.docx): multi-phase/multi-scale scanning,
  exact packed-rectangle verification, coverage safeguards, compute budgets and optional GPU prehash.
- [Implementation mapping](TigerVNC_Shift_Tolerant_Cache_Scan_Implementation_Mapping_Patch_Plan.docx):
  integration points, phased CPU/adaptive/GPU work and test plan.
- [API/parameters/logging](TigerVNC_Shift_Tolerant_Cache_Scan_API_Params_Logging_Schema.docx):
  proposed configuration and telemetry contract.
- [Developer checklist](TigerVNC_Shift_Tolerant_Cache_Scan_Developer_Checklist_Stub_Layout.docx):
  implementation checklist and suggested layout, not completion evidence.
- [January 17 update](_updated_2026-01-17.docx.docx): implementation mapping revision, with phased patch/test
  instructions. It does not certify all scan or subregion requirements as delivered.

Keep all DOCX proposals for their unique criteria; source presence establishes only the implemented portions above.
GPU acceleration remains a proposal in this audit; no GPU or runtime tests were launched.

## Evidence and requirements retained

November 2025 measurements remain historical: C++ ContentCache scenario 63–67% hits (27 hits, 13 misses),
about 300 KB saved; PersistentCache 100% hits (44 hits, 0 misses), 529,676 bytes (about 517 KB) saved,
99.7% reduction, 128×128 logos over 30 seconds, threshold 2048 pixels (formerly 4096).
Old 20/47-byte reference costs belong to those baselines, not today's 36-byte C++ REF.
No new runtime measurement or CI success is claimed.

The Rust plan's numerical acceptance criteria remain proposals: >80% reconnect hit rate, hashing >2 GB/s,
disk append >100 MB/s, memory overhead <15%, query latency <10 ms; the Rust protocol guide also proposes
800×600 hashing <1 ms, lookup <0.1 ms, and 10K-entry load/save <200 ms.
Quickstart targets (>80% hits, <2 GB memory, 100% visual correctness) remain historical targets.
All original tables, issue references, rejected alternatives and other criteria remain at the linked old anchors.

## Unresolved work and audit limits

- **A — Compatibility:** recommend a separately authorized implementation task to converge Rust and C++ wire
  identity, INIT parsing, message types and offset handling. With no further instruction, code stays unchanged
  and cache interoperability remains unverified. The brief's global u64 invariant conflicts with C++ source;
  this audit preserves u64 as the Rust baseline and records the discrepancy without changing protocol policy.
- **B — Validation:** recommend running the repository runner under the documented headless safety rules in
  a later authorized validation task. Until then, keep C++ cutover build/CI and Rust parity gates open.
- Historical backlinks remain valid because paths/headings were retained. No sibling edits are required for
  moved paths: none moved. External consumers of obsolete semantics must follow the new current reference.
- Audit validation covers edited local links/anchors and source inspection; it does not revalidate every link
  inside archived, unchanged plans or certify deployed binaries.

## Verification record

Inspected baseline: `cf1296fb5e2463fb1029ff13a9d26210d919800b` on canonical `master`;
initial checkout clean, origin `nickcrabtree/tigervnc`, safe pull already up to date.
Only documentation changed. The staged pre-commit gate passed, including Markdownlint, and staged
`git diff --check` passed. Local Markdown/reference/RST links and the linked heading anchor were checked.
Configured default stages are pre-commit, with some manual C++ hooks; no pre-push stage is configured,
no hooksPath override is set and `.git/hooks/pre-push` is absent (only its sample exists).
No runtime/build/GPU tests, service changes, deletions, commits or pushes were performed.
Touched quickstart/tracker/README formatting was corrected to satisfy existing lint; archived plans are unchanged.
