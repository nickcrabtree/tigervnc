# E2E Black‑Box Screenshot Test for Display Corruption

## Purpose

This document describes a **black‑box, screenshot‑based** end‑to‑end test that detects display corruption in the TigerVNC viewers without relying on any additional instrumentation in the viewer or server.

The core idea:

> Run two independent viewers against the same deterministic test server scenario and periodically capture screenshots of their windows. One viewer runs with cache protocols disabled (ground truth), the other with them enabled (under test). Any pixel‑level mismatch between the screenshots is treated as display corruption.

This test is designed to run on **Linux** (e.g. `quartz.local`) using the existing e2e infrastructure.

- No changes are required to the server or viewer binaries.
- The test interacts only via CLI and X11 (windows, screenshots).

---

## High‑Level Design

### Components

1. **Content server** (`VNCServer`)
   - Runs on a high‑numbered X display (e.g. `:998`) using `Xtigervnc` or local `Xnjcvnc`.
   - Hosts scripted desktop content via existing `tests/e2e/scenarios.py` helpers (xterm windows, repeated content, etc.).

2. **Viewer X server**
   - Runs on another X display (e.g. `:999`) using `Xtigervnc` (nested) or `Xvfb`.
   - Hosts two viewer windows side by side:
     - Viewer A: **ground truth** (caches disabled).
     - Viewer B: **under test** (caches enabled).

3. **Two viewers** (C++ viewer, potentially Rust later)
   - Both connect over TCP to the same VNC server port (e.g. `127.0.0.1:6898`).
   - Launched in the viewer X display (`DISPLAY=:999`).
   - Differ only in **run‑time parameters** (cache on/off).

4. **Screenshot tooling**
   - Uses existing X tooling on the host (prefer `xwd` + `convert`, or `import`, or `vncsnapshot`) to capture window images.
   - Screenshots saved to `tests/e2e/_artifacts/...` via `ArtifactManager`.

5. **Comparator script**
   - Loads screenshot pairs (`ground_truth_N.png`, `cache_on_N.png`).
   - Validates identical dimensions and pixel formats.
   - Computes pixel‑wise differences.
   - If any non‑zero difference is found, emits a visual diff (`diff_N.png`) and marks the test as **failed**.

### Test Flow (Single Run)

1. Preflight checks and artifact setup.
2. Start content server on `:998` and its window manager.
3. Start viewer X server on `:999` and its window manager.
4. Launch both viewers on `:999` connecting to `:998`:
   - Viewer A: `ContentCache=0`, `PersistentCache=0`.
   - Viewer B: `ContentCache=1` and/or `PersistentCache=1` as needed.
5. Run a deterministic scenario on the content server (e.g. `cache_hits_minimal` for a fixed duration or cycle count).
6. At several **checkpoints** during the scenario, capture synchronized screenshots of both viewer windows.
7. After the scenario ends, compare all screenshot pairs.
8. If any pair differs at the pixel level, the test fails and the artifacts are preserved for debugging.
9. Clean up all processes (server, viewers, X servers) using tracked PIDs / process groups.

---

## Synchronization Strategy

We want both viewers’ screenshots taken at a logically comparable moment.

Because we won’t modify the RFB protocol or viewer code, synchronization is achieved purely via **timing and scenario design**:

- The scenario on `:998` is **deterministic** and consists of repeated phases with static content.
- After a transition, the scenario deliberately waits long enough for both viewers to complete updates.
- The test harness sleeps slightly longer than the scenario’s guaranteed settle time before capturing screenshots.

### Example Checkpoint Pattern

For `cache_hits_minimal` (already deterministic):

1. Open xterm with fixed geometry and content; sleep ~2 seconds.
2. Reopen xterm with same geometry and identical content; sleep ~2 seconds.
3. Optionally move/resize windows; sleep ~1 second.

We define checkpoints at the end of each phase, e.g.:

- `checkpoint 1`: after first xterm content is fully drawn.
- `checkpoint 2`: after repeated xterm content (expected cache hits).
- `checkpoint 3`: after a move/resize.

The test harness only needs to know **how long** each phase takes. It sleeps a conservative superset (e.g. 0.5–1.0 seconds more) to ensure both viewers have rendered.

Number of checkpoints is configurable (e.g. 5–10 per run).

---

## New Script: `tests/e2e/run_black_box_screenshot_test.py`

This script orchestrates the entire test. It mirrors `run_contentcache_test.py` but focuses on screenshot comparison instead of cache metrics.

### CLI Interface

Example CLI:

```bash
python3 tests/e2e/run_black_box_screenshot_test.py \
  --display-content 998 --port-content 6898 \
  --display-viewer 999 --port-viewer 6899 \
  --duration 90 \
  --wm openbox \
  --checkpoints 6 \
  --mode persistent  # or contentcache, both
```

Suggested arguments:

- `--display-content` (int, default: 998) – content server display.
- `--port-content` (int, default: 6898) – content server port.
- `--display-viewer` (int, default: 999) – nested display for viewers.
- `--port-viewer` (int, default: 6899) – port if viewer display is also a VNC server (optional; may be unused if we use Xvfb).
- `--duration` (int, default: 90) – total scenario duration in seconds.
- `--wm` (str, default: `openbox`) – window manager.
- `--checkpoints` (int, default: 6) – number of screenshot checkpoints.
- `--mode` (str, default: `persistent`) – cache mode under test:
  - `none` – both viewers caches disabled (sanity check; screenshots must match).
  - `content` – ContentCache only.
  - `persistent` – PersistentCache only.
  - `both` – both caches enabled.
- `--verbose` – more logging.

### Steps in Detail

#### 1. Preflight

- Use a **variant** of `preflight_check_cpp_only` tailored to this test:
  - Required binaries:
    - `Xtigervnc` (or ability to use local `Xnjcvnc`).
    - `xterm`, `openbox`, `xsetroot`, `wmctrl`, `xdotool`.
    - Screenshot tools: prefer **any** of:
      - `xwd` + `convert` (ImageMagick), or
      - `import` (ImageMagick), or
      - `vncsnapshot`.
  - Required viewer binary:
    - `build/vncviewer/njcvncviewer` (C++ viewer).
  - Optional: Rust viewer binary for future extension.

If any critical binary is missing, raise `PreflightError` and exit.

#### 2. Artifact Manager

- Use existing `ArtifactManager` (`framework.ArtifactManager`) to create directories:
  - Logs: `tests/e2e/_artifacts/YYYYmmdd_HHMMSS/logs/`.
  - Screenshots: `tests/e2e/_artifacts/YYYYmmdd_HHMMSS/screenshots/`.
  - Reports: `tests/e2e/_artifacts/YYYYmmdd_HHMMSS/reports/`.

We’ll place screenshots in e.g.:

- `screenshots/checkpoint_N_ground_truth.png`.
- `screenshots/checkpoint_N_cache_on.png`.
- `screenshots/checkpoint_N_diff.png` (if mismatch).

#### 3. Start Content Server (`:998`)

- Use `framework.VNCServer` with `server_choice` `system` or `local` as appropriate.
- Geometry: e.g. `1600x1000`.
- Depth: `24`.
- `SecurityTypes None` and logging to `logs/content_server_998.log`.
- Call `start()` and then `start_session(wm=...)`.

Use existing `ScenarioRunner` later against this display.

#### 4. Start Viewer Display (`:999`)

Two options:

1. **Nested VNC server approach** (consistent with existing e2e tests):
   - Use another `VNCServer` instance on `:999`.
   - Start a WM on `:999`.
   - Viewer processes run with `DISPLAY=:999`.
   - This is already supported by `framework.VNCServer`.

2. **Xvfb approach** (if installed):
   - If `Xvfb` is present, start it manually for `:999` with fixed geometry, e.g.:

     ```bash
     Xvfb :999 -screen 0 1600x1000x24 -nolisten tcp -noreset
     ```

   - However, a second `VNCServer` is not strictly required in this case; the viewers just use `DISPLAY=:999`.

**Plan:** reuse the existing nested VNC approach (option 1), since the infrastructure (`VNCServer.start_session`, etc.) is already in place.

#### 5. Launch Two Viewers

Use helper functions similar to `run_viewer()` in `test_cpp_contentcache.py` / `test_cache_parity.py`, but with explicit cache parameters.

- Both viewers connect to the same content server port (`--display-content` / `--port-content`).
- Both viewers live in display `:999` (the viewer display created above).

Example commands (C++ viewer):

- Ground truth (caches off):

  ```bash
  njcvncviewer 127.0.0.1::6898 \
    Shared=1 \
    Log=*:stderr:100 \
    ContentCache=0 PersistentCache=0
  ```

- Cache under test:

  ```bash
  njcvncviewer 127.0.0.1::6898 \
    Shared=1 \
    Log=*:stderr:100 \
    ContentCache=1 PersistentCache=1
  ```

Mode mapping:

- `mode=none`:
  - Viewer A: `ContentCache=0 PersistentCache=0`.
  - Viewer B: `ContentCache=0 PersistentCache=0` (sanity; screenshots must match).
- `mode=content`:
  - Viewer A: `ContentCache=0 PersistentCache=0`.
  - Viewer B: `ContentCache=1 PersistentCache=0`.
- `mode=persistent`:
  - Viewer A: `ContentCache=0 PersistentCache=0`.
  - Viewer B: `ContentCache=0 PersistentCache=1`.
- `mode=both`:
  - Viewer A: `ContentCache=0 PersistentCache=0`.
  - Viewer B: `ContentCache=1 PersistentCache=1`.

**Window placement:**

- After both viewers connect, use `wmctrl` / `xdotool` on `:999` to place their windows side by side with fixed geometries, e.g.:
  - Viewer A: `800x1000+0+0`.
  - Viewer B: `800x1000+800+0`.

The screenshot helper must know which window belongs to which role (ground truth vs cache). This can be done via window titles or `wmctrl -l` / `xdotool search` filters.

We can set window titles explicitly using viewer parameters if available (e.g. via a config file), or by relying on the server name portion of the title and launching one viewer with `-geometry` or different `display` so that `wmctrl` can distinguish them by creation order.

**Simpler approach:** launch one viewer, wait for its window, rename it (with `wmctrl -r`), then launch the second one and rename that. Example:

1. Start Viewer A; use `wmctrl`/`xdotool` to find the most recently created window matching `TigerVNC` and rename it to `VNC Ground Truth`.
2. Start Viewer B; repeat, rename to `VNC Cache`. Move/resize both windows to fixed positions.

#### 6. Run Scenario on Content Server

Use the existing `ScenarioRunner`:

- Create `ScenarioRunner(display_content)`.
- Run e.g. `cache_hits_minimal(duration_sec=duration)` or a fixed number of cycles.

The scenario will:

- Repeatedly open/close xterms with identical content.
- Optionally move windows around.
- Sleep between operations to allow encoder/decoder pipelines to settle.

We’ll coordinate screenshot timing using the scenario’s known pauses.

#### 7. Screenshot Capture Helper

Implement a new helper in `framework.py` or a small local module in `tests/e2e` for capturing screenshots.

Requirements:

- Runs commands in display `:999`.
- Accepts a **window identifier** (title or ID) and output path.
- Uses available screenshot tooling in priority order, e.g.:

1. `xwd` + `convert`:

   ```bash
   xwd -display :999 -silent -id <WIN_ID> | convert xwd:- png:<outfile>
   ```

2. `import` (ImageMagick):

   ```bash
   import -display :999 -window <WIN_ID> <outfile>
   ```

3. `vncsnapshot` (if a separate VNC server is used for viewers, not strictly necessary here).

Window lookup with `wmctrl` / `xdotool`:

- Use `xdotool search --name "VNC Ground Truth"` and `"VNC Cache"` to get window IDs.
- Alternatively use WM_CLASS or partial names.

To reduce dependence on window titles and avoid racing with WM, we can:

- Identify windows by PID using `xdotool search --pid <pid>`.
- We already obtain PIDs when launching viewers via `subprocess.Popen`.

**Plan:** prefer PID‑based lookup for robustness.

Example:

```bash
xdotool search --pid <VIEWER_A_PID> | head -n1
# yields WIN_ID_A
xdotool search --pid <VIEWER_B_PID> | head -n1
# yields WIN_ID_B
```

**Screenshot API (Python pseudo‑signature):**

```python
capture_window_screenshot(display: int, win_id: str, outfile: Path) -> None
```

This helper should:

- Set `DISPLAY=":{display}"`.
- Run the best available screenshot command and check its exit status.

#### 8. Checkpoint Logic

We want N checkpoints over the run. Simple implementation:

- Compute checkpoint times evenly over the scenario’s runtime (e.g. every `duration / (checkpoints+1)` seconds starting after an initial warm‑up).
- Or tie checkpoints to scenario cycles (e.g. after each pair of xterm open/close operations).

Minimal implementation:

1. Start both viewers and scenario.
2. For `i` in `1..checkpoints`:
   - Sleep `interval` seconds.
   - Capture screenshots from both windows, e.g. to:
     - `screenshots/checkpoint_i_ground_truth.png`.
     - `screenshots/checkpoint_i_cache.png`.

The `interval` should be larger than the longest phase in the scenario to ensure both viewers have settled. For example, if scenario sleeps ~2 seconds between content changes, use `interval >= 3` seconds.

Later we can refine this by adding explicit waits for stabilized WM state (e.g. via `wmctrl -l` ordering) or longer sleeps.

#### 9. Comparator (Screenshot Diff)

Implement a small Python module, e.g. `tests/e2e/screenshot_compare.py`, that:

Inputs:

- `ground_truth: Path` – PNG screenshot.
- `cache_on: Path` – PNG screenshot.
- `diff_out: Path` – PNG diff image.

Steps:

1. Load both images (use Pillow / `PIL.Image`).
2. Assert dimensions match; if not, emit a descriptive error and fail.
3. Convert both to the same mode (e.g. `RGBA`) if needed.
4. Compute a per‑pixel diff:
   - Fast way: subtract arrays using NumPy, or compare bytes in Python.
   - Count `num_diff_pixels` where any channel differs.
5. If `num_diff_pixels == 0`:
   - Return success for this checkpoint.
6. If `num_diff_pixels > 0`:
   - Compute a diff image (`diff_out`), e.g.:
     - For differing pixels, set a bright color (red) or encode the magnitude of difference.
     - For matching pixels, set black or transparent.
   - Compute summary metrics:
     - Percentage of differing pixels.
     - Bounding boxes of changed regions.
   - Write a small JSON summary to `reports/checkpoint_i_diff.json`.
   - Mark the entire test as **failed**.

For CI, it’s enough to:

- Print a one‑line summary (`Checkpoint i: 12345 / 1.23% pixels differ; see artifacts`).
- Return non‑zero exit status from the main script.

#### 10. Cleanup

Use `ProcessTracker` to clean up:

- The content server (`vnc_content`);
- The viewer display server (`vnc_viewerwin`);
- All viewer processes (`viewer_ground_truth`, `viewer_cache`);
- Any other child processes (WMs) associated with those.

Call `tracker.cleanup_all()` in a `finally:` block.

---

## Test Matrix

We can reuse this runner for multiple modes.

### Quick CI Profile

- Resolution: `1600x1000`.
- Duration: 60–90 seconds.
- Checkpoints: 6–8.
- Modes:
  1. `none` – both viewers cache‑off (sanity; expects all checkpoints identical).
  2. `persistent` – ground truth vs PersistentCache.

Quick job runs each mode once.

### Extended / Nightly Profile

- Resolution: `1920x1080`.
- Duration: 120–180 seconds.
- Checkpoints: 10–12.
- Modes:
  - `content`.
  - `persistent`.
  - `both`.
- Potentially also run with a Rust viewer as the cache‑ON or ground‑truth client in the future.

---

## Integration on `quartz.local`

Implementation and execution will be done on the Linux host (e.g. `quartz.local`), not macOS.

Example workflow (on macOS client, *not* committed to this repo, just illustrative):

1. Update code on `quartz.local`:

   ```bash
   ssh quartz.local "cd /home/nickc/code/tigervnc && git pull"
   ```

2. Build viewer (and local server if desired):

   ```bash
   ssh quartz.local "cd /home/nickc/code/tigervnc && \
     cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo && \
     make -C build viewer"
   ```

3. Install e2e dependencies if not already present (`Xtigervnc`, `xterm`, `openbox`, `wmctrl`, `xdotool`, `xwd`, `convert`), per `tests/e2e/README.md`.

4. Run the new test:

   ```bash
   ssh quartz.local "cd /home/nickc/code/tigervnc && \
     python3 tests/e2e/run_black_box_screenshot_test.py --mode persistent --verbose"
   ```

This script will produce artifacts under `/home/nickc/code/tigervnc/tests/e2e/_artifacts/` on `quartz.local` for later inspection.

---

## Failure Modes and Diagnostics

When the test fails (screenshots differ), the artifacts should include:

- For the **first** failing checkpoint `i`:
  - `screenshots/checkpoint_i_ground_truth.png`.
  - `screenshots/checkpoint_i_cache.png`.
  - `screenshots/checkpoint_i_diff.png`.
  - `reports/checkpoint_i_diff.json` with summary stats.
- Viewer logs:
  - `logs/ground_truth_viewer.log`.
  - `logs/cache_viewer.log`.
- Server logs:
  - `logs/content_server_998.log`.
  - `logs/viewerwin_server_999.log`.
- Scenario log / stats (if any).

These artifacts should be sufficient to:

- Confirm the corruption visually.
- Correlate the corrupt region with ContentCache/PersistentCache events or other protocol logs.

---

## Possible Extensions (Future)

- Add Rust viewer to the matrix (C++ vs Rust, both black‑box).
- Use a **window‑specific screenshot** with stable naming rather than relying solely on PIDs.
- Add an option to capture **full viewer display** vs **cropped region** (e.g. crop to content area, excluding title bars, to avoid spurious differences from WM decorations).
- Support tolerance thresholds (e.g. allow for minor anti‑aliasing differences) although for VNC framebuffer corruption we generally want strict bitwise equality.

---

## Summary

This plan defines a self‑contained, black‑box e2e test that:

- Requires **no changes** to viewer or server code.
- Uses two viewers (cache‑off vs cache‑on) driven by existing e2e infrastructure.
- Detects display‑level corruption via pixel‑wise screenshot comparison.
- Produces rich artifacts to debug any detected mismatches.

All implementation work should be done and exercised on the Linux host (e.g. `quartz.local`), with this document serving as the blueprint for the new `run_black_box_screenshot_test.py` script and helpers.