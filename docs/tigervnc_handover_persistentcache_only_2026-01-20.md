# Handover: PersistentCache-only refactor (hard-breaking ContentCache removal)

## Current goal

- Complete a **hard-breaking removal** of *ContentCache* (no shims/aliases) and
  converge on **PersistentCache-only** to reduce SLOC/complexity.

## Repo + environment

- Repo: `/Users/nickc/code/tigervnc`
- Build dir: `/Users/nickc/code/tigervnc/build`
- Staging/upload dir: `/Volumes/Nick/tmp/`
- macOS toolchain quirks:

  - `date -Is` is GNU-only (use macOS-compatible formatting).

## What has been completed

### 1) ContentCache protocol removal (earlier work)

- Legacy ContentCache protocol message types and APIs were removed.
- Unit tests previously returned to green (364/364) after fixing multiple
  refactor regressions.

### 2) GoogleTest discovery timeout fix (resolved)

- Build-time GTest discovery (CMake’s `GoogleTestAddTests.cmake`) was timing out
  and deleting test executables.
- Fix was implemented by increasing `DISCOVERY_TIMEOUT` for slow unit-test
  targets.
- Because patch transport was unreliable, the successful approach was:

  - edit locally
  - generate patch via `git diff`
  - checksum with `shasum -a 256`.

### 3) Phase 1: remove ContentCache knobs from C++ viewer (applied)

A patch (`remove_contentcache_phase1_v2.patch`) was applied cleanly to:

- `vncviewer/parameters.h`
- `vncviewer/parameters.cxx`
- `vncviewer/CConn.cxx`

Intended Phase 1 behavior:

- Remove `ContentCache` / `ContentCacheSize` viewer parameters.
- Remove “ContentCache is an alias” wording and any references to
  `::contentCache`.

## Current blocker (needs immediate attention)

### Viewer build fails to link on macOS

`njcvncviewer` fails at link-time with undefined symbols:

- `loadViewerParameters(const char*)`
- `saveViewerParameters(const char*, const char*)`
- `_persistentCache`

### Evidence collected (do not lose)

1) `vncviewer/parameters.cxx` is only **246 lines** and has SHA-256:

   `3228d351cc113cced7000562f7a8606dcf08aec51eaa6c5a83dfc1b1b2002470`.

1) A byte/line capture shows the file ends after the `via` parameter and
   **does not include**:

   - `persistentCache` parameter definition
   - `saveViewerParameters()` implementation
   - `loadViewerParameters()` implementation

1) `rg` output file for the key symbols contains only the banner header and no
   matches, confirming they’re absent from the file content.

These captures were uploaded as:

- `parameters_cxx_stats.txt`
- `parameters_cxx_headtail.txt`
- `parameters_cxx_rg.txt`

## Why this is happening (likely cause)

Phase 1 patching accidentally removed/truncated too much from
`vncviewer/parameters.cxx`.

The header still declares `persistentCache` and the save/load functions, but the
`.cxx` file no longer defines them, producing undefined symbol errors even though
`parameters.cxx.o` is compiled and linked.

## Next steps for the next agent

### Step A — confirm the current broken state

Run:

```sh
cd /Users/nickc/code/tigervnc
wc -l vncviewer/parameters.cxx
rg -n "saveViewerParameters\(loadViewerParameters\(\bpersistentCache\b" -S vncviewer/parameters.cxx || true
```

### Step B — restore missing definitions

Goal: `vncviewer/parameters.cxx` must define:

- `core::BoolParameter persistentCache(...)`
- `void saveViewerParameters(...)`
- `char* loadViewerParameters(...)`

**Preferred repair approach (min risk):**

1) Use git history to recover the pre-Phase-1 version of
   `vncviewer/parameters.cxx`.
1) Re-apply *only* the intended deletions (remove ContentCache param/size, keep
   PersistentCache + save/load).

Suggested workflow:

```sh
cd /Users/nickc/code/tigervnc

# Identify last known-good version before Phase 1
# (If Phase 1 was the most recent commit, try HEAD^)
git log --oneline -n 20 -- vncviewer/parameters.cxx

# Inspect prior version
# git show <GOOD_COMMIT>:vncviewer/parameters.cxx | less

# Restore the file from a known-good commit (example)
# git checkout <GOOD_COMMIT> -- vncviewer/parameters.cxx
```

Then re-edit locally to remove ContentCache blocks and regenerate patch via
`git diff`.

### Step C — validate fix

```sh
cmake --build build --target njcvncviewer --verbose
cmake --build build -j
ctest --test-dir build --output-on-failure
```

### Step D — continue the broader refactor

After viewer builds again:

- Proceed to Phase 2 (tests and utilities that still reference ContentCache):

  - `tests/unit/decode_manager.cxx` ContentCache gating tests
  - `tests/unit/bandwidthstats.cxx` ContentCache tracking tests

- Then Phase 3 (docs/scripts/e2e cleanup) and Phase 4 (Rust subtree) as desired.

## Collaboration notes (important)

- Chat copy/paste is not byte-safe; use file uploads.
- Patch artifacts should be provided as `*.patch` + `*.sha256`.
- Prefer local-edit → `git diff` patch generation for reliability.
- Use subshell blocks `( ... )` when a block might invoke `exit`.

---

End of handover.
