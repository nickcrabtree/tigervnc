# Log-Driven Cache & Tiling Trace Test Plan

Status: **Design only** (no code changes yet)

Scope: C++ server (`Xnjcvnc`) + C++ viewer (`njcvncviewer`) unified ContentCache/PersistentCache engine and tiling

Audience: Future agents and maintainers implementing protocol-level tests and diagnostics

---

## 1. Background and Motivation

### 1.1 Unified cache engine and tiling

This fork of TigerVNC unifies the legacy **ContentCache** and the newer **PersistentCache** into a single cache engine keyed by a `(width, height, 64-bit contentId)` triple (represented by `CacheKey` with `contentHash`).

Conceptually:

- **ContentCache mode**
  - Session-only, no disk I/O.
  - Uses 64-bit IDs and a memory-only ARC cache.
- **PersistentCache mode**
  - Disk-backed and cross-session.
  - Uses the same 64-bit IDs on the wire, but also stores content on disk via `GlobalClientPersistentCache`.

The unified engine is exercised by both pseudo-encodings:

- `pseudoEncodingContentCache`
- `pseudoEncodingPersistentCache`

Negotiation determines *policy* (session-only vs disk-backed), but on the server side they both route through the same 64-bit ID path.

There is also ongoing work on **tiling enhancements** (see `docs/content_and_persistent_cache_tiling_enhancement.md`) so that:

- Large dirty regions are subdivided into tiles.
- Tiles with stable content are cached and reused efficiently.
- Tiles over highly detailed regions may be treated differently from constant-colour regions.

The result is a relatively complex pipeline:

- Damage tracking, tiling, and encoder selection in `EncodeManager`.
- Cache decisions on the server side (when to send INIT vs reference; when to fall back to normal encodings).
- Cache handling on the client side (DecodeManager + `GlobalClientPersistentCache`).
- Potential lossy encoders (e.g. Tight JPEG) and subsequent **lossless refresh** logic that can "heal" lossy regions over time.

### 1.2 Existing e2e tests and their limitations

The current e2e suite exercises caches via tests such as:

- `tests/e2e/test_cpp_contentcache.py` (C++ viewer ContentCache)
- `tests/e2e/test_cpp_persistentcache.py` (C++ viewer PersistentCache)
- `tests/e2e/test_cpp_cache_eviction.py` (small cache, eviction behavior)
- `tests/e2e/test_cache_eviction.py`, `tests/e2e/test_cache_parity.py`, etc.

These tests are excellent for **aggregate metrics**:

- Cache hit rate
- Bandwidth reduction
- Eviction counts

However, they are not ideal for tightly correlating individual image updates with protocol behavior. Reasons:

- The logs during these tests are very busy:
  - Desktop environment noise (clocks, panels, wallpaper changes).
  - Window-manager animations.
  - Background damage and continuous updates.
- They focus on repeated, synthetic workloads (e.g. tiled logos, image churn) rather than a **single, real-world image** whose content we care deeply about.

This makes it hard to answer questions like:

- "What did the server do *for this particular image* the first time vs the second time?"
- "Which tiles were cached, and exactly how many hits did we see when we redisplayed the same image?"
- "Did lossy encoders interfere with the hash-based cache identity we expect?"

### 1.3 Purpose of this new test

The proposed test is a **log-driven microscope** focused on a *single* real-world image.

Goals:

1. Provide a **minimal, low-noise environment** where the only interesting framebuffer changes are those the test script explicitly performs.
2. **Trace cache and tiling behavior** for a specific image through the logs:
   - On first display (initial sends / INITs).
   - On redisplay (references / hits).
   - Optionally, after clearing/replacing the image region.
3. Make it easy to **diff precise log snapshots** taken at well-defined milestones:
   - Before any content.
   - After first display.
   - After removal/re-display.
4. Enable **finer protocol-level reasoning**:
   - How many tiles were used?
   - Which tiles were served via INIT vs reference?
   - How much of the image area is actually benefiting from the cache?
   - How does this change under different encodings and cache modes?

This test is intended as a diagnostic / research tool, not just a go/no-go check.

---

## 2. High-Level Test Idea

The core idea is a **three-phase run with log snapshots**.

### 2.1 Phases

1. **Phase A – Idle baseline**
   - Start server + viewer with a minimal or no window manager, so that no clocks, panels, or other dynamic UI elements are generating updates.
   - Wait until the system is fully settled (no ongoing updates).
   - Snapshot server and viewer logs:
     - `A_server.log`
     - `A_viewer.log`

2. **Phase B – First image display**
   - Display a single test image on the server at a known location (e.g. via a helper scenario in `scenarios_static.py`).
   - Wait for the update to be fully propagated to the viewer and for activity to quiet down.
   - Snapshot logs again:
     - `B_server.log`
     - `B_viewer.log`

3. **Phase C – Remove/replace and redisplay**
   - Remove or cover the image region (e.g. close the image window, show a blank background).
   - Optionally wait for caches to adjust.
   - Redisplay the **same** image at the same coordinates.
   - Again wait for activity to quiet down.
   - Snapshot logs:
     - `C_server.log`
     - `C_viewer.log`

### 2.2 Diffing behavior

Once the snapshots are captured:

- `A → B` diff shows what the **initial display** did:
  - How many tiles were sent.
  - How many INIT messages were emitted.
  - Which parts of the image caused which cache protocol traffic.

- `B → C` diff shows what the **redisplay** did:
  - How many hits/references we got vs new INITs.
  - Whether the cache protocol behaves as expected (high re-use on repetition).

Because the image is known and the environment is intentionally quiet, we gain a clear mapping between log entries and the behavior of the cache/tiling protocols.

---

## 3. Environment and Safety Constraints

### 3.1 Displays and ports

The test must respect the safety rules encoded in `WARP.md` and the test harness:

- Only use the dedicated test displays:
  - `:998`
  - `:999`
- Only use the corresponding ports:
  - `6898` (for `:998`)
  - `6899` (for `:999`)
- Never interact with production displays:
  - `:1`
  - `:2`
  - `:3`

The existing `tests/e2e/framework.py` already provides helpers that encode these conventions:

- `VNCServer(display, port, name, artifacts, tracker, ...)`
- `ProcessTracker` for process cleanup.
- `check_port_available`, `check_display_available`.
- `best_effort_cleanup_test_server(display, port, verbose)` to clean up stale test servers.

**Future agents should reuse these helpers** rather than rolling new process management code.

### 3.2 Window manager and desktop noise

To get meaningful diffs, we must ensure **no extraneous updates** beyond the ones explicitly triggered by the test.

Guidelines:

- Prefer **no window manager** if the content scenario can work without it.
- If a WM is required (to easily spawn an image viewer), use the simplest available, e.g. `openbox`, and configure it minimally:
  - No status panels.
  - No compositing.
  - No clock widgets.
  - No animated wallpapers.
- Disable screen savers and DPMS where possible, or keep the test short enough that they do not activate.

The typical e2e pattern is:

- Content display `:998`:
  - X/VNC server.
  - Optionally `openbox` as WM.
- Viewer window display `:999`:
  - X/VNC server running a minimal desktop, again with `openbox` at most.

### 3.3 Viewer configuration

The test should start `njcvncviewer` with explicit parameters to avoid ambiguity. Example dimensions:

- **Encoding**:
  - For initial work, choose a single `PreferredEncoding`, e.g. `ZRLE` or `Tight` with fixed quality.
  - Later, sweep over multiple encodings to test different behaviors.

- **Cache modes**:
  - **ContentCache only**:
    - Server: `-EnablePersistentCache=0` (and default/explicit ContentCache parameters).
    - Viewer: `ContentCacheSize=...`, `PersistentCache=0`.
  - **PersistentCache only**:
    - Server: `-EnableContentCache=0`, `-EnablePersistentCache=1`.
    - Viewer: `ContentCache=0`, `PersistentCache=1`, plus a test-local `PersistentCachePath`.
  - **Unified (both)**:
    - Server and viewer with both encodings enabled; negotiation determines which is actually used.

- **Logging**:
  - For the viewer, use `Log=*:stderr:100` to keep a rich trace, but rely primarily on new structured debug lines (see next section) to interpret behavior.

- **PersistentCache path**:
  - For tests involving disk persistence, set `PersistentCachePath` inside the test artifacts directory so that it is:
    - Isolated from the user's real cache.
    - Disposable and easy to inspect.

---

## 4. Logging Design (Env-Gated, Structured)

### 4.1 Why we need structured debug logging

We want logs that are:

- **Stable** across runs (modulo timestamps).
- **Machine-parsable** with minimal regex logic.
- **Narrow in scope**: they should record only cache/tiling events relevant to this test, and only when explicitly enabled.

To achieve this:

- All extra logging introduced for this test must be **gated behind environment variables**.
- Logging should consist of **single-line, structured messages** with a clear prefix (e.g. `CCDBG`) and well-defined fields.

### 4.2 Environment variables

Proposed environment variables:

- `TIGERVNC_CC_TEST_LOG=1`
  - When set, enables high-signal cache/tiling test logging on both server and client.

In the future we could split this into server/client-specific vars if necessary, but a single switch is sufficient for initial implementation.

### 4.3 Server-side logging hook points

Likely insertion points (**do not implement yet; this is for guidance**):

- `common/rfb/EncodeManager.cxx`:
  - Inside `writeRects()` / `writeSubRect()` when determining how a rect is tiled and which encoder is used.
  - Inside `tryPersistentCacheLookup()` (or its unified equivalent) when deciding between INIT vs reference.

- `common/rfb/VNCSConnectionST.cxx`:
  - Where targeted refresh is triggered by `handleRequestCachedData`.

Suggested log formats (examples):

- **Tile / rect encoding**:

  ```
  CCDBG SERVER RECT: rect=[x1,y1-x2,y2] enc=<encoderName> cacheMode=<none|init|hit> cacheId=<u64_or_0>
  ```

- **Cache init**:

  ```
  CCDBG SERVER CACHE_INIT: rect=[x1,y1-x2,y2] cacheId=<id>
  ```

- **Cache hit (reference)**:

  ```
  CCDBG SERVER CACHE_HIT: rect=[x1,y1-x2,y2] cacheId=<id> savedBytes=<approx>
  ```

Where:

- `rect=[x,y-x,y]` matches patterns already used in `tests/e2e/log_parser.py` (which can parse coordinate forms like `[x,y-x,y]`).
- `cacheId` can be decimal or hex, as long as it’s consistent.

### 4.4 Client-side logging hook points

Likely insertion points:

- `common/rfb/CMsgReader.cxx`:
  - Already logs for `CachedRect` / `CachedRectInit` / `PersistentCachedRect` / `PersistentCachedRectInit`.

- `common/rfb/DecodeManager.cxx`:
  - When processing hits/misses in `handlePersistentCachedRect`.
  - When storing INITs in `storePersistentCachedRect`.

- `common/rfb/CConnection.cxx`:
  - Where the negotiated cache protocol is recorded.

Suggested log formats (examples):

- **Client receives INIT**:

  ```
  CCDBG CLIENT INIT: rect=[x1,y1-x2,y2] cacheId=<id> encoding=<innerEncoding>
  ```

- **Client receives reference**:

  ```
  CCDBG CLIENT REF: rect=[x1,y1-x2,y2] cacheId=<id>
  ```

- **Client cache decision**:

  ```
  CCDBG CLIENT CACHE_HIT: rect=[x1,y1-x2,y2] cacheId=<id>
  CCDBG CLIENT CACHE_MISS: cacheId=<id> action=RequestCachedData
  ```

All of these must be emitted **only** when `TIGERVNC_CC_TEST_LOG` is set, to avoid polluting normal logs.

---

## 5. Test Scenario: Control Flow

### 5.1 Test file and structure

The new test can be added under `tests/e2e` as, for example:

- `tests/e2e/test_cpp_cache_trace_single_image.py`

High-level steps inside `main()` or equivalent:

1. **Artifacts and preflight**
   - Create an `ArtifactManager()` and call `artifacts.create()`.
   - Use `preflight_check_cpp_only()` (or `preflight_check()` if server choice is flexible) to locate binaries.
   - Use `check_port_available(6898)`, `check_port_available(6899)`, `check_display_available(998)`, and `check_display_available(999)` to ensure nothing is already using the dedicated test ports/displays.

2. **Start content and viewer servers**
   - Create a `ProcessTracker()`.
   - Start the **content server** on display `:998`, port `6898` via `VNCServer`:
     - Geometry: e.g. `1920x1080`.
     - Log level: `*:stderr:100` to see cache logs.
     - `server_choice` set to `local` if Xnjcvnc is built, otherwise `system` (Xtigervnc) as fallback.
   - Start the **viewer window server** on display `:999`, port `6899` similarly.
   - Start sessions with a minimal WM (`openbox`) if needed.

3. **Start viewer**
   - Resolve viewer path (C++ viewer binary) via preflight results or `BUILD_DIR/vncviewer/njcvncviewer`.
   - Set up environment:
     - `DISPLAY=:999` so the viewer runs on the viewer window server.
     - `TIGERVNC_CC_TEST_LOG=1` for the special logging.
   - Launch viewer with:
     - `127.0.0.1::6898` as the target.
     - `Shared=1`.
     - `Log=*:stderr:100`.
     - Cache parameters depending on the mode you’re testing, e.g.:
       - ContentCache only: `ContentCacheSize=256`, `PersistentCache=0`.
       - PersistentCache only: `ContentCache=0`, `PersistentCache=1`, `PersistentCachePath=...`.
     - Fixed `PreferredEncoding`, e.g. `PreferredEncoding=ZRLE`.

4. **Phase A: Idle baseline**
   - Wait a short time (e.g. 2–3 seconds) for the viewer to connect and for any initial handshakes to complete.
   - Snapshot logs:
     - Copy or rename the content server log (e.g. `cpp_trace_content_server_998.log`) to `A_content.log`.
     - Copy or rename the viewer log (e.g. `trace_viewer.log`) to `A_viewer.log`.

5. **Phase B: First display**
   - Use a scenario helper (proposed below) to display the test image once on `:998`:
     - E.g. a helper function `display_single_image(image_path, display_num)` that spawns an image viewer.
   - Wait until:
     - The viewer process is still alive (no crash).
     - Either a fixed timeout has expired (e.g. 5 seconds) or a heuristic indicates quiescence (no new `CCDBG` lines for a short period).
   - Snapshot logs again to `B_content.log` and `B_viewer.log`.

6. **Phase C: Remove/replace and redisplay**
   - Close or hide the image window(s) from Phase B.
   - Optionally show a blank window to ensure the region is cleared.
   - Redisplay the **same** image at the same coordinates (idempotency is important).
   - Again wait for quiescence.
   - Snapshot logs as `C_content.log` and `C_viewer.log`.

7. **Analysis**
   - Parse the `CCDBG` lines from each of the six log files.
   - Construct in-memory representations of:
     - Which rects were INITed vs referenced.
     - For each cache ID, how many INITs and refs occurred in each phase.
   - For the first version of the test, assert **only coarse properties** (Section 7 below).

8. **Cleanup**
   - Call `tracker.cleanup_all()` to terminate any remaining viewer and server processes.
   - Rely on `best_effort_cleanup_test_server()` behavior embedded in `check_port_available` or call it explicitly if needed.

### 5.2 Scenario helper: displaying a single image

Implementation detail (for future agents; do not implement here):

- Likely extend `tests/e2e/scenarios_static.py` with a helper like:

  - `static_single_image(image_path: Path, display: int, duration_sec: float) -> dict`

High-level behavior:

- Spawns an image viewer (e.g. `feh`, `display`, or a tiny custom tool) on `DISPLAY=":<display>"`.
- Positions the window predictably (via geometry flags or `wmctrl`).
- Keeps the window open for at least `duration_sec` or until the test decides to close it.

Constraints:

- Must be deterministic: no slideshows, no animation.
- Must ensure the image is fully visible on the framebuffer.

The same helper can be reused in Phases B and C.

---

## 6. Test Image Fixtures

### 6.1 Location and format

The image should be stored in a dedicated fixtures directory, e.g.:

- `tests/e2e/fixtures/cache_trace/realworld_logo.png`

Requirements:

- Use a **lossless** format (PNG preferred) so server-side pixels are well-defined.
- Resolution should be large enough to make tiling interesting, e.g.:
  - 512×512
  - 640×360
  - 800×600

### 6.2 Content properties

The image should be chosen to stress the cache and tiling logic:

- **Constant-colour regions**:
  - Large, exactly constant blocks (or at least extremely low variation).
  - Good for verifying that tiles over uniform areas are cheap and easily cached.

- **High-detail regions**:
  - Text, gradients, photos, or noise textures.
  - These regions are harder to compress and may not gain much from tiling.

- **Mixed layout**:
  - Distinct, easily-referenced regions (e.g. a logo on a flat background, with some detailed sidebars) so we can visually map log-reported rects/tiles to subareas of the image.

A small `README.md` next to the image (e.g. `tests/e2e/fixtures/cache_trace/README.md`) should document:

- Which areas of the image are constant vs detailed.
- Which regions are particularly relevant to the tiling redesign described in `content_and_persistent_cache_tiling_enhancement.md`.

---

## 7. Metrics and Expected Behavior

Initially, the test should focus on **observational** and **sanity** assertions, not tight numeric thresholds. Once the basic test is stable, we can refine expectations.

### 7.1 Basic invariants

For a given cache mode (e.g. ContentCache-only or PersistentCache-only):

- **Phase A (idle baseline)**
  - There should be **no** `CCDBG` cache INIT/REF entries in `A_server.log` / `A_viewer.log`, or at least a very small, easily explained number (depending on how the viewer initializes).

- **A → B (first display)**
  - On the server:
    - Number of `CACHE_INIT`-style events (INITs) for this image’s rectangles should be **> 0**.
  - On the client:
    - Number of `CLIENT INIT`-style events should be **> 0**.
  - Number of cache references (HITs) may be small or zero on the very first appearance; that’s expected.

- **B → C (redisplay)**
  - The number of cache references on redisplay should be **≥** references observed on first display.
  - The number of INITs on redisplay should be **≤** the number on first display (strictly less is ideal if caching is working well).

The test can enforce these by parsing structured log events, e.g.:

- Count `CCDBG SERVER CACHE_INIT` vs `CCDBG SERVER CACHE_HIT` in each phase.
- Count `CCDBG CLIENT INIT` vs `CCDBG CLIENT REF` in each phase.

### 7.2 Longer-term expectations

Once the test is in place and stable, future work could:

- Break down behavior by subregion of the image:
  - E.g., constant background area vs detailed text area.
- Compare across modes:
  - No cache vs ContentCache vs PersistentCache.
- Cross-check with other metrics:
  - Bandwidth stats from `EncodeManager`.
  - Viewer-side bandwidth summaries if those are available.

But for the **initial implementation**, avoid over-fitting expectations. Focus on:

- "INIT > 0 on first display".
- "More hits on second display than first".
- "Fewer INITs on second display than first".

---

## 8. Test Variants and Matrix

Once the basic test harness is in place, it should be easy to add variants that run the same scenario under different configurations.

Recommended variants:

1. **No caches**:
   - Server: `-EnableContentCache=0`, `-EnablePersistentCache=0`.
   - Viewer: `ContentCache=0`, `PersistentCache=0`.
   - Purpose: baseline behavior of encoders without any cache.

2. **ContentCache only**:
   - Server: `-EnablePersistentCache=0` (ContentCache still enabled with its default/minimum thresholds).
   - Viewer: `ContentCacheSize > 0`, `PersistentCache=0`.
   - Purpose: measure session-only caching behavior for the single image.

3. **PersistentCache only**:
   - Server: `-EnableContentCache=0`, `-EnablePersistentCache=1`.
   - Viewer: `ContentCache=0`, `PersistentCache=1`, `PersistentCachePath=<test-dir>`.
   - Purpose: measure disk-backed caching and reuse within one run; can be extended to multi-run scenarios later.

4. **Unified (both caches negotiated)**:
   - Server and viewer both advertise ContentCache and PersistentCache encodings.
   - Negotiation and the unified engine determine which path is used, but IDs and behaviors should remain consistent.

The same script (`test_cpp_cache_trace_single_image.py`) can accept arguments or environment variables to select the variant.

---

## 9. Files and Components for Future Agents

This section summarizes where future code changes are likely to go, **without** prescribing exact implementations.

### 9.1 Test harness and scenarios

- **New test file** (suggested):
  - `tests/e2e/test_cpp_cache_trace_single_image.py`
  - Responsibilities:
    - Set up artifacts and trackers.
    - Start/stop servers and viewer.
    - Coordinate phases A/B/C and log snapshots.
    - Parse structured CCDBG lines and make assertions.

- **Scenario helper** (in `tests/e2e/scenarios_static.py` or similar):
  - Adds a function to display a single image window on the content display.
  - Might be named something like `tiled_single_image()` or `display_static_image()`.

### 9.2 Logging additions

Code-level logging hooks should be added **carefully and minimally** to:

- `common/rfb/EncodeManager.cxx` (server):
  - When encoding/tile decisions are made.
  - When cache lookups succeed or miss (INIT vs HIT).

- `common/rfb/DecodeManager.cxx` (client):
  - When references/INITs are handled.
  - When hits/misses occur in the client cache.

- `common/rfb/CConnection.cxx` (client):
  - For high-level protocol decisions (which cache protocol was negotiated, etc.).

All such logging must be guarded by `TIGERVNC_CC_TEST_LOG`.

### 9.3 Fixtures

- A new directory for fixtures:
  - `tests/e2e/fixtures/cache_trace/`

- At minimum:
  - `realworld_logo.png` (or similar real-world image).
  - `README.md` describing the image and what to look for.

---

## 10. Relationship to Existing Tests

This test is meant to **complement**, not replace, existing e2e tests:

- Existing tests (ContentCache bandwidth, PersistentCache bandwidth, evictions, parity) are focused on:
  - Long-running scenarios.
  - Aggregate metrics.
  - Stressing cache sizes and eviction behavior.

- The new test is focused on:
  - A single, real-world image.
  - Precise log-level tracing tied to that image.
  - Observing cache/tiling decisions under microscope-like conditions.

When implementing this test, future agents should:

- Reuse the e2e harness (`framework.py`, `scenarios_static.py`, `ProcessTracker`).
- Avoid duplicating logic for process management, port checks, etc.
- Keep the test’s logic and logging as orthogonal and optional as possible.

---

## 11. Summary

This document lays out a detailed plan for a **log-driven, image-centric cache trace test** that:

- Runs in a controlled, minimal environment using existing e2e infrastructure.
- Uses env-gated, structured logging to observe cache and tiling decisions for a single image.
- Captures snapshots of server and viewer logs before and after scripted framebuffer changes.
- Enables future developers to reason precisely about how the unified ContentCache/PersistentCache engine and tiling behave on challenging, real-world content.

Key takeaways for future agents implementing this test:

- Respect test environment safety: use only `:998`/`:999` and `6898`/`6899`.
- Keep desktop noise to a minimum: simplest possible WM; no clocks or animation.
- Gate all new diagnostics behind environment variables (e.g. `TIGERVNC_CC_TEST_LOG`).
- Use one-line, structured log messages to make parsing and diffing straightforward.
- Start with coarse assertions (more cache hits on second display than first) before tightening expectations.

With the above constraints observed, this test can serve as a powerful debugging and validation tool for the cache and tiling work in this fork of TigerVNC.
