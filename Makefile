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
server:
	@echo "Syncing TigerVNC source files to xserver build directory..."
	@rsync -a --checksum --itemize-changes unix/xserver/ $(XSERVER_BUILD_DIR)/ | grep -v '/$$' || true
	cmake --build $(BUILD_DIR) --target rfb
	cmake --build $(BUILD_DIR) --target rdr
	cmake --build $(BUILD_DIR) --target network
	cmake --build $(BUILD_DIR) --target core
	cmake --build $(BUILD_DIR) --target unixcommon
	$(MAKE) -C $(XSERVER_BUILD_DIR) TIGERVNC_SRCDIR=$(CURDIR) TIGERVNC_BUILDDIR=$(CURDIR)/$(BUILD_DIR)

# Rust Viewer: Build the Rust-based njcvncviewer-rs (cargo will rebuild deps as needed)
rust_viewer:
	@echo "Building Rust VNC viewer (njcvncviewer-rs)..."
	cargo build --manifest-path rust-vnc-viewer/Cargo.toml --release -p njcvncviewer-rs
	@mkdir -p $(BUILD_DIR)/vncviewer
	@ln -sf $(abspath rust-vnc-viewer/target/release/njcvncviewer-rs) $(BUILD_DIR)/vncviewer/njcvncviewer-rs
	@echo "Done. Binary: rust-vnc-viewer/target/release/njcvncviewer-rs"
	@echo "      Symlink: $(BUILD_DIR)/vncviewer/njcvncviewer-rs"
