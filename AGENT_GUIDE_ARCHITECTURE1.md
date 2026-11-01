# Agent guide: architecture1

Read when selected by AGENTS.md, not at every startup. Paths and commands are
relative to the repository root. Detailed source sections are retained below.
Current AGENTS.md and shared policy supersede dated history: old quiet-window
times, expired governor lifts, amend/reset advice and older host assumptions do
not grant permission. For current quiet-window status consult
`~/scripts/cross_repo_notes/AGENT_POLICY.md`.

## Detailed reference

## Code Architecture

TigerVNC is organized as layered libraries with platform-specific components.

### Platform-Specific Components

- **vncviewer/**: Cross-platform VNC viewer (FLTK-based GUI, produces njcvncviewer)
- **unix/**: Unix/Linux server components (x0vncserver, vncpasswd, vncconfig, xserver patches)
- **win/**: Windows server components (winvnc - unmaintained)
- **java/**: Java-based VNC viewer (optional, requires `-DBUILD_JAVA=ON`)

## Important Code Patterns and Gotchas

### PixelBuffer Access Patterns

```cpp
// Read-only
const uint8_t* buffer = pb->getBuffer(rect, &stride);

// Read-write — must call commitBufferRW when done!
uint8_t* buffer = pb->getBufferRW(rect, &stride);
// Modify buffer...
pb->commitBufferRW(rect);
```

Stride determines row traversal:

```cpp
for (int y = 0; y < height; y++) {
    const uint8_t* row = buffer + (y * stride * bytesPerPixel);
    for (int x = 0; x < width; x++) {
        const uint8_t* pixel = row + (x * bytesPerPixel);
        // Process pixel...
    }
}
```

## Cache and PersistentCache Implementation

Start with [current cache wire semantics](docs/CACHE_PROTOCOL_CURRENT.md) and the
[C++/Rust status checklist](rust-vnc-viewer/docs/CONVERGENCE_GATES.md).
C++ uses one PersistentCache-centred engine with memory-only or disk-backed viewer policy.
Rust retains separate paths and mixed wire assumptions; parity remains open.
The older 63–99% bandwidth measurements are dated evidence in the
[history map](docs/CACHE_DOCS_HISTORY.md), not current validation.

### Protocol Overview

- **C++ REF**: 12-byte rectangle header + 16-byte CacheKey + 8-byte offset/dimensions = 36 bytes.
- **C++ INIT v2**: header + key + mandatory flags + inner encoding = 33 bytes before pixels;
  add 16 bytes when canonical PixelFormat is carried. Seed is 28 bytes.
- **Rust REF/seed**: u64 IDs, 20 bytes including rectangle header. INIT reads 16 key bytes but keeps only
  the high u64 and uses legacy/v2 heuristics. See the current reference for other message discrepancies.
