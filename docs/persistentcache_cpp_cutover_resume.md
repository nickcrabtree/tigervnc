# PersistentCache C++ Viewer Cutover Resume

Last updated: 2026-06-07 10:32:56Z

## Purpose

This resume document captures the current state of the C++ PersistentCache viewer cutover so a different agent can continue once the ARO workflow supports the required sequence:

1. Commit locally on the Mac working tree.
2. Push the branch.
3. Pull the branch on quartz.
4. Build and test on quartz.
5. Only then decide whether to keep, revise, or revert the cutover.

The current ARO version could not complete that workflow because local `cmake` was unavailable and SSH to quartz failed. Do **not** treat this change as build-validated yet.

## User intent

Nick requested a cleanup of the C++ viewer to remove legacy compatibility cruft. The intended direction is a hard cutover for the C++ PersistentCache protocol path, making the cleaned-up C++ contract the target for future Rust parity work.

## Current working-tree state

The local Mac working tree contains uncommitted changes in these files:

- `common/rfb/CMsgReader.cxx`
- `common/rfb/EncodeManager.cxx`
- `common/rfb/SMsgWriter.cxx`
- `common/rfb/SMsgWriter.h`
- `common/rfb/cache/BandwidthStats.cxx`
- `common/rfb/cache/BandwidthStats.h`
- `common/rfb/encodings.h`
- `docs/PERSISTENTCACHE_DESIGN.md`
- `docs/remove_contentcache_implementation.md`
- `tests/e2e/log_parser.py`

A patch snapshot was generated during the previous ARO session at:

- `/Users/nickc/tmp/tigervnc_cpp_pcache_cutover.patch.txt`

Because M365 renamed the original `.patch` attachment to `.patch.txt`, preserve the content rather than relying on the extension.

## Intended protocol cutover

The intended C++ wire-contract cleanup is:

- `PersistentCachedRect` references use a 16-byte `CacheKey` plus offset fields.
- `PersistentCachedRectInit` is v2-only:
  - 16-byte `CacheKey`
  - mandatory 1-byte flags field
  - optional canonical `PixelFormat` when `native_format` is set
  - inner encoding
  - encoded payload
- The C++ server writer always emits the v2 flags byte.
- The C++ viewer reader no longer accepts or heuristically falls back to legacy flagless INIT.
- The `includeFlags` compatibility parameter and related call-site logic are removed.
- Comments, docs, and e2e log accounting are updated from old 8-byte/20-byte/24-byte wording to 16-byte `CacheKey`, 36-byte reference overhead, and 33-byte v2 INIT overhead.

## Checks already run

The previous ARO run reported:

- `git diff --check` passed.
- Greps for removed compatibility cruft returned no matches for:
  - `includeFlags`
  - `isSaneInnerEncoding`
  - `64-bit ID on the wire`
  - `20 bytes per PersistentCachedRect`
  - `24 bytes (12 header + 8 ID`
  - `Backwards-compatible entry point`
- `python3 -m py_compile tests/e2e/log_parser.py` passed.
- Local `cmake` was not available.
- `xcrun --find clang++` found Xcode clang.
- SSH to `quartz` timed out.
- `quartz.local` did not resolve.

## Important caveat

The patch has not been C++ build-tested. Do not commit to a shared branch or merge target until quartz build/test has run successfully. If you need to commit locally only to enable the push/pull workflow, use a clearly marked WIP commit and be
prepared to amend or revert.

## Recommended resume workflow for the next agent

### 1. Inspect the local working tree

```bash

cd "$HOME/code/tigervnc"
git status --short --branch
git diff --check
git diff --stat
python3 -m py_compile tests/e2e/log_parser.py

```text

### 2. Review the actual source, not M365-rendered diffs

M365/ARO rendered diffs were lossy in the previous session. Inspect source directly with `sed`, editor, or `git diff --word-diff=plain` before committing. Pay particular attention to:

- `common/rfb/CMsgReader.cxx`
  - `readPersistentCachedRectInit()` should read `16 + 1 + 4` minimum bytes.
  - It should read the 16-byte `CacheKey`, then flags, reject reserved flags, optionally read canonical PF, then read inner encoding.
  - It should reset `pendingPersistentCacheInitActive`, key, encoding, flags, and PF state after a completed rectangle.
- `common/rfb/SMsgWriter.cxx`
  - `writePersistentCachedRectInit()` should always write the flags byte.
  - It should reject reserved flags.
  - It should write PF only when `flags & 0x01`.
- `common/rfb/SMsgWriter.h`
  - Signature should not include `includeFlags`.
- `common/rfb/EncodeManager.cxx`
  - Call sites should pass `(rect, cacheKey, payloadEnc->encoding, flags/initFlags, pf-or-nullptr)` only.
- `tests/e2e/log_parser.py`
  - Python syntax should compile.
  - PersistentCache reference accounting should use 36 bytes.
  - PersistentCache INIT v2 accounting should use 33 bytes excluding optional PF.

### 3. Commit locally for the required push/pull workflow

Use a local WIP commit only after source inspection passes:

```bash

Use a local WIP commit only after source inspection passes:

```bash
git add common/rfb/CMsgReader.cxx \
  common/rfb/EncodeManager.cxx \
  common/rfb/SMsgWriter.cxx \
  common/rfb/SMsgWriter.h \
  common/rfb/cache/BandwidthStats.cxx \
  common/rfb/cache/BandwidthStats.h \
  common/rfb/encodings.h \
  docs/PERSISTENTCACHE_DESIGN.md \
  docs/remove_contentcache_implementation.md \
  tests/e2e/log_parser.py \
  docs/persistentcache_cpp_cutover_resume.md
git commit -m "Hard cut over C++ PersistentCache v2 wire format"
```bash

```

If local policy prefers not to commit docs resume into the product branch, keep this resume as an untracked handoff note or move it to the agent handoff branch first.

### 4. Push the branch

```bash

git push origin HEAD

```text

Record the branch name and commit SHA.

### 5. Pull on quartz

On quartz:

```bash

cd "$HOME/code/tigervnc"
git fetch origin
git checkout <branch-name>
git pull --ff-only

```

If quartz uses a different checkout path, locate it first rather than creating a second divergent checkout.

### 6. Build on quartz

At minimum:

```bash

cmake --build build --target rfb
cmake --build build --target njcvncviewer

```text

If quartz has no existing `build/`, configure according to the repository’s standard macOS/quartz build instructions, then run the same targets.

### 7. Test on quartz

At minimum:

```bash

python3 -m py_compile tests/e2e/log_parser.py
git diff --check

```

Then run the project’s relevant viewer/PersistentCache e2e tests. Prefer tests that exercise:

- `PersistentCachedRectInit` with flags `0`.
- `PersistentCachedRectInit` with `native_format` flag and canonical PF.
- Subsequent `PersistentCachedRect` references by 16-byte `CacheKey`.
- Viewer rejection of malformed reserved flags.
- Cache accounting/log parser expectations for 36-byte references and 33-byte INIT v2 overhead.

### 8. If build fails

Do not paper over the failure with compatibility fallback. The user explicitly requested removal of legacy compatibility cruft. Fix forward against the v2-only contract unless the failure proves the intended contract itself is wrong.

Common failure areas to inspect:

- Missing include for `std::logic_error` in `SMsgWriter.cxx` if not already present.
- Exact namespace spelling for `CacheKey`, `PixelFormat`, `protocol_error`, and `ServerParams`.
- `InputStream::hasDataOrRestore()` restore-point semantics when optional PF is partially available.
- Whether the reader should check for `16 + 4` before reading PF plus inner encoding, or split checks to avoid restore-point misuse.
- Existing tests or docs still assuming legacy 8-byte IDs.

### 9. Success criteria

The cutover can be considered ready for final review when:

- Local source inspection passes.
- The local WIP commit has been pushed.
- Quartz has pulled that commit.
- Quartz builds `rfb` and `njcvncviewer` successfully.
- Relevant PersistentCache/viewer tests pass.
- No remaining live C++ viewer/server path accepts legacy flagless INIT for `PersistentCachedRectInit`.
- No `includeFlags` compatibility path remains.

## Last known blocker

As of the prior ARO session:

- local Mac: `cmake` not found
- quartz SSH: unavailable from ARO
- patch file: `/Users/nickc/tmp/tigervnc_cpp_pcache_cutover.patch.txt`
- branch: `master...origin/master` with uncommitted changes

Continue from the local working tree if possible. If not, apply the patch snapshot to a clean branch and repeat source inspection before committing.
