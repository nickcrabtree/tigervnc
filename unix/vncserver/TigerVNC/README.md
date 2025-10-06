# TigerVNC Perl Modules

These Perl modules are adapted from Ubuntu's tigervnc-standalone-server package (v1.14.1).
They provide the modern wrapper infrastructure for tigervncserver with features like:

- Automatic display number selection (finds next free display :1-:99)
- Configuration file support (/etc/tigervnc/ and ~/.vnc/)
- Command-line argument parsing and validation
- Remote server support
- Proper cleanup with -kill and -list options

## Usage

The `tigervncserver` wrapper script uses these modules to provide a user-friendly
interface to the Xvnc/Xtigervnc server. It automatically:

1. Finds the next available display number
2. Checks ports 5900+n (VNC) and 6000+n (X11) for availability
3. Configures authentication and security
4. Starts the X session with proper environment

## Integration

These files are integrated into the TigerVNC build system via CMakeLists.txt:
- The tigervncserver.in script is configured at build time
- Perl modules are installed to ${CMAKE_INSTALL_PREFIX}/share/perl5/TigerVNC/
- A symlink Xtigervnc -> Xvnc is created for compatibility

## Copyright

Copyright (C) 2021-2022 Joachim Falk <joachim.falk@gmx.de>

Licensed under GPL v2 or later. See individual files for full copyright notices.
