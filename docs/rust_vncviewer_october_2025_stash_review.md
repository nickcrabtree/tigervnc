# Rust VNC Viewer October 2025 Stash Review

Reviewed in ARO session `rust vncviewer 25` on 2026-05-07.

The October Rust viewer stash was `stash@{2025-10-08 15:20:19 +0100}: On master: temp stash rust
viewer changes`.

Current validation showed `cargo check --workspace` succeeds for `rust-vnc-viewer` when using a
target directory under `~/tmp`.

Source-only stash classification found 33 unique source or documentation paths after excluding
generated `rust-vnc-viewer/target` artefacts.

The stash contains early Rust workspace notes and early helper files such as `GETTING_STARTED.md`,
`PROGRESS.md`, `STATUS.md`, `TASK_1.1_SUMMARY.md`, `rfb-common/src/config.rs`,
`rfb-common/src/cursor.rs`, and `rfb-protocol/src/io.rs`.

Decision: do not pop or apply the October Rust viewer stash. The current tree already contains the
active Rust workspace, and the stash also includes old generated target artefacts and older source
or documentation states. Preserve this review note, then drop only the verified October Rust viewer
stash.
