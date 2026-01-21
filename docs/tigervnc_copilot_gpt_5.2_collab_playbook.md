# TigerVNC Collaboration Playbook (Nick’s macOS workflow)

This document captures the practical constraints and workflow conventions for
collaborating on the TigerVNC refactor work in Nick’s environment.

## 1) Environment constraints (what *will* bite you)

### 1.1 Chat copy/paste is not byte-safe

- Copy/paste into chat may be **HTML-escaped** (`&` → `&amp;`, `<` → `&lt;`,
  etc.), which corrupts patches, logs, and code snippets.
- Prefer **file-based transfers**: generate outputs into files under
  `/Volumes/Nick/tmp/` and upload those files.

### 1.2 Upload limitations

- Uploads may require source files to have a `.txt` suffix (commonly for
  `.cxx/.cpp/.c`).
- Header files (`.h/.hpp`) typically upload fine **without renaming**.
- There is a **3-file upload limit** per message (plan outputs accordingly).

### 1.3 Patch transport reliability

- Inline diffs frequently get mangled; do **not** paste patch text into chat.
- Preferred artifact style: provide **`*.patch` + `*.sha256`**, minimal hunks,
  exact context.
- If `git apply` fails, capture **byte-accurate context** with:

  - `gcat -A <file>` and
  - `nl -ba <file>`.

### 1.4 macOS tool differences

- `date -Is` is **GNU-only** and fails on macOS (use
  `date -u '+%Y-%m-%dT%H:%M:%SZ'` or `date +'%F_%H%M%S'`).
- The system uses MacPorts tools (`/opt/local/bin/...`) in some setups.

## 2) Canonical directory layout

- Repo root: `/Users/nickc/code/tigervnc`
- Build dir: `/Users/nickc/code/tigervnc/build`
- Upload/staging dir: `/Volumes/Nick/tmp/`

## 3) Logging: capture stdout+stderr for a command *block*

Avoid appending `2>&1` to each line. In `zsh`, wrap commands in a subshell and
redirect once.

### 3.1 Tee to screen + logfile (recommended)

```sh
LOGFILE="/Volumes/Nick/tmp/log_$(date +'%F_%H%M%S').log"
(
  set -uo pipefail
  exec > >(tee -a "$LOGFILE") 2>&1

  echo "=== START $(date -u '+%Y-%m-%dT%H:%M:%SZ') ==="
  echo "PWD: $(pwd)"
  echo

  cd /Users/nickc/code/tigervnc

  exit 1

  echo "=== step: rg ... ==="
  rg -n "pattern" -S path1 path2 || true

  echo "=== step: build ==="
  cmake --build build -j

  echo "=== DONE $(date -u '+%Y-%m-%dT%H:%M:%SZ') ==="
)

echo "Logged to $LOGFILE"
```

Notes:

- Avoid `set -e` in logging blocks unless you also add `|| true` to commands
  that may return non-zero (e.g., `rg` returns 1 when there are no matches).
- Ensure to run in a subshell so exits due to `set -uo pipefail` do not exit
  the user's terminal session entirely.

## 4) Staging files for upload (copy → rename only C/C++)

Nick’s typical pattern:

```sh
cd /Users/nickc/code/tigervnc
cp -v path/to/file1.cxx path/to/file2.c path/to/file3.cpp /Volumes/Nick/tmp/

# Rename only C/C++ sources for upload (zsh glob; (N) avoids “no matches”)
(
  cd /Volumes/Nick/tmp
  for i in *.(c cxx cpp)(N); do
    [ -f "$i" ] && mv -v "$i" "${i}.txt"
  done
)

ls -larth /Volumes/Nick/tmp | tail -20
```

If you need to stage a mix of files but avoid renaming headers, copy headers
separately or exclude `h/hpp` from the rename glob.

## 5) Patch workflow (robust in this environment)

### 5.1 Preferred: generate patches *locally* from the working tree

Because patches generated in-chat can be corrupted by formatting/escaping, the
most reliable approach is:

1) Edit locally.
2) Generate a patch from *your* tree:

   ```sh
   git diff > /Volumes/Nick/tmp/my_change_v1.patch
   ```

3) Generate checksum and verify:

   ```sh
   cd /Volumes/Nick/tmp
   shasum -a 256 my_change_v1.patch > my_change_v1.patch.sha256
   shasum -a 256 -c my_change_v1.patch.sha256
   ```

This “local edit → git diff artifact” approach proved reliable after repeated
patch-format/application failures.

### 5.2 Applying a patch

Nick commonly uses:

```sh
p=$(pwd)
f=the_patch_name.patch

cd /Volumes/Nick/tmp &&   shasum -c "${f}.sha256" &&   cd "$p" &&   echo "Dry run" &&   git apply --check "/Volumes/Nick/tmp/${f}" &&   echo "Real thing" &&   git apply --verbose "/Volumes/Nick/tmp/${f}" &&   echo "Patching done"
```

### 5.3 If `git apply` fails

Request / capture:

```sh
gcat -A path/to/file | sed -n 'START,ENDp'
nl -ba path/to/file | sed -n 'START,ENDp'
```

This exposes hidden whitespace/CRLF and line numbers.

## 6) Subshells: avoid exiting your terminal

When using `exit` in scripts/blocks, wrap in a subshell `( ... )` so failures do
not terminate the interactive shell session.

## 7) Build/test commands used in this repo

Typical:

```sh
cmake --build build -j
ctest --test-dir build --output-on-failure
```

Viewer-only:

```sh
make clean && make viewer
# or
cmake --build build --target njcvncviewer --verbose
```

## 8) Known gotchas we hit

- GoogleTest discovery during build can time out; increasing
  `DISCOVERY_TIMEOUT` fixed this previously.
- Patch “corrupt” errors often came from invalid unified diff formatting (blank
  lines inside hunks without proper prefixes) or from chat-escaped content.

---

**Keep this playbook updated** whenever a new toolchain quirk or workflow
constraint is discovered.
