TIGERVNC_SRCDIR=$(abspath ${top_srcdir}/../..)
TIGERVNC_BUILDDIR=$(abspath ${top_builddir}/../..)

RFB_LIB=$(TIGERVNC_BUILDDIR)/common/rfb/librfb.la
RDR_LIB=$(TIGERVNC_BUILDDIR)/common/rdr/librdr.la
OS_LIB=$(TIGERVNC_BUILDDIR)/common/os/libos.la
NETWORK_LIB=$(TIGERVNC_BUILDDIR)/common/network/libnetwork.la
UNIXCOMMON_LIB=$(TIGERVNC_BUILDDIR)/unix/common/libunixcommon.la
COMMON_LIBS=$(UNIXCOMMON_LIB) $(RFB_LIB) $(NETWORK_LIB) $(RDR_LIB) $(CORE_LIB)

AM_CPPFLAGS = \
	-I$(TIGERVNC_BUILDDIR) \
	-I$(TIGERVNC_SRCDIR)/common \
	-I$(TIGERVNC_SRCDIR)/unix/common \
	$(DIX_CFLAGS) $(LIBXCVT_CFLAGS)

AM_CXXFLAGS = -std=gnu++11

noinst_LTLIBRARIES = libvnccommon.la

HDRS = vncExtInit.h vncHooks.h \
	vncBlockHandler.h vncPresent.h vncSelection.h \
	XorgGlue.h XserverDesktop.h xorg-version.h \
	vncInput.h RFBGlue.h

BUILT_SOURCES = xvnc_version.h
CLEANFILES = xvnc_version.h

xvnc_version.h: $(TIGERVNC_SRCDIR)/.git/HEAD $(TIGERVNC_SRCDIR)/.git/index
	TIGERVNC_SRCDIR=$(TIGERVNC_SRCDIR) $(srcdir)/generate_xvnc_version.sh $@ $(PACKAGE_VERSION)

libvnccommon_la_SOURCES = $(HDRS) \
	vncExt.c vncExtInit.cc vncHooks.c vncPresent.c vncSelection.c \
	vncBlockHandler.c XorgGlue.c RandrGlue.c RFBGlue.cc XserverDesktop.cc \
	vncInput.c vncInputXKB.c qnum_to_xorgevdev.c qnum_to_xorgkbd.c

libvnccommon_la_CPPFLAGS = $(AM_CPPFLAGS) \
	-I$(TIGERVNC_SRCDIR)/unix/vncconfig

bin_PROGRAMS = Xnjcvnc

man1_MANS = Xnjcvnc.man

Xnjcvnc_SOURCES = xvnc.c \
	$(top_srcdir)/Xi/stubs.c $(top_srcdir)/mi/miinitext.c \
	buildtime.c

# Xnjcvnc contains no C++ sources so automake doesn't understand that we
# need to use the C++ compiler to link things. This is the upstream
# recommendation for coaxing automake.
nodist_EXTRA_Xnjcvnc_SOURCES = dummy.cxx

Xnjcvnc_CPPFLAGS = $(AM_CPPFLAGS)

LOCAL_LIBS = \
	$(XVNC_LIBS) \
	$(XSERVER_LIBS) \
	libvnccommon.la \
	$(COMMON_LIBS)

Xnjcvnc_DEPENDENCIES = $(LOCAL_LIBS)
Xnjcvnc_LDADD = \
	$(LOCAL_LIBS) \
	$(XSERVER_SYS_LIBS) \
	$(XVNC_SYS_LIBS) \
	$(LIBXCVT_LIBS)

Xnjcvnc_LDFLAGS = $(LD_EXPORT_SYMBOLS_FLAG)

if DRI3
Xnjcvnc_SOURCES += vncDRI3.h vncDRI3.c vncDRI3Draw.c
Xnjcvnc_CPPFLAGS += \
	-I$(top_srcdir)/dri3 \
	$(GBM_CFLAGS)
Xnjcvnc_LDADD += $(GBM_LIBS)
endif

libvnc_la_LTLIBRARIES = libvnc.la
libvnc_ladir = $(moduledir)/extensions

libvnc_la_SOURCES = vncModule.c

# See Xnjcvnc magic above
nodist_EXTRA_libvnc_la_SOURCES = dummy.cxx

libvnc_la_CPPFLAGS = $(AM_CPPFLAGS) \
	-I$(top_srcdir)/hw/xfree86/common \
	-I$(top_srcdir)/hw/xfree86/os-support \
	-I$(top_srcdir)/hw/xfree86/os-support/bus

libvnc_la_LDFLAGS = -module -avoid-version -Wl,-z,now

libvnc_la_LIBADD = libvnccommon.la $(COMMON_LIBS)

EXTRA_DIST = Xnjcvnc.man
