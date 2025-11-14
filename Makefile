# Top-level Makefile for TigerVNC convenience builds
# Targets:
#   make          -> builds viewer and server
#   make viewer   -> builds vncviewer (via CMake)
#   make server   -> builds Xvnc (via CMake libs + Xorg autotools)

.PHONY: all viewer server rust_viewer

BUILD_DIR := build
XSERVER_BUILD_DIR := $(BUILD_DIR)/unix/xserver

# Default: build both viewer and server
all: viewer server

# Viewer (CMake will rebuild its dependencies as needed)
viewer:
	cmake --build $(BUILD_DIR) --target njcvncviewer

# Server: build CMake library deps first, then the Xorg-based Xvnc
# Note: The xserver build uses a copy of TigerVNC source files in the build tree.
#       We sync them before building to pick up any source changes, using checksums
#       to detect content differences regardless of timestamps.
#
# Sledgehammer policy: because the xserver/autotools dependency tracking is
# flaky, we ALWAYS:
#   1. Clean the CMake build tree (so librdr/librfb/etc. are rebuilt).
#   2. Sync unix/xserver into the build tree.
#   3. Rebuild all CMake libs.
#   4. Clean the xserver tree and rebuild Xnjcvnc.
server:
	@echo "[server] Cleaning CMake build tree (sledgehammer)..."
	@cmake --build $(BUILD_DIR) --target clean || true
	@echo "[server] Syncing TigerVNC unix/xserver sources to build tree..."
	@rsync -a --checksum --itemize-changes unix/xserver/ $(XSERVER_BUILD_DIR)/ | grep -v '/$$' || true
	@echo "[server] Rebuilding core libraries..."
	cmake --build $(BUILD_DIR) --target core
	cmake --build $(BUILD_DIR) --target rdr
	cmake --build $(BUILD_DIR) --target network
	cmake --build $(BUILD_DIR) --target rfb
	cmake --build $(BUILD_DIR) --target unixcommon
	@echo "[server] Cleaning xserver build tree before rebuild (sledgehammer)..."
	@$(MAKE) -C $(XSERVER_BUILD_DIR) clean || true
	@echo "[server] Rebuilding Xnjcvnc server..."
	$(MAKE) -C $(XSERVER_BUILD_DIR) TIGERVNC_SRCDIR=$(CURDIR) TIGERVNC_BUILDDIR=$(CURDIR)/$(BUILD_DIR)

# Rust Viewer: Build the Rust-based njcvncviewer-rs (cargo will rebuild deps as needed)
rust_viewer:
	@echo "Building Rust VNC viewer (njcvncviewer-rs)..."
	cargo build --manifest-path rust-vnc-viewer/Cargo.toml --release -p njcvncviewer-rs
	@mkdir -p $(BUILD_DIR)/vncviewer
	@ln -sf $(abspath rust-vnc-viewer/target/release/njcvncviewer-rs) $(BUILD_DIR)/vncviewer/njcvncviewer-rs
	@echo "Done. Binary: rust-vnc-viewer/target/release/njcvncviewer-rs"
	@echo "      Symlink: $(BUILD_DIR)/vncviewer/njcvncviewer-rs"
