# TigerVNC Documentation Index

This repository has undergone a documentation housekeeping pass to reduce duplication and remove stale design notes. Legacy and detailed historical documents are available in git history.

Canonical docs to use now:

- README.rst — Project overview and components
- BUILDING.txt — Build requirements and instructions (CMake + Xorg server setup)
- WARP.md — Developer guidance for this repo
- CONTENTCACHE_DESIGN_IMPLEMENTATION.md — ContentCache design, protocol, and integration (canonical)
- ARC_ALGORITHM.md — Details of the ARC cache algorithm used by ContentCache
- PERSISTENTCACHE_DESIGN.md — Persistent cache design (future work)
- DEBUG_LOGGING.md — Guidance for enabling and interpreting logs
- tests/e2e/README.md and QUICKSTART.md — End-to-end testing harness
- rust-vnc-viewer/README.md — Rust viewer docs and roadmap

Note about removals

- A number of status reports, TODOs, and overlapping design drafts have been deprecated. See git history if you need the old content.
- Deprecated files now contain a short notice pointing back to this index and the canonical documents above.
