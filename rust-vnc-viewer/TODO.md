# Rust Viewer Consolidation - Remaining Work

**Status**: Phase 1 complete - `make rust_viewer` target added and working  
**Last Updated**: 2025-10-25

## Background

The repository currently has **two Rust VNC viewers**:
- **`njcvncviewer-rs`** - Main production viewer (canonical)
- **`rvncviewer`** - Experimental viewer with M1 fullscreen prototypes

Goal: Consolidate to a single viewer (`njcvncviewer-rs`) by porting features from `rvncviewer` and removing it.

## Completed ✅

- [x] Inventory workspace crates and binaries
- [x] Identify canonical viewer (njcvncviewer-rs) and experimental viewer (rvncviewer)
- [x] Audit feature differences (display enumeration, fullscreen controller, hotkeys)
- [x] Add `make rust_viewer` target to top-level Makefile
- [x] Verify build system works end-to-end
- [x] Update WARP.md with rust_viewer documentation

## Phase 2: Feature Migration

### TODO #1: Design Integration Architecture

**Goal**: Plan how M1 features integrate into njcvncviewer-rs

**Tasks**:
- [ ] Create module structure in `njcvncviewer-rs/src/`:
  ```
  njcvncviewer-rs/src/
  ├── display/
  │   └── mod.rs      # Cross-platform display enumeration
  ├── fullscreen/
  │   └── mod.rs      # Cross-platform fullscreen controller
  ├── app.rs          # Integrate controller + hotkeys
  └── main.rs         # Add --monitor CLI flag
  ```
- [ ] Document design decisions in `njcvncviewer-rs/docs/M1_FULLSCREEN_NOTES.md`
- [ ] Decide on CLI parameters:
  - `--fullscreen` (already exists)
  - `--monitor <primary|index|name>`
  - Consider: `--span-multi-monitor` for future

**Acceptance**: Clear module layout documented, ready to implement

---

### TODO #2: Port Display Enumeration

**Goal**: Copy display module from rvncviewer to njcvncviewer-rs

**Source**: `rvncviewer/src/display/mod.rs`

**Tasks**:
- [ ] Create `njcvncviewer-rs/src/display/mod.rs`
- [ ] Copy implementation:
  - `MonitorInfo` struct
  - `enumerate_monitors()` function (uses winit)
  - `select_monitor()` function (supports "primary", index, name)
- [ ] Add module declaration to `njcvncviewer-rs/src/main.rs`
- [ ] Test compilation on Linux (should work - uses winit)
- [ ] Verify no regressions in existing builds

**Acceptance**: Module compiles, tests pass on Linux

---

### TODO #3: Port Fullscreen Controller

**Goal**: Copy fullscreen controller from rvncviewer to njcvncviewer-rs

**Source**: `rvncviewer/src/fullscreen/mod.rs`

**Tasks**:
- [ ] Create `njcvncviewer-rs/src/fullscreen/mod.rs`
- [ ] Copy implementation:
  - `FullscreenState` struct
  - `FullscreenController` with methods:
    - `new()`, `toggle()`, `set_enabled()`
    - `set_target()` - select monitor
    - `next_monitor()`, `prev_monitor()` - navigation
    - `jump_to_monitor()`, `jump_to_primary()` - direct selection
    - `apply()` - send viewport command to egui
- [ ] Add module declaration to `njcvncviewer-rs/src/main.rs`
- [ ] Verify compilation

**Acceptance**: Module compiles, no runtime testing yet

---

### TODO #4: Add --monitor CLI Flag

**Goal**: Allow users to specify target monitor from command line

**File**: `njcvncviewer-rs/src/main.rs`

**Tasks**:
- [ ] Add CLI argument to `Args` struct:
  ```rust
  /// Monitor selection for fullscreen (primary, index, or name substring)
  #[arg(long)]
  monitor: Option<String>,
  ```
- [ ] Pass `args.monitor` to app initialization
- [ ] Update help text examples

**Acceptance**: `--help` shows `--monitor` option

---

### TODO #5: Integrate Fullscreen Controller into App

**Goal**: Wire controller into njcvncviewer-rs application lifecycle

**File**: `njcvncviewer-rs/src/app.rs`

**Tasks**:
- [ ] Add fields to `VncViewerApp`:
  ```rust
  fullscreen: FullscreenController,
  monitors: Vec<MonitorInfo>,
  fullscreen_pending: bool,
  ```
- [ ] In `VncViewerApp::new()`:
  - Call `enumerate_monitors()`
  - Initialize `FullscreenController`
  - Call `fullscreen.set_target()` with `--monitor` arg
  - Set initial fullscreen state from config/args
- [ ] In `update()` method, apply pending fullscreen state:
  ```rust
  if self.fullscreen_pending {
      self.fullscreen.apply(ctx);
      self.fullscreen_pending = false;
  }
  ```

**Acceptance**: App compiles, controller initialized at startup

---

### TODO #6: Add Keyboard Hotkeys

**Goal**: Add M1 fullscreen navigation hotkeys to app event loop

**File**: `njcvncviewer-rs/src/app.rs`

**Reference**: `rvncviewer/src/app.rs` lines 338-366

**Tasks**:
- [ ] In `update()` method, add input handling:
  - **F11** or **Ctrl+Alt+F**: Toggle fullscreen
  - **Ctrl+Alt+←**: Previous monitor
  - **Ctrl+Alt+→**: Next monitor
  - **Ctrl+Alt+0-9**: Jump to monitor by index
  - **Ctrl+Alt+P**: Jump to primary monitor
- [ ] Set `fullscreen_pending = true` when hotkey pressed
- [ ] Log monitor switches for debugging

**Example code**:
```rust
// Toggle fullscreen
if ctx.input(|i| i.key_pressed(egui::Key::F11) || 
    (i.modifiers.ctrl && i.modifiers.alt && i.key_pressed(egui::Key::F))) {
    self.fullscreen.toggle();
    self.fullscreen_pending = true;
}

// Monitor navigation (only in fullscreen)
if self.fullscreen.state().enabled {
    if ctx.input(|i| i.modifiers.ctrl && i.modifiers.alt && 
        i.key_pressed(egui::Key::ArrowLeft)) {
        self.fullscreen.prev_monitor(&self.monitors);
        self.fullscreen_pending = true;
    }
    // ... etc
}
```

**Acceptance**: Hotkeys compile, can test interactively

---

### TODO #7: Align Dependencies

**Goal**: Ensure Cargo.toml has required dependencies

**File**: `njcvncviewer-rs/Cargo.toml`

**Tasks**:
- [ ] Check `winit` version matches rvncviewer (should already be present via eframe)
- [ ] Verify no additional crates needed
- [ ] Run `cargo build --release -p njcvncviewer-rs`
- [ ] Fix any compilation errors

**Acceptance**: Clean build on Linux

---

## Phase 3: Testing

### TODO #8: End-to-End Testing Matrix

**Tasks**:

**Linux Testing** (primary development platform):
- [ ] `cargo run -p njcvncviewer-rs -- --help`
- [ ] Verify `--monitor` option appears in help
- [ ] Test basic connection: `cargo run -p njcvncviewer-rs -- localhost:2`
- [ ] Test fullscreen toggle with F11
- [ ] Verify no regressions in existing functionality

**macOS Testing** (if available):
- [ ] Build on macOS: `cargo build --release -p njcvncviewer-rs`
- [ ] Test monitor enumeration (check logs for detected monitors)
- [ ] Test `--monitor primary` flag
- [ ] Test `--monitor 0`, `--monitor 1` flags
- [ ] Test `--monitor <name>` with partial monitor name
- [ ] Test fullscreen hotkeys:
  - F11 toggle
  - Ctrl+Alt+← / → navigation
  - Ctrl+Alt+0-9 direct jump
  - Ctrl+Alt+P jump to primary
- [ ] Verify smooth transitions between monitors

**Acceptance**: All tests pass, no critical bugs

---

## Phase 4: Cleanup

### TODO #9: Remove rvncviewer Crate

**Goal**: Delete experimental viewer from repository

**Tasks**:
- [ ] Verify all features ported to njcvncviewer-rs
- [ ] Remove from workspace:
  - Edit `rust-vnc-viewer/Cargo.toml`
  - Remove `"rvncviewer"` from `members` array
- [ ] Delete directory:
  ```bash
  git rm -r rust-vnc-viewer/rvncviewer
  ```
- [ ] Search for remaining references:
  ```bash
  rg -n '\brvncviewer\b' rust-vnc-viewer/ -g '!build' -g '!.git'
  ```
- [ ] Rebuild workspace:
  ```bash
  cd rust-vnc-viewer
  cargo build --release
  ```
- [ ] Verify `cargo metadata --no-deps` shows no rvncviewer package

**Acceptance**: rvncviewer removed, workspace builds cleanly

---

### TODO #10: Update Documentation

**Goal**: Remove all references to rvncviewer from documentation

**Files to update**:
```bash
rust-vnc-viewer/README.md
rust-vnc-viewer/docs/ROADMAP.md
rust-vnc-viewer/docs/testing/M1_FULLSCREEN_TESTING.md
rust-vnc-viewer/M1_QUICKREF.md
rust-vnc-viewer/M1_SUMMARY.md
rust-vnc-viewer/PROGRESS_2025-10-24_M1_INITIAL.md
rust-vnc-viewer/NEXT_STEPS.md
rust-vnc-viewer/PERSISTENTCACHE_IMPLEMENTATION_PLAN.md
```

**Tasks**:
- [ ] Find all references:
  ```bash
  cd rust-vnc-viewer
  rg -n '\brvncviewer\b' -g '*.md'
  ```
- [ ] Replace with `njcvncviewer-rs` where appropriate:
  ```bash
  # Careful review first, then:
  perl -pi -e 's/\brvncviewer\b/njcvncviewer-rs/g' <file list>
  ```
- [ ] Update command examples in testing docs
- [ ] Update build instructions
- [ ] Review changes for any broken context

**Acceptance**: No `rvncviewer` references remain in docs

---

### TODO #11: Adjust CI and Scripts

**Goal**: Update any CI pipelines or scripts referencing rvncviewer

**Tasks**:
- [ ] Search for references:
  ```bash
  rg -n '\brvncviewer\b' .github .gitlab .circleci scripts tools 2>/dev/null
  ```
- [ ] Update CI build steps if any reference rvncviewer
- [ ] Update any developer scripts
- [ ] Test CI pipeline passes

**Acceptance**: CI green on Linux (and macOS if available)

---

## Phase 5: Finalization

### TODO #12: Document Changes

**Goal**: Create clear commit message and summary

**Tasks**:
- [ ] Write commit message:
  ```
  Consolidate Rust viewers into njcvncviewer-rs
  
  - Added make rust_viewer target to top-level Makefile
  - Ported M1 fullscreen and multi-monitor features from rvncviewer
  - Added --monitor CLI flag for display selection
  - Integrated fullscreen controller with keyboard hotkeys
  - Removed experimental rvncviewer crate
  - Updated all documentation to reference njcvncviewer-rs only
  
  Features:
  - Display enumeration with primary detection
  - Monitor selection: --monitor primary|index|name
  - Fullscreen hotkeys: F11, Ctrl+Alt+arrows, Ctrl+Alt+0-9, Ctrl+Alt+P
  - Seamless multi-monitor navigation
  ```
- [ ] Update CHANGELOG if one exists
- [ ] Update `rust-vnc-viewer/CONSOLIDATION_STATUS.md` to mark complete

**Acceptance**: Clear commit ready to push

---

### TODO #13: Post-Merge Housekeeping

**Goal**: Clean up developer environments

**Tasks**:
- [ ] Clean stale build artifacts:
  ```bash
  cargo clean -p rvncviewer 2>/dev/null || true
  ```
- [ ] Announce change to team (if applicable)
- [ ] Update any internal wikis or documentation
- [ ] Consider git tag: `git tag rust-viewer-consolidated`

**Acceptance**: Developers can `make rust_viewer` with no confusion

---

## Quick Reference

### Key Files

**Source (to port from)**:
- `rvncviewer/src/display/mod.rs` - Monitor enumeration (78 lines)
- `rvncviewer/src/fullscreen/mod.rs` - Fullscreen controller (79 lines)
- `rvncviewer/src/app.rs` lines 338-366 - Hotkey implementation

**Target (to port to)**:
- `njcvncviewer-rs/src/display/mod.rs` (create)
- `njcvncviewer-rs/src/fullscreen/mod.rs` (create)
- `njcvncviewer-rs/src/main.rs` - Add --monitor flag
- `njcvncviewer-rs/src/app.rs` - Integrate controller + hotkeys

### Build Commands

```bash
# Build Rust viewer
make rust_viewer

# Or directly:
cd rust-vnc-viewer
cargo build --release -p njcvncviewer-rs

# Run
./rust-vnc-viewer/target/release/njcvncviewer-rs --help
./rust-vnc-viewer/target/release/njcvncviewer-rs localhost:2
```

### Testing Commands

```bash
# Verify workspace state
cd rust-vnc-viewer
cargo metadata --no-deps | jq '.packages[].name'

# Search for rvncviewer references
rg -n '\brvncviewer\b' -g '!build' -g '!.git'

# Build and test
cargo build --release
cargo test --all
```

## Estimated Effort

- **Feature porting** (TODO #1-7): 4-6 hours
- **Testing** (TODO #8): 2-3 hours
- **Cleanup** (TODO #9-11): 1-2 hours
- **Documentation** (TODO #12-13): 1 hour

**Total**: 1-2 days of focused work

## Notes

- Development happens on main branch (no feature branches required)
- M1 features are cross-platform (use winit for display enumeration)
- `rvncviewer` was created in October 2024 for M1 prototype work
- `njcvncviewer-rs` is the canonical viewer with ContentCache support
- Workspace currently builds with warnings but no errors

## References

- **CONSOLIDATION_STATUS.md** - Detailed consolidation status
- **WARP.md** - Build system documentation
- **rust-vnc-viewer/README.md** - Current Rust viewer status
