# Xvnc Version String Build System TODO

**Status**: Code changes complete, build system integration pending  
**Last Updated**: 2025-10-08

---

## Current Situation

### ✅ What's Done
- `generate_xvnc_version.sh` script created and working
- `xvnc.c` updated to use `XVNC_VERSION` from generated header
- `vncExtInit.cc` updated to use `XVNC_VERSION` in desktop name
- `Makefile.am` rules added for generating `xvnc_version.h`

### ⚠️ What's Broken
The Makefile rules in `unix/xserver/hw/vnc/Makefile.am` were added, but the generated `Makefile` in the build directory doesn't include them because:

1. **Autotools requires regeneration**: Changes to `.am` files need `autoreconf` to regenerate configure scripts
2. **Build directory is out of sync**: The `build/unix/xserver/hw/vnc/Makefile` doesn't know about the new rules

### Current Workaround
Manually generate the version header before building:
```bash
cd /home/nickc/code/tigervnc
unix/xserver/hw/vnc/generate_xvnc_version.sh \
    build/unix/xserver/hw/vnc/xvnc_version.h \
    1.15.80
```

---

## The Problem in Detail

### Makefile.am Changes Made

In `unix/xserver/hw/vnc/Makefile.am`, these lines were added:

```makefile
BUILT_SOURCES = xvnc_version.h
CLEANFILES = xvnc_version.h

xvnc_version.h: $(TIGERVNC_SRCDIR)/.git/HEAD $(TIGERVNC_SRCDIR)/.git/index
	$(srcdir)/generate_xvnc_version.sh $@ $(PACKAGE_VERSION)
```

### Why It's Not Working

1. **Autotools workflow**:
   ```
   Makefile.am  -->  [autoreconf]  -->  Makefile.in  -->  [configure]  -->  Makefile
   ```

2. **We changed**: `Makefile.am`
3. **But didn't run**: `autoreconf` or `configure`
4. **So**: `Makefile` in build directory has no idea about version header generation

---

## Solution Options

### Option A: Proper Autotools Regeneration (Recommended)

This is the correct way but requires rebuilding everything.

**Steps:**

1. **Navigate to xserver source**:
   ```bash
   cd ~/code/tigervnc/unix/xserver
   ```

2. **Regenerate autotools files**:
   ```bash
   autoreconf -fiv
   ```
   This regenerates `configure` and `Makefile.in` files from `.am` files.

3. **Re-run configure** (from build directory):
   ```bash
   cd ~/code/tigervnc/build/unix/xserver
   ../../../unix/xserver/configure \
       --prefix=/usr/local \
       --with-pic \
       --without-dtrace \
       --disable-static \
       --disable-dri \
       --disable-xinerama \
       --disable-xvfb \
       --disable-xnest \
       --disable-xorg \
       --disable-dmx \
       --disable-xwin \
       --disable-xephyr \
       --disable-kdrive \
       --with-xkb-path=/usr/share/X11/xkb \
       --with-xkb-output=/var/lib/xkb \
       --with-xkb-bin-directory=/usr/bin \
       --with-serverconfig-path=/usr/lib/xorg
   ```
   (Adjust configure flags to match your original build)

4. **Rebuild**:
   ```bash
   make -j$(nproc)
   ```

**Pros**:
- Proper solution
- Will work for all future builds
- Automatic version header generation

**Cons**:
- Requires finding original configure flags
- Full rebuild takes time
- Might encounter other build issues

---

### Option B: Manual Generation Hook (Quick Fix)

Add a script that generates the header before building.

**Create**: `~/code/tigervnc/build-xvnc.sh`

```bash
#!/bin/bash
# Helper script to build Xvnc with version metadata

set -e

TIGERVNC_ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$TIGERVNC_ROOT/build/unix/xserver/hw/vnc"
VERSION="1.15.80"

echo "Generating version header..."
"$TIGERVNC_ROOT/unix/xserver/hw/vnc/generate_xvnc_version.sh" \
    "$BUILD_DIR/xvnc_version.h" \
    "$VERSION"

echo "Building Xvnc..."
cd "$BUILD_DIR"
make -j$(nproc)

echo "Done! Binary: $BUILD_DIR/Xnjcvnc"
"$BUILD_DIR/Xnjcvnc" -version 2>&1 | head -5
```

**Usage:**
```bash
chmod +x ~/code/tigervnc/build-xvnc.sh
~/code/tigervnc/build-xvnc.sh
```

**Pros**:
- Quick
- No configure changes needed
- Easy to use

**Cons**:
- Not integrated with build system
- Need to remember to use script instead of `make`
- Header not automatically regenerated on git changes

---

### Option C: CMake Migration (Future)

The viewer already uses CMake. Migrating Xvnc to CMake would:
- Make build consistent across client/server
- Easier to maintain
- Better cross-platform support

**Status**: Not started, significant work

---

## Recommended Approach

**For now (quick)**: Use Option B (manual script)

**When time allows**: Implement Option A (proper autotools)

**Long term**: Consider Option C (CMake migration)

---

## Files Involved

### Source Files (git-tracked)
- `unix/xserver/hw/vnc/generate_xvnc_version.sh` - Version generation script
- `unix/xserver/hw/vnc/Makefile.am` - Build rules (needs autoreconf)
- `unix/xserver/hw/vnc/xvnc.c` - Uses XVNC_VERSION
- `unix/xserver/hw/vnc/vncExtInit.cc` - Uses XVNC_VERSION

### Generated Files (not tracked)
- `build/unix/xserver/hw/vnc/xvnc_version.h` - Auto-generated header
- `build/unix/xserver/hw/vnc/Makefile` - Needs regeneration from Makefile.am

---

## Testing After Fix

After implementing the fix, verify it works:

```bash
# 1. Check version header exists and is current
cat build/unix/xserver/hw/vnc/xvnc_version.h

# 2. Rebuild
cd build/unix/xserver/hw/vnc
make clean
make

# 3. Check version output
./Xnjcvnc -version 2>&1 | head -5
# Should show: TigerVNC 1.15.80+build.XXXX.yyyyyyy

# 4. Start server and check desktop name
# Client should see: user@host (TigerVNC 1.15.80+build.XXXX.yyyyyyy)
```

---

## Original Configure Flags

You'll need to find the original configure command used. Check:

```bash
# Look in build log
cat build/unix/xserver/config.log | grep configure

# Or check config.status
build/unix/xserver/config.status --version
```

Common TigerVNC configure flags:
```
--prefix=/usr/local
--with-pic
--without-dtrace
--disable-static
--disable-dri
--disable-xinerama
--enable-glx
```

---

## Related Commits

- `d0f624bb` - Add git build metadata to Xvnc version (initial)
- `d9958018` - Fix generate_xvnc_version.sh path handling
- Current: Waiting for build system integration

---

## Alternative: Package Version at Configure Time

Another option is to generate the version once at configure time rather than every build:

**In configure.ac**, add:
```m4
AC_MSG_CHECKING([git build version])
if test -d "$srcdir/../../../.git" ; then
    GIT_HASH=$(cd "$srcdir/../../.." && git rev-parse --short=7 HEAD)
    GIT_COUNT=$(cd "$srcdir/../../.." && git rev-list --count HEAD)
    XVNC_VERSION="$PACKAGE_VERSION+build.$GIT_COUNT.$GIT_HASH"
else
    XVNC_VERSION="$PACKAGE_VERSION"
fi
AC_SUBST([XVNC_VERSION])
AC_MSG_RESULT([$XVNC_VERSION])
```

**Pros**: Simpler, no script needed  
**Cons**: Version only updates on reconfigure, not on every build

---

## Summary

**Current State**: Code is ready, build system not integrated  
**Blocker**: Need to run `autoreconf` and reconfigure  
**Workaround**: Manual script to generate header before building  
**Fix**: Either Option A (proper) or Option B (quick)  

The functionality works - just needs build system plumbing!
