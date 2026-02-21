# TigerVNC PersistentCache – Agent Playbook (Nick environment)

Last updated: 2026-02-01 17:37:43Z

This playbook is a **hands-on runbook** for future agents working with Nick’s TigerVNC viewer build and PersistentCache work. It focuses on **what actually worked**, what failed, and the constraints of this chat+artifact workflow.

---

## 0) Quick facts / constraints

### Repo + build

- Repo root: `/Users/nickc/code/tigervnc`

- Viewer target built via: `make viewer` (drives CMake + builds `njcvncviewer`).

### Canonical viewer launcher

- **Always start the viewer via the wrapper script** (to match Nick’s environment behavior and avoid divergence from direct binary invocation):

  - `~/scripts/njcvncviewer_start.sh` (primary)

  - There may be a similarly-named variant without underscore (check if needed).

### Staging / uploads / file constraints

- Staging area: `/Volumes/Nick/tmp` (preferred; flat, no subdirs).

- This environment often restricts uploads to **3 files per message**; logs count as one.

- Some file extensions can be rejected; common workaround:

  - Upload `.cxx/.cpp/.c` as `*.cxx.txt` etc.

  - `.h` typically uploads as-is.

- Avoid dotfiles in staging; create visible copies.

---

## 1) Parameter discovery (authoritative, code-based)

### PersistentCache parameters

From `vncviewer/parameters.cxx` (grep-driven discovery), these are the relevant CLI parameters:

- **Enable/disable PersistentCache protocol**: `-PersistentCache` (bool)

- **Memory cap (MB)**: `-PersistentCacheSize`

- **Disk cap (MB)**: `-PersistentCacheDiskSize`

- **Shard size (MB)**: `-PersistentCacheShardSize`

- **Override cache directory**: **`-PersistentCachePath`**

Default cache directory if not overridden:

- `~/.cache/tigervnc/persistentcache/`

### Recommended: fresh cache directory for tests

To isolate behavior and avoid existing cache state (size cap, corruption, old shards), run against a fresh empty directory:

```bash
$HOME/scripts/njcvncviewer_start.sh <server:port>   -PersistentCachePath "/Volumes/Nick/tmp/pcache_fresh_YYYYMMDD_HHMMSS"
```

Nick requested keeping the disk cap at default (4GB), so omit `-PersistentCacheDiskSize` unless testing cap behavior.

---

## 2) Patch/workflow strategy (safe and repeatable)

### Core principle

**Prefer patch artifacts over “edit scripts”.**

Rationale:

- Patches can be checked (`git apply --check`, custom structural checker, checksum) and refused safely.

- Chat-transmitted scripts can be subtly mangled and mutate the working tree in harmful ways.

### Patch artifact requirements

- Provide patch as **downloadable artifact** (not inline in chat).

- Also provide checksum file:

  - Use `.sha256.txt` (NOT `.sha256`).

- Before applying:

  - Verify checksum from staging directory.

  - Run Nick’s patch structural sanity checker.

  - `git apply --check` then `git apply`.

### Recommended apply block template

```bash
(
  ts="$(date +%Y%m%d_%H%M%S)"
  log="/Volumes/Nick/tmp/stepXX_apply_${ts}.log"
  (
    set -uo pipefail
    exec > >(tee "$log") 2>&1
    repo="/Users/nickc/code/tigervnc"
    cd "$repo"

    f="<patchfile>.patch.txt"
    sha="${f}.sha256.txt"

    ( cd /Volumes/Nick/tmp && shasum -a 256 -c "$sha" )
    python3 "$HOME/scripts/check_patch.py" --no-git --no-patch --repo "$repo" "/Volumes/Nick/tmp/$f"

    git apply --check "/Volumes/Nick/tmp/$f"
    git apply "/Volumes/Nick/tmp/$f"

    make viewer
  )
)
```

### When a patch “does not apply”

Common causes:

- Target file has drifted vs the context used to create the patch.

Preferred recovery:

- Upload the **exact current file(s)** from the working tree to the agent.

- Agent generates a new patch against those bytes.

---

## 3) Key failures & blind alleys encountered

### A) Patch drift: “patch does not apply”

Repeatedly occurred when patches were anchored at line numbers or context that no longer matched. Fix: regenerate patch against _current_ file content.

### B) Attribute portability: `[[maybe_unused]]` rejected

On this macOS clang toolchain, attempts to silence `-Werror -Wunused-function` using `[[maybe_unused]]` failed.

Workaround that compiled:

- Use `__attribute__((unused))` on the function.

### C) Linker failure: missing `writeEntryToShard` symbol

After partial edits, linker error occurred: undefined symbol `GlobalClientPersistentCache::writeEntryToShard(...)`.

Root cause:

- Function declared and called (from `flushDirtyEntries()` and `onWriteRequest()`), but no definition in the translation unit.

Resolution path:

- Implement `writeEntryToShard(...)` in `GlobalClientPersistentCache.cxx`.

### D) Dangerous script mangling: literal `\t` in source

A local-edit script inserted a function body containing literal `\t` sequences, producing compile errors.

Fix:

- Replace the broken function with a correct version using real tabs/spaces.

- Prefer patch-based remediation.

### E) Test harness gotcha: missing shard selection/restore bug

A test attempted to remove a shard that didn’t exist and then restored with the wrong name.

Lesson:

- Select shards from `ls "$cache"/shard_*.dat` and restore to the exact original basename.

---

## 4) Reliable test approach for missing-shard self-heal

### Why use a fresh cache directory

Existing cache state can trigger cap enforcement spam (e.g., cache usage > configured cap) and obscure the missing-shard signal.

### Suggested two-phase test (fresh cache)

1) **Run once** with fresh cache dir to create `index.dat` and `shard_*.dat`.

2) Stop viewer.

3) Move one shard aside.

4) **Run again** with same `-PersistentCachePath` and observe:

   - open/seek/read hydration failure logging

   - one-time/per-shard logging behavior

   - self-heal behavior (dropping index entries referencing missing shard)

5) Restore shard to original name.

### Log locations

- Viewer wrapper produces `/tmp/njcvncviewer_*.log`

- PersistentCache debug logger produces `/tmp/persistentcache_debug_<ts>.log`

---

## 5) Handy staging commands (Nick preferences)

### Stage specific source files for upload (no dotfiles, zsh-safe)

```bash
(
  ts="$(date +%Y%m%d_%H%M%S)"
  log="/Volumes/Nick/tmp/stage_${ts}.log"
  (
    set -uo pipefail
    setopt null_glob
    exec > >(tee "$log") 2>&1

    repo="/Users/nickc/code/tigervnc"
    cd "$repo"

    src_cxx="common/rfb/GlobalClientPersistentCache.cxx"
    src_h="common/rfb/GlobalClientPersistentCache.h"

    dst_cxx="/Volumes/Nick/tmp/stepXX_${ts}_GlobalClientPersistentCache.cxx.txt"
    dst_h="/Volumes/Nick/tmp/stepXX_${ts}_GlobalClientPersistentCache.h"

    cp -p -- "$src_cxx" "$dst_cxx"
    cp -p -- "$src_h" "$dst_h"

    # Touch so they sort to top
    touch "$dst_cxx" "$dst_h"
    ls -lt /Volumes/Nick/tmp | head -n 20
  )
)
```

---

## 6) Glossary of important paths

- Repo: `/Users/nickc/code/tigervnc`

- Default cache dir: `~/.cache/tigervnc/persistentcache/`

- Fresh cache dir recommended: `/Volumes/Nick/tmp/pcache_fresh_<ts>`

- Viewer logs: `/tmp/njcvncviewer_*.log`

- PersistentCache logs: `/tmp/persistentcache_debug_*.log`

---

## 7) “Do not repeat” checklist

- Don’t guess parameter names; **grep `vncviewer/parameters.cxx`**.

- Don’t rely on brittle patch line numbers; regenerate against current file bytes.

- Don’t use `[[maybe_unused]]` on this toolchain; use `__attribute__((unused))`.

- Don’t ship source-rewriting scripts unless explicitly requested; prefer patch artifacts.

- When simulating missing shards: always select an existing shard and restore exact basename.
