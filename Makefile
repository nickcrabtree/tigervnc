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

UNAME_S := $(shell uname -s)

# Default parallelism for cmake --build (all cores if detectable)
NUM_JOBS ?= $(shell sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)

# macOS: ensure CMake uses an installed SDK and the /usr/bin clang wrappers (which inject SDK include paths)
CMAKE_VIEWER_ARGS := -DBUILD_VIEWER=ON
ifeq ($(UNAME_S),Darwin)
MACOS_SDK_PATH := $(shell xcrun --sdk macosx --show-sdk-path 2>/dev/null)
ifneq ($(strip $(MACOS_SDK_PATH)),)
CMAKE_VIEWER_ARGS += -DCMAKE_OSX_SYSROOT="$(MACOS_SDK_PATH)" -DCMAKE_C_COMPILER=/usr/bin/clang -DCMAKE_CXX_COMPILER=/usr/bin/clang++
endif
endif

# Default: build viewer, server, and Rust viewer
all: viewer server rustviewer

# Viewer (CMake will (re)configure as needed and rebuild its dependencies)
viewer:
	@echo "[viewer] Configuring CMake with BUILD_VIEWER=ON using $${HOME}/bin/cmake if available..."
	@env MAKEFLAGS= CMAKE_BUILD_PARALLEL_LEVEL=$(NUM_JOBS) PATH="$${HOME}/bin:$${PATH}" cmake -S . -B $(BUILD_DIR) $(CMAKE_VIEWER_ARGS)
	@echo "[viewer] Building C++ viewer (njcvncviewer) via CMake..."
	@env MAKEFLAGS= CMAKE_BUILD_PARALLEL_LEVEL=$(NUM_JOBS) PATH="$${HOME}/bin:$${PATH}" cmake --build $(BUILD_DIR) --target njcvncviewer

# Server: build CMake library deps first, then the Xorg-based Xnjcvnc.
#
# We now rely on a static dependency map for the xserver tree stored in
# $(XSERVER_DEPMAP). The map is refreshed automatically (using
# tools/xserver_depmap.py refresh) when missing or older than a threshold,
# and rsync + targeted object invalidation (tools/xserver_depmap.py sync)
# are used instead of always cleaning the entire xserver build tree.
#
# IMPORTANT: The xserver autotools Makefile doesn't track librfb.a as a
# dependency, so we explicitly check if the binary needs relinking by
# comparing timestamps.
XNJCVNC_BIN := $(XSERVER_BUILD_DIR)/hw/vnc/Xnjcvnc
LIBRFB := $(BUILD_DIR)/common/rfb/librfb.a

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
	@# Force relink if librfb.a is newer than Xnjcvnc binary
	@if [ -f "$(XNJCVNC_BIN)" ] && [ -f "$(LIBRFB)" ] && [ "$(LIBRFB)" -nt "$(XNJCVNC_BIN)" ]; then \
		echo "[server] librfb.a is newer than Xnjcvnc - forcing relink..."; \
		rm -f "$(XNJCVNC_BIN)"; \
	fi
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
