# Top-level Makefile for TigerVNC convenience builds
# Targets:
#   make          -> builds viewer, server, and Rust viewer
#   make viewer   -> builds vncviewer (via CMake)
#   make server   -> builds Xnjcvnc (via CMake libs + Xorg autotools)
#   make rustviewer -> builds Rust viewer (via Cargo)

.PHONY: all viewer server rust_viewer rustviewer clean

BUILD_DIR := build
XSERVER_BUILD_DIR := $(BUILD_DIR)/unix/xserver
XSERVER_DEPMAP := $(XSERVER_BUILD_DIR)/.tigervnc_depmap.json

# Default: build viewer, server, and Rust viewer
all: viewer server rustviewer

# Viewer (CMake will rebuild its dependencies as needed)
viewer:
	cmake --build $(BUILD_DIR) --target njcvncviewer

# Server: build CMake library deps first, then the Xorg-based Xnjcvnc.
#
# We now rely on a static dependency map for the xserver tree stored in
# $(XSERVER_DEPMAP). The map is refreshed automatically (using
# tools/xserver_depmap.py refresh) when missing or older than a threshold,
# and rsync + targeted object invalidation (tools/xserver_depmap.py sync)
# are used instead of always cleaning the entire xserver build tree.
server:
	@echo "[server] Ensuring Xserver dependency map is up to date..."
	@python3 tools/xserver_depmap.py refresh
	@echo "[server] Syncing TigerVNC unix/xserver sources to build tree and invalidating stale objects..."
	@python3 tools/xserver_depmap.py sync
	@echo "[server] Rebuilding core libraries (CMake)..."
	cmake --build $(BUILD_DIR) --target core
	cmake --build $(BUILD_DIR) --target rdr
	cmake --build $(BUILD_DIR) --target network
	cmake --build $(BUILD_DIR) --target rfb
	cmake --build $(BUILD_DIR) --target unixcommon
	@echo "[server] Rebuilding Xnjcvnc server incrementally..."
	$(MAKE) -C $(XSERVER_BUILD_DIR) TIGERVNC_SRCDIR=$(CURDIR) TIGERVNC_BUILDDIR=$(CURDIR)/$(BUILD_DIR)

# Rust Viewer: Build the Rust-based njcvncviewer-rs (cargo will rebuild deps as needed)
rust_viewer:
	@echo "Building Rust VNC viewer (njcvncviewer-rs)..."
	cargo build --manifest-path rust-vnc-viewer/Cargo.toml --release -p njcvncviewer-rs
	@mkdir -p $(BUILD_DIR)/vncviewer
	@ln -sf $(abspath rust-vnc-viewer/target/release/njcvncviewer-rs) $(BUILD_DIR)/vncviewer/njcvncviewer-rs
	@echo "Done. Binary: rust-vnc-viewer/target/release/njcvncviewer-rs"
	@echo "      Symlink: $(BUILD_DIR)/vncviewer/njcvncviewer-rs"

# User-facing target name for the Rust viewer (matches README expectation)
rustviewer: rust_viewer

# Clean: remove all build artifacts including depmap
clean:
	@echo "[clean] Cleaning CMake build..."
	@cmake --build $(BUILD_DIR) --target clean 2>/dev/null || true
	@echo "[clean] Cleaning xserver build..."
	@$(MAKE) -C $(XSERVER_BUILD_DIR) clean 2>/dev/null || true
	@echo "[clean] Cleaning Rust build..."
	@cargo clean --manifest-path rust-vnc-viewer/Cargo.toml 2>/dev/null || true
	@echo "[clean] Removing xserver dependency map..."
	@rm -f $(XSERVER_DEPMAP)
	@echo "[clean] Done"
