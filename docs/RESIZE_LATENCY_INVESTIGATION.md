# Resize-Induced Latency Investigation (Xnjcvnc, November 30 2025)

## Context and Symptom

User-observed bug:

- Environment: custom Xnjcvnc server on display `:2` (production-ish desktop) running from a *slightly older* git commit than current `HEAD` (`df58798`).
- Symptom: when the user resizes the remote desktop to fit an iPad (down from a full 3840-wide desktop to ~1280 wide), they start experiencing **persistent input latency** (notably a lag between typing and text appearing), even though:
  - The original full-resolution 3840-wide session was fully responsive on the iPad.
  - The new resolution is smaller (e.g. 1280x1024), so naive "too many pixels" explanations do *not* match the observed behaviour.

Initial inspection of the `:2` server log (`~/.config/tigervnc/quartz:2.log`) around the time of the lag showed:

- Frequent `EncodeManager: CC doUpdate begin` lines with large "changed" regions:
  - Examples included `bbox:(0,15 1280x1009) rects:182` (approximate) and similar.
- That suggests the encoder believes **most of the framebuffer is dirty**, split into a large number of rects, after the resize.
- No explicit congestion or error messages were visible; only benign warnings like `Xlib:  extension "DPMS" missing on display ":2.0".`

Hypothesis space considered:

- Encoder/damage tracking could be over-reporting dirty regions after a framebuffer size change.
- Congestion control (`rfb::Congestion` + `VNCSConnectionST::isCongested()`) could be gating updates in a way that causes visible input-to-display lag.
- Differences in WM/compositor or resolution change path (`xrandr` usage) might be triggering a bad interaction.

## Code Areas Touched

Files inspected and/or modified during the session:

- `common/rfb/EncodeManager.cxx`
  - Confirmed existing logging around `doUpdate()` and framebuffer size changes (lines with `"Framebuffer size changed from ... to ..."`).
  - Confirmed that after the ContentCache unification, the server still tracks `lastFramebufferRect` and logs size changes but no longer directly clears a server-side ContentCache.
- `common/rfb/Congestion.[ch]`
  - Reviewed congestion window logic and RTT-based bandwidth estimation.
- `common/rfb/VNCSConnectionST.cxx`
  - Reviewed `writeRTTPing()` and `isCongested()`, including the use of `congestion.getUncongestedETA()` and fences.
- e2e framework and scenarios:
  - `tests/e2e/framework.py`
  - `tests/e2e/run_contentcache_test.py`
  - `tests/e2e/scenarios.py`
  - `tests/e2e/scenarios_static.py`
  - `tests/e2e/test_cpp_contentcache.py`

New file created:

- `tests/e2e/run_resize_latency_test.py` — **ad hoc resize latency debug harness** (see below).

These changes are **not** yet turned into a formal test with pass/fail criteria; they are debugging utilities.

## New e2e Harness: `tests/e2e/run_resize_latency_test.py`

### Purpose

Provide a reproducible, disposable environment to:

- Run a local Xnjcvnc server on a high display (`:998`) with controlled geometry.
- Attach a C++ viewer (njcvncviewer) via a separate viewer-window X server (`:999`).
- Apply `xrandr` framebuffer resizes inside the content server display.
- Drive real, repeated content both **before** and **after** the resize (tiled logos).
- Parse the server log after the last framebuffer size change to characterise:
  - Bounding-box sizes of dirty regions.
  - Rectangle counts per update.
  - Whether "full-FB" dirty regions appear repeatedly.

### Key Behaviour

- Preflight:
  - Uses `preflight_check_cpp_only()` to ensure required tools (
    `Xtigervnc`, `xterm`, `openbox`, `wmctrl`, `xdotool`, etc.) are present and that the **C++ viewer** exists or gets built.
  - Verifies ports and displays for the e2e environment:
    - Content server: `:998` / port `6898`.
    - Viewer-window server: `:999` / port `6899`.

- **Critically, the harness requires the local Xnjcvnc server build and will not fall back to system Xtigervnc**:
  - Checks for `build/unix/vncserver/Xnjcvnc` or `build/unix/xserver/hw/vnc/Xnjcvnc`.
  - If found: uses `server_choice='local'` for `VNCServer`.
  - If *not* found: prints a failure message and exits; no system Xtigervnc fallback is used.

- Server startup:
  - Content server: `VNCServer(display=998, port=6898, geometry=<initial>, depth=24)` using local Xnjcvnc.
  - Viewer-window server: `VNCServer(display=999, port=6899, geometry=<initial>, depth=24)` using local Xnjcvnc.
  - Both run openbox as the WM by default (`--wm` argument).

- Viewer startup:
  - External C++ viewer: `build/vncviewer/njcvncviewer`.
  - Runs on display `:999`, connects to `127.0.0.1::6898` (content server).
  - Launched with default parameters: `Shared=1`, `Log=*:stderr:100`.

- Content generation (before and after resize):
  - Uses `StaticScenarioRunner` from `tests/e2e/scenarios_static.py`.
  - Runs `tiled_logos_test(tiles=8, duration=8.0, delay_between=1.0)` on the content server display (`:998`) both:
    - **Before** the framebuffer resize.
    - **After** the resize + idle period.

- Resize:
  - Applied inside the content server display (`:998`) via `VNCServer.run_in_display()`:

    - **Current hard-coded command**:

      ```bash
      date; xrandr --output VNC-0 --fb 1366x1024 || echo 'xrandr failed'; sleep <duration>; date
      ```

    - Earlier in the session this was parameterized (`--geometry-resized`), but it is now fixed at 1366x1024 to match the user's real-world scenario.

- Log parsing and metrics:

  - After teardown, the harness inspects the content server log at:

    ```
    tests/e2e/_artifacts/<timestamp>/logs/resize_content_server_998.log
    ```

  - `parse_post_resize_updates()` does the following:
    - Finds the **last** `EncodeManager: Framebuffer size changed from [..] to [..]` line.
    - Extracts the new framebuffer width/height from the `to [x,y-X,Y]` range.
    - Scans all subsequent `EncodeManager: CC doUpdate begin` lines.
    - For each such line, parses:
      - Bbox width/height from `bbox:(x,y wxh)`.
      - `rects:N` count.
    - Aggregates metrics:
      - `fb_width`, `fb_height` (new framebuffer size).
      - `num_updates` (how many CC updates parsed after the size change).
      - `max_rects` (maximum `rects:` seen in a single update).
      - `num_large_rects` (count of updates whose bbox covers most of the FB: width >= 90% of `fb_width` and height >= 80% of `fb_height`).
    - Prints these metrics and the first few matching update lines for manual inspection.

## What We Actually Observed in the Harness

### Server build identification

- From the resize harness logs:
  - Banner line: `Xvnc TigerVNC 1.15.80+build.5744.df58798 - built Nov 23 2025 19:25:50`.
  - This matches git commit `df58798` (current `HEAD` at time of session):
    - `df587980 (HEAD -> master) Unify ContentCache on new cache engine and require viewer ContentCache stats`.
  - Therefore, the e2e harness is exercising **the current local Xnjcvnc build**, not a distro Xtigervnc.

### First attempts (1920x1080 → 1366x1024)

- Initial tests used an initial geometry of 1920x1080 and a resize to 1366x1024 (via `xrandr --fb`), with pre/post tiled logos.
- Post-resize metrics (approximate example from one run):
  - `Framebuffer size changed from [0,0-1918,1040] to [...]`.
  - `Updates parsed  : 9`.
  - `Max rects/update: 8`.
  - `Large full-FB-ish updates: 0`.
- `CC doUpdate` lines showed:
  - A small number of updates with bboxes like `0,0 1366x1024` or `100,100 1180x924`, but with **very low rect counts** (1–8).
  - No persistent flood of hundreds of rects.

### Realistic test 1: 3840x2100 → 1280x1024

- Updated harness to start the content server at 3840x2100 (to match the user's `:2` full desktop), still resizing via `--fb 1280x1024` initially.
- Pre/post tiled logos again around the resize.
- Observed metrics (summarised):
  - `Framebuffer size changed from [0,0-3838,2060] to [...]`.
  - `Updates parsed  : 14`.
  - `Max rects/update: 6`.
  - `Large full-FB-ish updates: 0`.
- First `CC doUpdate` after resize often had `bbox:(0,0 1280x1024) rects:1`, followed by some smaller or medium-sized bboxes.
- Still **no** evidence of the "huge rect count" behaviour the user sees in the `:2` logs.

### Realistic test 2: 3840x2100 → **exact** 1366x1024

- Per user request, the harness now hardcodes `xrandr --output VNC-0 --fb 1366x1024` as the target resize.
- Ran with initial geometry 3840x2100, pre/post 8-logo tiled scenarios, and the 20 second idle after resize.

Result (from `_artifacts/20251130_102156`):

- Framebuffer size change logged as:
  - `EncodeManager: Framebuffer size changed from [0,0-3838,2060] to [...]`.
- Post-resize `CC doUpdate` examples included:
  - `changed bbox:(0,0 1366x1024) rects:1, copied`.
  - Several updates with:
    - `bbox:(100,100 99x123) rects:1`.
    - `bbox:(100,100 672x123) rects:4`.
    - `bbox:(673,100 672x123) rects:4`.
    - `bbox:(1246,100 120x123) rects:4`.
    - `bbox:(100,101 1266x923) rects:2`.
    - etc.
- Aggregated metrics:
  - `Updates parsed  : 10`.
  - `Max rects/update: 8`.
  - `Large full-FB-ish updates: 0`.

Again, this is **not** the problematic pattern from the real `:2` environment (where we see full-width, tall bboxes and rect counts well into the hundreds, repeatedly).

### Later runs: XFCE, caches disabled, and typing load

Subsequent work extended the harness and experiments along several axes:

- **WM / session alignment**:
  - Added `--use-xstartup` support so the content server on `:998` can start the
    same XFCE session as `:2` via `~/.config/tigervnc/xstartup` instead of
    openbox-only.
- **Cache protocol alignment**:
  - Forced the C++ viewer under test to use `ContentCache=0` and
    `PersistentCache=0` so that it does **not** negotiate any cache
    pseudo-encodings, matching the behaviour of the iPad client (which does
    not implement these protocols).
  - Started the content server on `:998` with `EnablePersistentCache=0` so only
    session-local ContentCache remains, and even that is effectively inert when
    the viewer does not negotiate it.
- **More realistic content**:
  - Replaced the tiled-logo scenario with desktop-activity scenarios in
    `tests/e2e/scenarios.py`, including:
    - `eviction_stress`: many xterms at varying positions with unique content.
    - `typing_stress`: an xterm running an internal Python script that prints
      characters with small gaps and logs per-character timestamps to
      `/tmp/typing_stress_<display>.log`.
- **Client-driven typing vs. resize order**:
  - The harness now explicitly drives a typing phase **before** the
    framebuffer resize, applies the resize via an xterm running
    `xrandr --output VNC-0 --fb 1366x1024`, then runs another typing phase
    **after** the resize.

Even with all of the above (XFCE session, caches effectively disabled,
interactive-style typing before and after resize), the content server on `:998`
continues to behave "well" in logs:

- Post-resize `CC doUpdate` metrics stay low:
  - `max_rects/update` observed in these runs has been on the order of 2–8,
    occasionally up to ~30–40 for brief periods, but **not** the sustained
    80–150+ range seen on `:2`.
  - `num_large_rects` remains 0 (no repeated full-FB-ish updates).
- Typing appears visually smooth in the C++ viewer when connected to `:998`
  under the harness.

This further reinforces that the pathological pattern (and associated input
latency) is not reproduced by any of the local C++ viewer scenarios against the
Xnjcvnc test server, even when the WM, cache settings and basic typing
characteristics are aligned with the `:2` environment.

## Interim Conclusions

- Under the controlled e2e environment (displays :998/:999, openbox WM, local Xnjcvnc at commit `df58798`, pre/post tiled logos, and resolutions 1920→1366 or 3840→1366), the server behaves **well** after a framebuffer resize:
  - Dirty regions are reasonable in size.
  - Rect counts per update are low (max ~8 so far).
  - There are no repeated full-FB-ish dirty rectangles, and thus nothing in the logs suggests a pathological encoder/damage behaviour.
- This implies that the severe input latency and "lots of rects" behaviour seen
  on `:2` is likely driven by factors not yet mirrored in the harness, such as:
  - The **window manager/compositor and desktop session** running on :2 (e.g.
    XFCE panels, compositing, per-monitor scaling), though XFCE-on-:998 tests
    have so far looked healthy.
  - The exact **client configuration** on the **iPad viewer** (encoding
    preferences, quality, scaling, continuous updates, aggressive frame pacing,
    etc.).
  - The fact that `:2` has historically run a *slightly older* commit than
    `df58798` (although more recent tests with the dev server on :2 and
    current HEAD still show the bad pattern when the iPad is used as client).
  - Potential interactions between frequent compositing/animations and damage
    tracking after a mode set.
  - **Most importantly:** the problem currently reproduces *only* when the
    iPad viewer is the client. Connecting with the custom C++ desktop viewer
    (njcvncviewer) to the same server and desktop does **not** show the severe
    latency or the pathological rect pattern. This strongly suggests a
    client-specific trigger (e.g. encodings, update request patterns,
    continuous updates + fences behaviour) rather than a purely
    server-internal bug that is independent of the viewer.

## Recommended Next Steps for a Future Agent

### 1. Align the WM/session with display :2

Goal: reproduce the large-rect, high-rect-count behaviour under the harness so fixes can be validated quickly.

Suggestions:

- Identify which WM/desktop session is used on `:2` (likely from user's `xstartup` / config in `~/.config/tigervnc/xstartup` or similar).
- Either:
  - Modify `VNCServer.start_session()` for this debug harness to run the same WM/desktop stack; or
  - Add an option to `run_resize_latency_test.py` (e.g. `--wm xfce4-session` or `--use-xstartup`) and call the user's production `xstartup` script in the e2e session.
- Once the harness is running the same WM/compositor, repeat the 3840→1366 test and re-check post-resize rect metrics.

### 2. Match the exact server build from :2

- The production-like `:2` server is known to be on a *slightly older* commit than `df58798`.
- Create a separate build directory for that commit, e.g.:
  - `git checkout <older-commit>`
  - `cmake -S . -B build-old ...`
  - `make -C build-old server viewer`
- Run the harness with `BUILD_DIR=build-old` so that `VNCServer` and the C++ viewer use the exact same binaries as `:2`.
- Check if the problematic pattern (large `rects:` counts after resize) appears once the commit matches.

### 3. Investigate congestion control once reproduced

If/when the harness environment finally reproduces the `:2`-style logs (full-FB bboxes with hundreds of rects, persistent updates), then:

- Temporarily add an environment-gated override in `VNCSConnectionST::isCongested()` (example design):

  - Early return `false` if `TIGERVNC_DISABLE_CONGESTION=1`, plus an info log.
  - Rebuild Xnjcvnc and re-run the harness with `TIGERVNC_DISABLE_CONGESTION=1` set in the server environment.
- Compare:
  - Rect metrics and log patterns with and without congestion disabled.
  - Observed latency (e.g. via an automated typing scenario in `scenarios.py`) to see if disabling congestion materially reduces input lag.

If disabling congestion solves the lag:

- The fix will likely live in `rfb::Congestion` / `VNCSConnectionST::isCongested()`:
  - Avoid over-conservative ETA estimates when there are few pings or after a mode set.
  - Possibly reset congestion state when framebuffer size changes significantly (to re-measure RTT under the new content regime).

If disabling congestion **does not** change latency but the rect pattern is bad:

- Focus on damage/update tracking:
  - `common/rfb/UpdateTracker` (not yet specifically inspected this session).
  - `VNCSConnectionST::writeDataUpdate()` / `updates.getUpdateInfo()`.
  - Interactions with the WM/compositor or frequent full-screen redraws.

### 4. Turn the harness into a real test once behaviour is pinned down

Once a reproducible bad pattern is found (under e2e) and a fix is designed:

- Promote `run_resize_latency_test.py` from an ad hoc script to a proper test by:
  - Adding CLI options or hard-coded thresholds (e.g. fail if `max_rects > 100` **and** `num_large_rects > 5` in a short window after resize).
  - Optionally adding a simple input-latency proxy (e.g. scripted typing plus log correlation) if feasible.
- Integrate via CTest label (e.g. `e2e_resize_latency`) so regressions are caught in automated runs.

### 5. iPad-specific debugging plan

Given the latest observations (no repro with local C++ viewer, reproducible
only with the iPad app), the next phase of debugging should focus on the
server-side view of the **iPad connection** itself. The iPad client cannot be
fully automated from this repo, but we can instrument the server to learn
exactly what that client is doing.

Proposed steps:

1. **Add richer per-connection logging in VNCSConnectionST / ServerCore**

   - On client connect, log:
     - Client encodings list (including whether `encodingTight`, `encodingH264`,
       `pseudoEncodingLastRect`, `encodingCopyRect`, etc. are requested).
     - Negotiated pixel format, compressLevel, qualityLevel.
     - Whether the client enables continuous updates and/or fences.
   - On `setEncodings()` and `enableContinuousUpdates()`, log any changes in
     client behaviour for the iPad connection.
   - Optionally add a simple `TIGERVNC_RESIZE_TRACE=1` mode that:
     - Logs a short window (e.g. 5–10 seconds) of all `CC doUpdate` summaries,
       RTT pings, congestion state, and requested update rectangles before and
       after a detected framebuffer size change.

2. **Interactive debugging loop using a dedicated test server display**

   - Run an Xnjcvnc test server on a non-production display (e.g. `:998`) with
     XFCE via `~/.config/tigervnc/xstartup`.
   - Point the iPad viewer at this test server instead of `:2` and reproduce
     the resize + typing scenario there.
   - Capture:
     - The server log for the test display (with the enhanced per-connection
       logging enabled).
     - Optionally, a typing capture log from inside an xterm on `:998` via
       `tests/e2e/typing_capture.py` to correlate user input timing with server
       update behaviour.
   - Compare the test-server logs against the original `quartz:2.log` to
     confirm that the many-rects/full-FB pattern is truly tied to the iPad
     client, not some long-lived quirk of the `:2` desktop.

3. **Once iPad behaviour is characterised, attempt a synthetic reproduction**

   - Based on the observed iPad connection parameters (encodings, continuous
     updates, request frequency), construct a synthetic scenario for the C++
     viewer that mimics those characteristics as closely as possible.
   - Extend `tests/e2e/run_resize_latency_test.py` with options to:
     - Force the C++ viewer to use the same encodings and quality/compression
       settings as the iPad.
     - Enable continuous updates + fences in the same way.
   - If this synthetic run can now reproduce the bad rect pattern in the
     harness, we regain a **hands-off repro** that can be iterated on quickly.

4. **If still iPad-only, focus on robustness fixes guarded by capability checks**

   - Treat the iPad connection as a "stress test" of how the server handles a
     client that is:
     - Requesting updates at a particular cadence.
     - Potentially mixing continuous updates with explicit framebuffer update
       requests and resize operations.
   - Harden the server by:
     - Ensuring damage tracking (`UpdateTracker`) does not repeatedly
       over-report nearly-full-screen regions when resize + continuous updates
       interact badly.
     - Making congestion control more robust to bursty clients and mode
       switches (e.g. resetting state after large framebuffer changes, or
       bounding the effect of a single over-estimated RTT sample).
   - Any such fixes should be validated against both the iPad test server logs
     and the harness (once updated to mimic the iPad’s negotiation profile),
     to avoid regressions for desktop viewers.

## Log Locations and Artifacts from This Session

The following directories were created during the investigation:

- `tests/e2e/_artifacts/20251130_094129/`
  - Early 1920→1366 runs (before some harness enhancements).
- `tests/e2e/_artifacts/20251130_100837/`
  - 3840→1280 run with pre/post tiled logos.
- `tests/e2e/_artifacts/20251130_101709/`
  - Another 3840→1280 run (slightly different parameters).
- `tests/e2e/_artifacts/20251130_102156/`
  - **Key run**: 3840×2100 → **1366×1024**, pre/post tiled logos, showing good behaviour (low rect counts) under current harness.

For each, the main server log of interest is:

- `logs/resize_content_server_998.log`

The harness also logs the first several `CC doUpdate` lines and the computed metrics at the end of its own stdout.

---

This document is intended to give the next agent enough context to:

1. Understand the observed bug and why simple "more pixels" explanations don’t fit.
2. See what instrumentation and harnesses already exist.
3. Continue aligning the e2e environment with the real `:2` environment so the bug is reproducible in a disposable test display.
4. Then iterate on a fix, most likely in congestion control and/or damage tracking code, validated via the enhanced harness.
