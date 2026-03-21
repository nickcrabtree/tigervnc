# TigerVNC Documentation Index

This repository has undergone a documentation cleanup (November 2025) to consolidate implementation documentation and archive interim work. All historical documents remain available in git history.

## Core Documentation

### Project Overview

- `README.rst` — Project overview and components
- `BUILDING.txt` — Build requirements and instructions (CMake + Xorg server setup)
- `WARP.md` — Developer guidance, test environment, and safety warnings

### Cache Protocol Implementation (Complete)

- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` — Historical ContentCache design plus notes on the current unified cache model
  - Status: ✅ C++ viewer/server now use the unified cache engine; the historical ContentCache scenario maps to the disk-free / session-only policy
  - Threshold: 2048 pixels
  - Test: `tests/e2e/test_cpp_contentcache.py` covers the historical ContentCache-named scenario of the unified cache path
- `PERSISTENTCACHE_DESIGN.md` — PersistentCache protocol and unified cache model (complete, tested)
  - Status: ✅ C++ implementation complete, 100% hit rate, 99.7% bandwidth reduction
  - Threshold: 2048 pixels
  - Test: `tests/e2e/test_cpp_persistentcache.py` passing
- `ARC_ALGORITHM.md` — Adaptive Replacement Cache algorithm used by the unified cache engine

### Testing

- `tests/e2e/README.md` — End-to-end test suite documentation
- `tests/e2e/QUICKSTART.md` — Quick start guide for running tests

### Rust Viewer (Pending)

- `rust-vnc-viewer/README.md` — Rust viewer overview
- `rust-vnc-viewer/PERSISTENTCACHE_IMPLEMENTATION_PLAN.md` — Implementation roadmap

## Archived Documentation

Interim work, debug notes, and completed plans archived to `archive/2025-11-13/`:

- Debug analyses and threshold optimization studies
- Bug fix documentation (all bugs now resolved)
- Progress tracking and status documents
- Implementation guides (superseded by canonical docs)
- ARC eviction plan and summary (implementation complete)

See `archive/2025-11-13/README.md` for full index of archived content.

## Implementation Status (November 2025)

- ✅ **Unified cache engine**: Complete and tested in C++
- ✅ **Disk-free / session-only cache policy**: Covered by the historical ContentCache-named C++ tests and docs
- ✅ **Disk-backed PersistentCache policy**: Complete and tested in C++
- ✅ **ARC Eviction**: Client-side eviction with server notifications implemented
- 🚧 **Rust Viewer**: Cache protocol parity with the current unified C++ model pending
