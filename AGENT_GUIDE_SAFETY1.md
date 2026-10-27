# Agent guide: safety1

Read when selected by AGENTS.md, not at every startup. Paths and commands are
relative to the repository root. Detailed source sections are retained below.
Current AGENTS.md and shared policy supersede dated history: old quiet-window
times, expired governor lifts, amend/reset advice and older host assumptions do
not grant permission. For current quiet-window status consult
`~/scripts/cross_repo_notes/AGENT_POLICY.md`.

## Detailed reference

## VNC Viewer Testing Guardrail

Do not launch GUI VNC viewer windows on Nick's Mac during automated testing,
trace capture, or ARO-driven investigation unless Nick explicitly asks for a
local interactive viewer.
Use quartz and headless VNC server/test harness workflows for
Rust TigerVNC viewer testing. Follow the existing tests for how to exercise
protocol/cache behaviour without opening a macOS viewer window.

## Production VNC Server Guardrail

Do not touch, restart, reconfigure, connect automated experiments to, or
otherwise interfere with Nick's production VNC servers on ports 5901, 5902, or
5903. Treat those ports as reserved unless Nick explicitly authorises their use
for a specific interactive task.
For automated Rust/C++ VNC viewer testing, copy the existing C++ viewer
headless test harness pattern and create isolated throwaway VNC servers on
non-production ports. Do not invent ad-hoc local macOS GUI viewer runs or point
experiments at the production servers.

This repo runs two VNC server binaries with production instances that must
**never** be killed, beyond the global no-pkill policy in `~/code/AGENT_POLICY.md`
(kill only specific, verified PIDs — never `pkill`/`killall`):

```text
Xtigervnc :1  # port 5901 - PRODUCTION
Xtigervnc :2  # port 5902 - PRODUCTION (user's active desktop)
Xnjcvnc   :3  # port 5903 - PRODUCTION (session cache/PersistentCache)
```

On macOS: `njcvncviewer` (user's active VNC viewer sessions) and `Xvfb` (may be legitimate) are similarly off-limits
without explicit PID verification.

Test servers only run on isolated displays `:998`/`:999` (ports 6898/6899), managed by the `tests/e2e/` framework — safe
to kill by specific PID after verification:

```bash
ps aux | grep -E "Xnjcvnc :99[89]"   # only matches test displays
pwdx <PID>                           # verify working directory
ps -p <PID> -o pid,args=             # verify full command
kill <specific-verified-pid>         # only after verification
```

Never manually start viewers on displays `:1`, `:2`, or `:3` — these are the user's working desktop. Never kill or
restart the VNC server on AWS Jakarta or quartz without explicit user permission.

## Key Conventions

- **Build system**: CMake 3.10+ required; out-of-tree builds recommended (`-B build`). Default build type `Release`
  (assertions kept via `-UNDEBUG`). `-DCMAKE_BUILD_TYPE=Debug` gives `-Og` and `-Werror`.
- **Platform support**: Linux/Unix native with system X11 libs; macOS native (requires FLTK, produces .dmg); Windows via
  MinGW/MinGW-w64 (MSVC not supported, requires FLTK, produces .exe installer).
- **Optional features** (CMake options): `ENABLE_NLS`, `ENABLE_GNUTLS`, `ENABLE_NETTLE`, `ENABLE_H264`, `BUILD_VIEWER`,
  `BUILD_JAVA`, `ENABLE_ASAN`, `ENABLE_TSAN` — `AUTO` (build if found), `ON` (require), `OFF` (disable).
- **Dependencies**: required — zlib, pixman, libjpeg. Optional — FLTK 1.3.3+, GnuTLS 3.x, Nettle 3.0+, PAM, Xorg dev
  libs, ffmpeg, GoogleTest.
- **Cross-compilation**: see `BUILDING.txt` for MinGW details. Xnjcvnc requires patching Xorg source (`unix/xserver*.patch`).
- **Compiler flags**: C `-std=gnu99`, C++ `-std=gnu++11`. Strict warnings: `-Wall -Wextra -Wformat=2 -Wvla`. Debug
  builds treat warnings as errors (`-Werror`). Assertions active in all build types.
- Avoid unguarded `grep/find | head` pipelines under `set -euo pipefail`; they can return `rc=141` from SIGPIPE even
  when the generated output file is useful. Prefer bounded Python filtering, `awk`, `sed -n`, or append `|| true` around
  intentionally truncated pipelines.
