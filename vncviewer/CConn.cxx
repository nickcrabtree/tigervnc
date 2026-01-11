/* Copyright (C) 2002-2005 RealVNC Ltd.  All Rights Reserved.
 * Copyright (C) 2011 D. R. Commander.  All Rights Reserved.
 * Copyright 2009-2014 Pierre Ossman for Cendio AB
 * 
 * This is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 * 
 * This software is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 * 
 * You should have received a copy of the GNU General Public License
 * along with this software; if not, write to the Free Software
 * Foundation, Inc., 59 Temple Place - Suite 330, Boston, MA  02111-1307,
 * USA.
 */

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <build_version.h>

#include <assert.h>
#ifndef _WIN32
#include <unistd.h>
#include <csignal>
#include <sys/stat.h>
#endif
#include <cstdlib>
#include <fstream>

#include <core/LogWriter.h>
#include <core/Timer.h>
#include <core/string.h>
#include <core/time.h>

#include <rdr/FdInStream.h>
#include <rdr/FdOutStream.h>

#include <rfb/CMsgWriter.h>
#include <rfb/CSecurity.h>
#include <rfb/Exception.h>
#include <rfb/Security.h>
#include <rfb/fenceTypes.h>
#include <rfb/screenTypes.h>

#include <network/TcpSocket.h>
#ifndef WIN32
#include <network/UnixSocket.h>
#endif

#include <FL/Fl.H>
#include <FL/fl_ask.H>

#include "CConn.h"
#include "OptionsDialog.h"
#include "DesktopWindow.h"
#include "PlatformPixelBuffer.h"
#include "i18n.h"
#include "parameters.h"
#include "vncviewer.h"

#ifdef WIN32
#include "win32.h"
#endif

static core::LogWriter vlog("CConn");

// Global pointer for signal handler and shutdown logging
static CConn* g_activeConn = nullptr;
static volatile sig_atomic_t g_verifyRequested = 0;
static volatile sig_atomic_t g_refreshRequested = 0;
static bool g_logStatsRegistered = false;

static void logStatsAtExit()
{
  if (!g_activeConn)
    return;

  vlog.info("Framebuffer statistics:");
  g_activeConn->logFramebufferStats();
}

// Signal handler for SIGUSR1 - trigger framebuffer verification
#ifndef WIN32
static void handleVerifySignal(int sig)
{
  (void)sig;
  g_verifyRequested = 1;
  // Wake up FLTK event loop so the flag gets checked even if no network activity
  Fl::awake();
}

// Signal handler for SIGUSR2 - trigger safe full framebuffer refresh
static void handleRefreshSignal(int sig)
{
  (void)sig;
  g_refreshRequested = 1;
  // Wake up FLTK event loop so the flag gets checked even if no network activity
  Fl::awake();
}
#endif

// 8 colours (1 bit per component)
static const rfb::PixelFormat verylowColourPF(8, 3,false, true,
                                              1, 1, 1, 2, 1, 0);
// 64 colours (2 bits per component)
static const rfb::PixelFormat lowColourPF(8, 6, false, true,
                                          3, 3, 3, 4, 2, 0);
// 256 colours (2-3 bits per component)
static const rfb::PixelFormat mediumColourPF(8, 8, false, true,
                                             7, 7, 3, 5, 2, 0);

// Time new bandwidth estimates are weighted against (in ms)
static const unsigned bpsEstimateWindow = 1000;

CConn::CConn()
  : serverPort(0), sock(nullptr), desktop(nullptr),
    updateCount(0), pixelCount(0),
    lastServerEncoding((unsigned int)-1),
    hourlyStatsTimer(this, &CConn::handleHourlyStats),
    bpsEstimate(20000000),
    verificationInProgress_(false), savedFBWidth_(0), savedFBHeight_(0)
{
  // Record session start time for aggregate bandwidth statistics
  gettimeofday(&sessionStartTime, nullptr);
  setShared(::shared);
 
  supportsLocalCursor = true;
  supportsCursorPosition = true;
  supportsDesktopResize = true;
  supportsLEDState = true;
  
  // Cache protocol support (controlled by command-line/config file).
  // The ContentCache option is now an alias for PersistentCache with
  // an ephemeral (memory-only) policy: both enable the same on-wire
  // PersistentCache protocol (-321), but disk I/O is controlled solely
  // by the PersistentCache parameter in the DecodeManager.
  supportsContentCache = false;  // no separate ContentCache protocol
  supportsPersistentCache = (::persistentCache || ::contentCache);

  if (customCompressLevel)
    setCompressLevel(::compressLevel);

  if (!noJpeg)
    setQualityLevel(::qualityLevel);

  OptionsDialog::addCallback(handleOptions, this);
  
  // Set global pointer for signal handler and install signal handlers
  g_activeConn = this;
#ifndef WIN32
  signal(SIGUSR1, handleVerifySignal);
  signal(SIGUSR2, handleRefreshSignal);
  vlog.info("Framebuffer verification + debug dump: kill -USR1 %d", (int)getpid());
  vlog.info("Full framebuffer refresh: kill -USR2 %d", (int)getpid());
#endif

  // Register an atexit handler once to dump framebuffer/cache statistics
  if (!g_logStatsRegistered) {
    atexit(logStatsAtExit);
    g_logStatsRegistered = true;
  }
}

CConn::~CConn()
{
  hourlyStatsTimer.stop();

  // Dump framebuffer/cache statistics at the end of the session so that
  // end-to-end tests (and users) can see actual bandwidth savings.
  // This mirrors the server-side behaviour where VNCServerST logs
  // EncodeManager statistics on connection teardown.
  vlog.info("Framebuffer statistics:");
  logFramebufferStats();

  // Aggregate network statistics for this viewer session. We use the
  // underlying socket streams to estimate total bytes transferred and
  // average throughput.
  if (sock) {
    struct timeval now;
    gettimeofday(&now, nullptr);

    unsigned long long elapsedUsec =
      (unsigned long long)(now.tv_sec - sessionStartTime.tv_sec) * 1000000ULL +
      (unsigned long long)(now.tv_usec - sessionStartTime.tv_usec);
    if (elapsedUsec == 0)
      elapsedUsec = 1;

    double seconds = (double)elapsedUsec / 1e6;
    uint64_t rxBytes = sock->inStream().pos();
    uint64_t txBytes = sock->outStream().bytesWritten();

    double rxBitsPerSec = (double)rxBytes * 8.0 / seconds;
    double txBitsPerSec = (double)txBytes * 8.0 / seconds;

    vlog.info("Network summary: duration=%.1fs, rx=%s, tx=%s",
              seconds,
              core::iecPrefix(rxBytes, "B").c_str(),
              core::iecPrefix(txBytes, "B").c_str());
    vlog.info("Network throughput: rx≈%d kbit/s, tx≈%d kbit/s",
              (int)(rxBitsPerSec / 1000.0),
              (int)(txBitsPerSec / 1000.0));
  }

  // Clear global pointer so the atexit handler won't try to access a
  // destroyed connection object.
  if (g_activeConn == this)
    g_activeConn = nullptr;
  
  close();

  OptionsDialog::removeCallback(handleOptions);
  Fl::remove_timeout(handleUpdateTimeout, this);

  if (desktop)
    delete desktop;

  if (sock) {
    struct timeval now;

    sock->shutdown();

    // Do a graceful close by waiting for the peer (up to 250 ms)
    // FIXME: should do this asynchronously
    gettimeofday(&now, nullptr);
    while (core::msSince(&now) < 250) {
      bool done;

      done = false;
      while (true) {
        try {
          sock->inStream().skip(sock->inStream().avail());
          if (!sock->inStream().hasData(1))
            break;
        } catch (std::exception&) {
          done = true;
          break;
        }
      }

      if (done)
        break;

  #ifdef WIN32
      Sleep(10);
  #else
      usleep(10000);
  #endif
    }

    Fl::remove_fd(sock->getFd());

    delete sock;
  }
}

void CConn::connect(const char* vncServerName, network::Socket* socket)
{
  sock = socket;
  if(sock == nullptr) {
    try {
#ifndef WIN32
      if (strchr(vncServerName, '/') != nullptr) {
        sock = new network::UnixSocket(vncServerName);
        serverHost = sock->getPeerAddress();
        vlog.info(_("Connected to socket %s"), serverHost.c_str());
      } else
#endif
      {
        network::getHostAndPort(vncServerName, &serverHost, &serverPort);

        sock = new network::TcpSocket(serverHost.c_str(), serverPort);
        vlog.info(_("Connected to host %s port %d"),
                  serverHost.c_str(), serverPort);
      }
    } catch (std::exception& e) {
      vlog.error("%s", e.what());
      abort_connection(_("Failed to connect to \"%s\":\n\n%s"),
                       vncServerName, e.what());
      return;
    }
  }

  Fl::add_fd(sock->getFd(), FL_READ | FL_EXCEPT, socketEvent, this);

  setServerName(serverHost.c_str());
  setStreams(&sock->inStream(), &sock->outStream());

  initialiseProtocol();
}

std::string CConn::connectionInfo()
{
  std::string infoText;

  char pfStr[100];

  infoText += core::format(_("Desktop name: %.80s"), server.name());
  infoText += "\n";

  infoText += core::format(_("Host: %.80s port: %d"),
                           serverHost.c_str(), serverPort);
  infoText += "\n";

  infoText += core::format(_("Size: %d x %d"),
                           server.width(), server.height());
  infoText += "\n";

  // TRANSLATORS: Will be filled in with a string describing the
  // protocol pixel format in a fairly language neutral way
  server.pf().print(pfStr, 100);
  infoText += core::format(_("Pixel format: %s"), pfStr);
  infoText += "\n";

  infoText += core::format(_("Requested encoding: %s"),
                           rfb::encodingName(getPreferredEncoding()));
  infoText += "\n";

  infoText += core::format(_("Last used encoding: %s"),
                           rfb::encodingName(lastServerEncoding));
  infoText += "\n";

  infoText += core::format(_("Line speed estimate: %d kbit/s"),
                           (int)(bpsEstimate / 1000));
  infoText += "\n";

  infoText += core::format(_("Protocol version: %d.%d"),
                           server.majorVersion, server.minorVersion);
  infoText += "\n";

  infoText += core::format(_("Security method: %s"),
                           rfb::secTypeName(csecurity->getType()));
  infoText += "\n";

  return infoText;
}

unsigned CConn::getUpdateCount()
{
  return updateCount;
}

unsigned CConn::getPixelCount()
{
  return pixelCount;
}

unsigned CConn::getPosition()
{
  return sock->inStream().pos();
}

void CConn::logFramebufferStats()
{
  logDecodeStats();
}

void CConn::handleHourlyStats(core::Timer* t)
{
  (void)t;

  vlog.info("Hourly framebuffer statistics:");
  logFramebufferStats();

  if (sock) {
    struct timeval now;
    gettimeofday(&now, nullptr);

    unsigned long long elapsedUsec =
      (unsigned long long)(now.tv_sec - sessionStartTime.tv_sec) * 1000000ULL +
      (unsigned long long)(now.tv_usec - sessionStartTime.tv_usec);
    if (elapsedUsec == 0)
      elapsedUsec = 1;

    double seconds = (double)elapsedUsec / 1e6;
    uint64_t rxBytes = sock->inStream().pos();
    uint64_t txBytes = sock->outStream().bytesWritten();

    double rxBitsPerSec = (double)rxBytes * 8.0 / seconds;
    double txBitsPerSec = (double)txBytes * 8.0 / seconds;

    vlog.info("Hourly network summary: duration=%.1fs, rx=%s, tx=%s",
              seconds,
              core::iecPrefix(rxBytes, "B").c_str(),
              core::iecPrefix(txBytes, "B").c_str());
    vlog.info("Hourly network throughput: rx≈%d kbit/s, tx≈%d kbit/s",
              (int)(rxBitsPerSec / 1000.0),
              (int)(txBitsPerSec / 1000.0));
  }

  // Re-arm for the next hour without accumulating drift
  if (t)
    t->repeat();
}

void CConn::socketEvent(FL_SOCKET fd, void *data)
{
  CConn *cc;
  static bool recursing = false;
  int when;

  assert(data);
  cc = (CConn*)data;

  // I don't think processMsg() is recursion safe, so add this check
  assert(!recursing);

  recursing = true;
  Fl::remove_fd(fd);

  try {
    // We might have been called to flush unwritten socket data
    cc->sock->outStream().flush();

    cc->getOutStream()->cork(true);

    // processMsg() only processes one message, so we need to loop
    // until the buffers are empty or things will stall.
    while (cc->processMsg()) {

      // Make sure that the FLTK handling and the timers gets some CPU
      // time in case of back to back messages
      Fl::check();
      core::Timer::checkTimeouts();

      // Also check if we need to stop reading and terminate
      if (should_disconnect())
        break;
    }
    
    // Check if framebuffer verification was requested via SIGUSR1
    // This also triggers a coordinated debug dump on both client and server
#ifndef WIN32
    if (g_verifyRequested) {
      g_verifyRequested = 0;
      // First dump debug state (before any updates change things)
      cc->dumpCorruptionDebugInfo();
      // Then request verification
      cc->verifyFramebuffer();
    }

    // Check if a safe full refresh was requested via SIGUSR2
    if (g_refreshRequested) {
      g_refreshRequested = 0;
      vlog.info("SIGUSR2: calling refreshFramebuffer()");
      cc->refreshFramebuffer();
      vlog.info("SIGUSR2: refreshFramebuffer() returned, forcing display update");
      if (cc->desktop) {
        cc->desktop->updateWindow();
        Fl::flush();
      }
    }
#endif

    cc->getOutStream()->cork(false);
  } catch (rdr::end_of_stream& e) {
    vlog.info("%s", e.what());
    if (!cc->desktop) {
      vlog.error(_("The connection was dropped by the server before "
                   "the session could be established."));
      abort_connection(_("The connection was dropped by the server "
                       "before the session could be established."));
    } else {
      disconnect();
    }
  } catch (rfb::auth_cancelled& e) {
    vlog.info("%s", e.what());
    disconnect();
  } catch (rfb::auth_error& e) {
    cc->resetPassword();
    vlog.error(_("Authentication failed: %s"), e.what());
    abort_connection(_("Failed to authenticate with the server. Reason "
                       "given by the server:\n\n%s"), e.what());
  } catch (std::exception& e) {
    vlog.error("%s", e.what());
    abort_connection_with_unexpected_error(e);
  }

  when = FL_READ | FL_EXCEPT;
  if (cc->sock->outStream().hasBufferedData())
    when |= FL_WRITE;

  Fl::add_fd(fd, when, socketEvent, data);
  recursing = false;
}

void CConn::resetPassword()
{
    dlg.resetPassword();
}

////////////////////// CConnection callback methods //////////////////////

bool CConn::showMsgBox(rfb::MsgBoxFlags flags, const char *title,
                       const char *text)
{
    return dlg.showMsgBox(flags, title, text);
}

void CConn::getUserPasswd(bool secure, std::string *user,
                          std::string *password)
{
    dlg.getUserPasswd(secure, user, password);
}

// initDone() is called when the serverInit message has been received.  At
// this point we create the desktop window and display it.  We also tell the
// server the pixel format and encodings to use and request the first update.
void CConn::initDone()
{
  // Log client and server versions for debugging
  vlog.info("Client version: %s", BUILD_VERSION);
  vlog.info("Server name: %s", server.name());
  vlog.info("Server protocol: %d.%d", server.majorVersion, server.minorVersion);

  // Periodic stats reporting so long-running sessions still emit bandwidth/
  // cache statistics even if the viewer is never cleanly exited.
  hourlyStatsTimer.start(3600 * 1000);

  // If using AutoSelect with old servers, start in FullColor
  // mode. See comment in autoSelectFormatAndEncoding. 
  if (server.beforeVersion(3, 8) && autoSelect)
    fullColour.setParam(true);

  desktop = new DesktopWindow(server.width(), server.height(), this);
  fullColourPF = desktop->getPreferredPF();

  // Force a switch to the format and encoding we'd like
  updateEncoding();
  updatePixelFormat();
}

void CConn::setExtendedDesktopSize(unsigned reason, unsigned result,
                                   int w, int h,
                                   const rfb::ScreenSet& layout)
{
  CConnection::setExtendedDesktopSize(reason, result, w, h, layout);

  if (reason == rfb::reasonClient)
    desktop->setDesktopSizeDone(result);
}

// setName() is called when the desktop name changes
void CConn::setName(const char* name)
{
  CConnection::setName(name);
  desktop->updateCaption();
}

// framebufferUpdateStart() is called at the beginning of an update.
// Here we try to send out a new framebuffer update request so that the
// next update can be sent out in parallel with us decoding the current
// one.
void CConn::framebufferUpdateStart()
{
  CConnection::framebufferUpdateStart();

  // For bandwidth estimate
  gettimeofday(&updateStartTime, nullptr);
  updateStartPos = sock->inStream().pos();

  // Update the screen prematurely for very slow updates
  Fl::add_timeout(1.0, handleUpdateTimeout, this);
}

// framebufferUpdateEnd() is called at the end of an update.
// For each rectangle, the FdInStream will have timed the speed
// of the connection, allowing us to select format and encoding
// appropriately, and then request another incremental update.
void CConn::framebufferUpdateEnd()
{
  unsigned long long elapsed, bps, weight;
  struct timeval now;

  CConnection::framebufferUpdateEnd();

  updateCount++;

  // Calculate bandwidth everything managed to maintain during this update
  gettimeofday(&now, nullptr);
  elapsed = (now.tv_sec - updateStartTime.tv_sec) * 1000000;
  elapsed += now.tv_usec - updateStartTime.tv_usec;
  if (elapsed == 0)
    elapsed = 1;
  bps = (unsigned long long)(sock->inStream().pos() -
                             updateStartPos) * 8 *
                            1000000 / elapsed;
  // Allow this update to influence things more the longer it took, to a
  // maximum of 20% of the new value.
  weight = elapsed * 1000 / bpsEstimateWindow;
  if (weight > 200000)
    weight = 200000;
  bpsEstimate = ((bpsEstimate * (1000000 - weight)) +
                 (bps * weight)) / 1000000;

  Fl::remove_timeout(handleUpdateTimeout, this);
  desktop->updateWindow();
  Fl::flush();  // Force display flush to prevent stale rendering
  
  // Check if this was a verification update
  if (verificationInProgress_) {
    verificationInProgress_ = false;
    vlog.info("========== VERIFICATION UPDATE RECEIVED ==========" );
    
    // Compare received framebuffer with saved state
    rfb::ModifiablePixelBuffer* pb = getFramebuffer();
    if (!pb) {
      vlog.error("Cannot compare: framebuffer not available");
    } else if (savedFBWidth_ != server.width() || savedFBHeight_ != server.height()) {
      vlog.error("Cannot compare: framebuffer size changed during verification");
    } else {
      int width = savedFBWidth_;
      int height = savedFBHeight_;
      int bytesPerPixel = savedFBFormat_.bpp / 8;
      
      core::Rect rect(0, 0, width, height);
      int stride;
      const uint8_t* currentFB = pb->getBuffer(rect, &stride);
      
      size_t diffCount = 0;
      size_t totalPixels = width * height;
      
      // Compare pixel by pixel
      for (int y = 0; y < height; y++) {
        const uint8_t* currentRow = currentFB + (y * stride * bytesPerPixel);
        const uint8_t* savedRow = &savedFramebuffer_[y * width * bytesPerPixel];
        
        for (int x = 0; x < width; x++) {
          const uint8_t* currentPixel = currentRow + (x * bytesPerPixel);
          const uint8_t* savedPixel = savedRow + (x * bytesPerPixel);
          
          if (memcmp(currentPixel, savedPixel, bytesPerPixel) != 0) {
            diffCount++;
            
            // Log first 10 differences with pixel values
            if (diffCount <= 10) {
              char savedStr[32], currentStr[32];
              if (bytesPerPixel == 4) {
                snprintf(savedStr, sizeof(savedStr), "[%02x %02x %02x %02x]",
                        savedPixel[0], savedPixel[1], savedPixel[2], savedPixel[3]);
                snprintf(currentStr, sizeof(currentStr), "[%02x %02x %02x %02x]",
                        currentPixel[0], currentPixel[1], currentPixel[2], currentPixel[3]);
              } else if (bytesPerPixel == 3) {
                snprintf(savedStr, sizeof(savedStr), "[%02x %02x %02x]",
                        savedPixel[0], savedPixel[1], savedPixel[2]);
                snprintf(currentStr, sizeof(currentStr), "[%02x %02x %02x]",
                        currentPixel[0], currentPixel[1], currentPixel[2]);
              } else {
                snprintf(savedStr, sizeof(savedStr), "[...]");
                snprintf(currentStr, sizeof(currentStr), "[...]");
              }
              vlog.error("DIFFERENCE at (%d,%d): saved=%s current=%s",
                        x, y, savedStr, currentStr);
            }
          }
        }
      }
      
      if (diffCount == 0) {
        vlog.info("VERIFICATION PASSED: All %zu pixels match!", totalPixels);
      } else {
        vlog.error("VERIFICATION FAILED: %zu/%zu pixels differ (%.3f%%)",
                  diffCount, totalPixels, (100.0 * diffCount / totalPixels));
        if (diffCount > 10) {
          vlog.error("(Only first 10 differences logged)");
        }
      }
    }
    
    vlog.info("==================================================");
  }

  // Compute new settings based on updated bandwidth values
  if (autoSelect) {
    updateEncoding();
    updateQualityLevel();
    updatePixelFormat();
  }
}

// The rest of the callbacks are fairly self-explanatory...

void CConn::bell()
{
  fl_beep();
}

bool CConn::dataRect(const core::Rect& r, int encoding,
		     const rfb::ServerParams* serverOverride)
{
  bool ret;

  if (encoding != rfb::encodingCopyRect)
    lastServerEncoding = encoding;

  ret = CConnection::dataRect(r, encoding, serverOverride);

  if (ret)
    pixelCount += r.area();

  return ret;
}

void CConn::setCursor(int width, int height, const core::Point& hotspot,
                      const uint8_t* data)
{
  CConnection::setCursor(width, height, hotspot, data);

  desktop->setCursor();
}

void CConn::setCursorPos(const core::Point& pos)
{
  desktop->setCursorPos(pos);
}

void CConn::setLEDState(unsigned int state)
{
  CConnection::setLEDState(state);

  desktop->setLEDState(state);
}

void CConn::handleClipboardRequest()
{
  desktop->handleClipboardRequest();
}

void CConn::handleClipboardAnnounce(bool available)
{
  desktop->handleClipboardAnnounce(available);
}

void CConn::handleClipboardData(const char* data)
{
  desktop->handleClipboardData(data);
}


////////////////////// Internal methods //////////////////////

void CConn::resizeFramebuffer()
{
  desktop->resizeFramebuffer(server.width(), server.height());
}

void CConn::updateEncoding()
{
  int encNum;

  if (autoSelect)
    encNum = rfb::encodingTight;
  else
    encNum = rfb::encodingNum(::preferredEncoding.getValueStr().c_str());

  if (encNum != -1)
    setPreferredEncoding(encNum);
}

void CConn::updateCompressLevel()
{
  if (customCompressLevel)
    setCompressLevel(::compressLevel);
  else
    setCompressLevel(-1);
}

void CConn::updateQualityLevel()
{
  int newQualityLevel;

  if (noJpeg)
    newQualityLevel = -1;
  else if (!autoSelect)
    newQualityLevel = ::qualityLevel;
  else {
    // Above 16Mbps (i.e. LAN), we choose the second highest JPEG
    // quality, which should be perceptually lossless. If the bandwidth
    // is below that, we choose a more lossy JPEG quality.

    if (bpsEstimate > 16000000)
      newQualityLevel = 8;
    else
      newQualityLevel = 6;

    if (newQualityLevel != getQualityLevel()) {
      vlog.info(_("Throughput %d kbit/s - changing to quality %d"),
                (int)(bpsEstimate/1000), newQualityLevel);
    }
  }

  setQualityLevel(newQualityLevel);
}

void CConn::updatePixelFormat()
{
  bool useFullColour;
  rfb::PixelFormat pf;

  if (server.beforeVersion(3, 8)) {
    // Xvnc from TightVNC 1.2.9 sends out FramebufferUpdates with
    // cursors "asynchronously". If this happens in the middle of a
    // pixel format change, the server will encode the cursor with
    // the old format, but the client will try to decode it
    // according to the new format. This will lead to a
    // crash. Therefore, we do not allow automatic format change for
    // old servers.
    return;
  }

  useFullColour = fullColour;

  // If the bandwidth drops below 256 Kbps, we switch to palette mode.
  if (autoSelect) {
    useFullColour = (bpsEstimate > 256000);
    if (useFullColour != (server.pf() == fullColourPF)) {
      if (useFullColour)
        vlog.info(_("Throughput %d kbit/s - full color is now enabled"),
                  (int)(bpsEstimate/1000));
      else
        vlog.info(_("Throughput %d kbit/s - full color is now disabled"),
                  (int)(bpsEstimate/1000));
    }
  }

  if (useFullColour) {
    pf = fullColourPF;
  } else {
    if (lowColourLevel == 0)
      pf = verylowColourPF;
    else if (lowColourLevel == 1)
      pf = lowColourPF;
    else
      pf = mediumColourPF;
  }

  if (pf != server.pf()) {
    char oldStr[256], newStr[256];
    server.pf().print(oldStr, 256);
    pf.print(newStr, 256);
    vlog.info(_("PIXEL FORMAT CHANGE: old=[%s] new=[%s] bpsEstimate=%llu"),
              oldStr, newStr, (unsigned long long)bpsEstimate);
    vlog.debug("WARNING: Pixel format change may cause cache format mismatches!");
    setPF(pf);
  }
}

void CConn::handleOptions(void *data)
{
  CConn *self = (CConn*)data;

  self->updateEncoding();
  self->updateCompressLevel();
  self->updateQualityLevel();
  self->updatePixelFormat();
}

void CConn::handleUpdateTimeout(void *data)
{
  CConn *self = (CConn *)data;

  assert(self);

  self->desktop->updateWindow();

  Fl::repeat_timeout(1.0, handleUpdateTimeout, data);
}

void CConn::verifyFramebuffer()
{
  vlog.info("========== FRAMEBUFFER VERIFICATION REQUESTED ==========" );
  vlog.info("NOTE: Verification compares internal framebuffer state, not screen pixels");
  vlog.info("If corruption appears fixed after refresh but verification passes,");
  vlog.info("the bug may be in display rendering, not framebuffer content");
  
  if (!desktop) {
    vlog.error("Cannot verify: desktop not initialized");
    return;
  }
  
  if (verificationInProgress_) {
    vlog.error("Verification already in progress");
    return;
  }
  
  // Force display update to ensure screen matches framebuffer
  desktop->updateWindow();
  Fl::flush();
  vlog.info("Display flushed to ensure screen matches internal framebuffer");
  
  // Get current framebuffer
  rfb::ModifiablePixelBuffer* pb = getFramebuffer();
  if (!pb) {
    vlog.error("Cannot verify: framebuffer not available");
    return;
  }
  
  int width = server.width();
  int height = server.height();
  const rfb::PixelFormat& pf = pb->getPF();
  
  vlog.info("Saving current framebuffer state: %dx%d, %dbpp", width, height, pf.bpp);
  
  // Save current framebuffer state
  savedFBWidth_ = width;
  savedFBHeight_ = height;
  savedFBFormat_ = pf;
  
  int bytesPerPixel = pf.bpp / 8;
  size_t totalBytes = width * height * bytesPerPixel;
  savedFramebuffer_.resize(totalBytes);
  
  // Copy current framebuffer (row by row to handle stride)
  core::Rect rect(0, 0, width, height);
  int stride;
  const uint8_t* fbData = pb->getBuffer(rect, &stride);
  
  for (int y = 0; y < height; y++) {
    memcpy(&savedFramebuffer_[y * width * bytesPerPixel],
           fbData + (y * stride * bytesPerPixel),
           width * bytesPerPixel);
  }
  
  vlog.info("Framebuffer saved (%zu bytes)", totalBytes);
  vlog.info("Requesting full refresh from server (non-incremental)...");
  
  // Request full framebuffer update (incremental=false)
  // Server will send complete data using current encoding
  verificationInProgress_ = true;
  writer()->writeFramebufferUpdateRequest(rect, false);
  
  vlog.info("Verification update requested (will compare when received)");
  vlog.info("Note: Verification will use current encoding settings");
}

void CConn::dumpCorruptionDebugInfo()
{
#ifdef WIN32
  vlog.error("Corruption debug dump not supported on Windows");
  return;
#else
  vlog.info("=== CORRUPTION DEBUG DUMP TRIGGERED ===");
  
  // Use epoch timestamp so server and client use the same directory
  uint32_t epochTimestamp = (uint32_t)time(nullptr);
  
  // Send debug dump request to server FIRST (so server dumps at same time)
  vlog.info("Requesting server debug dump (timestamp=%u)", epochTimestamp);
  writer()->writeDebugDumpRequest(epochTimestamp);
  
  // Create output directory using epoch timestamp
  char dirName[64];
  snprintf(dirName, sizeof(dirName), "/tmp/corruption_debug_%u", epochTimestamp);
  std::string outputDir = dirName;
  
  if (mkdir(outputDir.c_str(), 0755) != 0) {
    vlog.error("Failed to create debug output directory: %s", outputDir.c_str());
    return;
  }
  
  vlog.info("Debug output directory: %s", outputDir.c_str());
  
  // 1. Dump cache state
  dumpCacheDebugState(outputDir.c_str());
  
  // 2. Save framebuffer as PPM image
  rfb::ModifiablePixelBuffer* pb = getFramebuffer();
  if (pb) {
    int width = server.width();
    int height = server.height();
    const rfb::PixelFormat& pf = pb->getPF();
    
    std::string ppmPath = outputDir + "/framebuffer.ppm";
    std::ofstream ppm(ppmPath, std::ios::binary);
    if (ppm.is_open()) {
      // PPM header
      ppm << "P6\n" << width << " " << height << "\n255\n";
      
      // Get framebuffer data
      core::Rect rect(0, 0, width, height);
      int stride;
      const uint8_t* fbData = pb->getBuffer(rect, &stride);
      int bytesPerPixel = pf.bpp / 8;
      
      // Convert to RGB and write using PixelFormat::rgbFromBuffer
      std::vector<uint8_t> rgbRow(width * 3);
      
      for (int y = 0; y < height; y++) {
        const uint8_t* srcRow = fbData + (y * stride * bytesPerPixel);
        // Use the pixel format's built-in conversion
        pf.rgbFromBuffer(rgbRow.data(), srcRow, width);
        ppm.write((const char*)rgbRow.data(), rgbRow.size());
      }
      ppm.close();
      vlog.info("Framebuffer saved to: %s", ppmPath.c_str());
    } else {
      vlog.error("Failed to create PPM file: %s", ppmPath.c_str());
    }
  } else {
    vlog.error("Framebuffer not available for dump");
  }
  
  // 3. Write session info
  std::string infoPath = outputDir + "/info.txt";
  std::ofstream info(infoPath);
  if (info.is_open()) {
    info << "=== VNC Viewer Corruption Debug Info ===\n";
    info << "Timestamp (epoch): " << epochTimestamp << "\n";
    info << "PID: " << getpid() << "\n";
    info << "\n=== Server Info ===\n";
    info << "Server: " << serverHost << ":" << serverPort << "\n";
    info << "Desktop name: " << server.name() << "\n";
    info << "Resolution: " << server.width() << "x" << server.height() << "\n";
    info << "\n=== Pixel Format ===\n";
    const rfb::PixelFormat& pf = server.pf();
    info << "Bits per pixel: " << pf.bpp << "\n";
    info << "Depth: " << pf.depth << "\n";
    info << "Big endian: " << (pf.isBigEndian() ? "yes" : "no") << "\n";
    info << "True colour: " << (pf.trueColour ? "yes" : "no") << "\n";
    char pfStr[256];
    pf.print(pfStr, sizeof(pfStr));
    info << "Format: " << pfStr << "\n";
    info << "\n=== Statistics ===\n";
    info << "Update count: " << updateCount << "\n";
    info << "Pixel count: " << pixelCount << "\n";
    info << "BPS estimate: " << bpsEstimate << "\n";
    info << "\n=== Cache Protocol ===\n";
    info << "ContentCache enabled: " << (::contentCache ? "yes" : "no") << "\n";
    info << "PersistentCache enabled: " << (::persistentCache ? "yes" : "no") << "\n";
    
    // Network stats
    if (sock) {
      struct timeval now_tv;
      gettimeofday(&now_tv, nullptr);
      unsigned long long elapsedUsec =
        (unsigned long long)(now_tv.tv_sec - sessionStartTime.tv_sec) * 1000000ULL +
        (unsigned long long)(now_tv.tv_usec - sessionStartTime.tv_usec);
      double seconds = (double)elapsedUsec / 1e6;
      uint64_t rxBytes = sock->inStream().pos();
      uint64_t txBytes = sock->outStream().bytesWritten();
      
      info << "\n=== Network Stats ===\n";
      info << "Session duration: " << seconds << " seconds\n";
      info << "Bytes received: " << rxBytes << "\n";
      info << "Bytes sent: " << txBytes << "\n";
      if (seconds > 0) {
        info << "Avg RX rate: " << (rxBytes / seconds / 1024.0) << " KB/s\n";
      }
    }
    
    info.close();
    vlog.info("Session info saved to: %s", infoPath.c_str());
  }
  
  vlog.info("=== CORRUPTION DEBUG DUMP COMPLETE ===");
  vlog.info("Output directory: %s", outputDir.c_str());
#endif
}
