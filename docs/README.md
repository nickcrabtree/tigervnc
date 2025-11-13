# TigerVNC Documentation Index

This repository has undergone a documentation cleanup (November 2025) to consolidate implementation documentation and archive interim work. All historical documents remain available in git history.

## Core Documentation

### Project Overview
- `README.rst` — Project overview and components
- `BUILDING.txt` — Build requirements and instructions (CMake + Xorg server setup)
- `WARP.md` — Developer guidance, test environment, and safety warnings

### Cache Protocol Implementation (Complete)
- `CONTENTCACHE_DESIGN_IMPLEMENTATION.md` — ContentCache protocol (complete, tested)
  - Status: ✅ C++ implementation complete, 63-67% hit rate
  - Threshold: 2048 pixels
  - Test: `tests/e2e/test_cpp_contentcache.py` passing
- `PERSISTENTCACHE_DESIGN.md` — PersistentCache protocol (complete, tested)
  - Status: ✅ C++ implementation complete, 100% hit rate, 99.7% bandwidth reduction
  - Threshold: 2048 pixels  
  - Test: `tests/e2e/test_cpp_persistentcache.py` passing
- `ARC_ALGORITHM.md` — Adaptive Replacement Cache algorithm used by both protocols

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

✅ **ContentCache**: Complete and tested in C++  
✅ **PersistentCache**: Complete and tested in C++  
✅ **ARC Eviction**: Client-side eviction with server notifications implemented  
🚧 **Rust Viewer**: Cache protocol implementation pending
