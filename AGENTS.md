# AGENTS.md - tigervnc

> **Global rules:** see `~/code/AGENTS.md` for conventions that span all repos
> and machines. Those apply here too, in addition to anything below.

<!-- Selective repository startup: 2026-10-27 -->

Shared policy: `~/scripts/cross_repo_notes/AGENT_POLICY.md`; selective index:
`~/scripts/cross_repo_notes/code_AGENTS.md`. Read those compact files if not already supplied.
Current shared rules supersede older Git, test-runner, host, sandbox and quiet-window
recipes below. Read only task-relevant references before that operation. Dated runtime
states and measurements are historical evidence, not proof of today's deployed state.

## Project safeguards

C++/Rust VNC tools. NEVER interfere with production displays :1/:2/:3 or ports
5901/5902/5903. Do not restart/kill production VNC on Quartz or AWS without Nick's
explicit permission. No automated GUI viewer windows on Nick's Mac unless asked.
Use existing headless harnesses and isolated test displays :998/:999; inspect
exact PIDs before stopping a test process. Run potentially blocking commands
under timeout (on macOS, ~/bin/timeout). Code deployment uses Git, never rsync/scp.
Before protocol/cache work read the relevant references and
`rust-vnc-viewer/docs/CONVERGENCE_GATES.md`; PersistentCache IDs are u64, not old
16-byte hashes. Preserve assertions and the documented pixel-stride/commitBufferRW
contracts. Heavy builds follow current shared execution policy.

## Select the reference before acting

- Before operating or changing the listed subsystem: [reference](AGENT_GUIDE_OPERATIONS1.md).
  Topics: Agent Instructions; Server Infrastructure; ⚠️ ALWAYS USE TIMEOUTS; Starting and Managing Servers; SSH Tunnels;
  Quick Start; Binary Locations; Creating Release Packages; Core Libraries (common/); Log Locations; Debug Logging
  Framework; Cache Debugging; Stride is in Pixels, Not Bytes.
- Before the listed development, test, build or gate operation: [reference](AGENT_GUIDE_DEVELOPMENT1.md).
  Topics: Host Topology and Test Flow; Build Commands; Initial Configuration (first time only); Xserver Setup (required
  for server build); Running Tests; Testing Structure; Configuration; Test Results (November 2025); Code quality
  (desloppify).
- Before policy, safety, claims or workflow changes: [reference](AGENT_GUIDE_SAFETY1.md).
  Topics: VNC Viewer Testing Guardrail; Production VNC Server Guardrail; Key Conventions.
- Before changing the listed components or algorithms: [reference](AGENT_GUIDE_ARCHITECTURE1.md).
  Topics: Code Architecture; Platform-Specific Components; Important Code Patterns and Gotchas; PixelBuffer Access
  Patterns; Cache and PersistentCache Implementation; Protocol Overview.
- Before operating or changing the listed subsystem: [reference](AGENT_GUIDE_OPERATIONS2.md).
  Topics: Key Files; Rust viewer convergence / PersistentCache parity; Related Documentation; Shared utility scripts.

Detailed text remains in these ordinary references, not automatic imports.
