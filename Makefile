# Top-level Makefile for TigerVNC convenience builds
# Targets:
#   make          -> builds viewer and server
#   make viewer   -> builds vncviewer (via CMake)
#   make server   -> builds Xvnc (via CMake libs + Xorg autotools)

.PHONY: all viewer server

BUILD_DIR := build
XSERVER_BUILD_DIR := $(BUILD_DIR)/unix/xserver

# Default: build both viewer and server
all: viewer server

# Viewer (CMake will rebuild its dependencies as needed)
viewer:
	cmake --build $(BUILD_DIR) --target vncviewer

# Server: build CMake library deps first, then the Xorg-based Xvnc
server:
	cmake --build $(BUILD_DIR) --target rfb
	cmake --build $(BUILD_DIR) --target rdr
	cmake --build $(BUILD_DIR) --target network
	cmake --build $(BUILD_DIR) --target core
	cmake --build $(BUILD_DIR) --target unixcommon
	$(MAKE) -C $(XSERVER_BUILD_DIR) TIGERVNC_SRCDIR=$(CURDIR) TIGERVNC_BUILDDIR=$(CURDIR)/$(BUILD_DIR)
