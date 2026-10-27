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

This fork includes two custom cache protocols providing 63-99% bandwidth reduction for repeated content:

- **session cache**: session-based cache with server-assigned IDs (20-byte references)
- **PersistentCache**: disk-backed cache with content hashes (47-byte references, survives sessions)

### Protocol Overview

- **session cache**: `CachedRect` (20 bytes, server references by cache ID), `CachedRectInit` (20 bytes + encoding, full
  data + ID for storage)
- **PersistentCache**: `PersistentCachedRect` (47 bytes, server references by content hash), `PersistentCachedRectInit`
  (47 bytes + encoding, full data + hash for storage)
