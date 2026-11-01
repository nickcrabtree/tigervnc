# TigerVNC Documentation Index

## Current cache documentation

Source/status audit: 2026-11-01. Read these before implementing or testing cache changes:

- [Current protocol reference](CACHE_PROTOCOL_CURRENT.md): C++ 16-byte CacheKey/v2 wire semantics,
  byte counts, negotiation, and the Rust u64/mixed-parser boundary.
- [C++/Rust convergence checklist](../rust-vnc-viewer/docs/CONVERGENCE_GATES.md): delivered source paths,
  historical test evidence, remaining build/CI, protocol, persistence, pixel and performance gates.
- [History and coverage map](CACHE_DOCS_HISTORY.md): archived-in-place plans, authoritative proposals,
  preserved measurements/acceptance criteria and unresolved implementation discrepancies.
- [Removal tracker](remove_contentcache_implementation.md): C++ unification decisions and incomplete CI acceptance.
- [ARC algorithm](ARC_ALGORITHM.md): cache replacement algorithm background.

The unified C++ engine is present in source. This does **not** mean the later C++ v2 cutover is build/CI-validated
or that Rust has reached parity. November 2025 “complete/tested” claims and bandwidth figures are historical
evidence, not current cross-language acceptance. Rust has cache implementations, disk I/O and notification
plumbing; protocol compatibility remains unresolved.

## Project and development

- [Project overview](../README.rst)
- [Build requirements](../BUILDING.txt)
- [Developer guidance](../WARP.md)
- [Rust viewer](../rust-vnc-viewer/README.md)
- [Development and test runner](../AGENT_GUIDE_DEVELOPMENT1.md)
- [End-to-end testing](../tests/e2e/README.md)
- [Test quickstart](../tests/e2e/QUICKSTART.md)

## Historical documents

The [cache history map](CACHE_DOCS_HISTORY.md) replaces competing cache-plan status claims while preserving
the original files and anchors. It distinguishes original/refined lazy-refresh and subregion proposals from
the portions now present in C++.

The [2025-11-13 archive](../archive/2025-11-13/README.md) retains earlier debug analyses, threshold studies,
bug evidence and implementation plans. Dated “resolved” claims do not establish current regression status.
