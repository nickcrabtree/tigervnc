# Conversation Summary & Way Forward (TigerVNC PersistentCache)

Last updated: 2026-02-01 17:37:43Z

This document is a handover summary for a future agent. It captures what was done, what broke, what was fixed, and exactly what remains.

---

## 1) Goals

1. Build TigerVNC viewer and iterate on PersistentCache behavior.

2. Fix build breaks caused by missing symbols / Werror.

3. Validate PersistentCache robustness, especially missing-shard self-heal and avoiding log storms.

4. Continue on to the original operational issue (resize/disconnect) once PersistentCache is stable.

---

## 2) What we fixed

### Build unblocked

- Viewer (`njcvncviewer`) builds successfully after patching `GlobalClientPersistentCache.cxx`.

### Fixed the dangerous `\t` insertion

- A malformed local edit inserted literal `\t` sequences in C++ source, causing compile errors.

- A patch was generated from the uploaded current file that replaced the broken `writeEntryToShard()` body with a correct version and removed trailing whitespace.

### Identified correct runtime parameters

From grepping `vncviewer/parameters.cxx`:

- `-PersistentCachePath` overrides the cache directory.

- `-PersistentCacheDiskSize` exists, but Nick wants to keep the default cap (~4GB).

---

## 3) Major pitfalls discovered (and how to avoid)

1. **Patch drift**: patches can fail with “patch does not apply” if file context differs.

   - Fix: upload exact current file, regenerate patch.

2. **Script mangling risk**: chat-transmitted scripts can be subtly altered (e.g., `\t` literals), causing destructive edits.

   - Fix: prefer patch artifacts; only use scripts for staging/inspection.

3. **Toolchain attribute quirks**:

   - `[[maybe_unused]]` rejected by clang when placed after `static`.

   - `__attribute__((unused))` works.

4. **Test harness gotcha**:

   - A missing-shard test selected a shard that didn’t exist and restored with wrong name.

   - Fix: select real shard from cache directory; restore exact basename.

---

## 4) Current status

- Build: green.

- Parameters discovered: `-PersistentCachePath` confirmed.

- Next step: run **fresh-cache missing-shard self-heal test** using `-PersistentCachePath` and Nick’s wrapper script.

---

## 5) Way forward (recommended plan)

### A) Fresh-cache missing-shard self-heal test

1) Create fresh empty directory under `/Volumes/Nick/tmp/pcache_fresh_<ts>`.

2) Start viewer via wrapper:

   - `$HOME/scripts/njcvncviewer_start.sh <server:port> -PersistentCachePath "$cache"`

3) Run long enough to create index+shard.

4) Stop viewer.

5) Move one existing shard aside.

6) Start viewer again with same `-PersistentCachePath`.

7) Inspect logs for:

   - `failed during hydration` (open/seek/read)

   - `dropped N index entries referencing shard X`

   - rate-limited behavior (no storms)

8) Restore shard to original basename.

### B) Only after A passes: return to resize/disconnect issue

Reproduce the original issue and collect logs.

---

## 6) Artifacts to collect

- Viewer log: `/tmp/njcvncviewer_*.log`

- PersistentCache debug log: `/tmp/persistentcache_debug_*.log`

- Any applied patches in `/Volumes/Nick/tmp/`

---

## 7) Collaboration preferences

- Patch artifacts (downloadable) over inline diffs

- Checksum files as `.sha256.txt`

- Logged command blocks

- Stage directly into `/Volumes/Nick/tmp` root

- Keep within 3-file upload cap
