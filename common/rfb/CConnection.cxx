/* Copyright (C) 2002-2005 RealVNC Ltd.  All Rights Reserved.
 * Copyright 2011-2019 Pierre Ossman for Cendio AB
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

#include <assert.h>
#include <stdio.h>
#include <string.h>

#include <algorithm>

#include <core/LogWriter.h>
#include <core/string.h>

#include <rfb/Exception.h>
#include <rfb/clipboardTypes.h>
#include <rfb/fenceTypes.h>
#include <rfb/screenTypes.h>
#include <rfb/CMsgReader.h>
#include <rfb/CMsgWriter.h>
#include <rfb/CSecurity.h>
#include <rfb/Cursor.h>
#include <rfb/Decoder.h>
#include <rfb/KeysymStr.h>
#include <rfb/PixelBuffer.h>
#include <rfb/Security.h>
#include <rfb/SecurityClient.h>
#include <rfb/CConnection.h>
#include <rfb/ContentHash.h>

#define XK_MISCELLANY
#define XK_XKB_KEYS
#include <rfb/keysymdef.h>

#include <rdr/InStream.h>
#include <rdr/OutStream.h>

using namespace rfb;

static core::LogWriter vlog("CConnection");

// Client-side ContentCache / update tracing helper
static bool isClientCCDebugEnabled()
{
  const char* env = getenv("TIGERVNC_CC_CLIENT_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

// Optional framebuffer hash debug for a fixed region (ContentCache debugging)
static bool isFBHashDebugEnabled()
{
  const char* env = getenv("TIGERVNC_FB_HASH_DEBUG");
  return env && env[0] != '\0' && env[0] != '0';
}

CConnection::CConnection()
  : csecurity(nullptr),
    supportsLocalCursor(false), supportsCursorPosition(false),
    supportsDesktopResize(false), supportsLEDState(false),
    supportsContentCache(true), supportsPersistentCache(true), supportsNativeFormatCache_(true),
    is(nullptr), os(nullptr), reader_(nullptr), writer_(nullptr),
    shared(false),
    state_(RFBSTATE_UNINITIALISED),
    pendingPFChange(false), preferredEncoding(encodingTight),
    compressLevel(2), qualityLevel(-1),
    formatChange(false), encodingChange(false),
    firstUpdate(true), pendingUpdate(false), continuousUpdates(false),
    forceNonincremental(true),
    framebuffer(nullptr), decoder(this),
    hasRemoteClipboard(false), hasLocalClipboard(false)
{
}

CConnection::~CConnection()
{
  close();
}

void CConnection::setServerName(const char* name_)
{
  if (name_ == nullptr)
    name_ = "";
  serverName = name_;
}

void CConnection::setStreams(rdr::InStream* is_, rdr::OutStream* os_)
{
  is = is_;
  os = os_;
}

void CConnection::setFramebuffer(ModifiablePixelBuffer* fb)
{
  decoder.flush();

  if (fb) {
    assert(fb->width() == server.width());
    assert(fb->height() == server.height());
  }

  if ((framebuffer != nullptr) && (fb != nullptr)) {
    core::Rect rect;

    const uint8_t* data;
    int stride;

    const uint8_t black[4] = { 0, 0, 0, 0 };

    vlog.debug("setFramebuffer: old %dx%d, new %dx%d",
               framebuffer->width(), framebuffer->height(),
               fb->width(), fb->height());

    // Copy still valid area

    rect = fb->getRect();
    rect = rect.intersect(framebuffer->getRect());
    vlog.debug("setFramebuffer: copying overlap rect [%d,%d-%d,%d]",
               rect.tl.x, rect.tl.y, rect.br.x, rect.br.y);
    data = framebuffer->getBuffer(framebuffer->getRect(), &stride);
    vlog.debug("setFramebuffer: old framebuffer stride=%d, data ptr=%p", stride, data);
    
    // Sample a few pixels from the old framebuffer before copy
    if (rect.width() >= 800 && rect.height() >= 860) {
      int bytesPerPixel = framebuffer->getPF().bpp / 8;
      const uint8_t* sample = data + (859 * stride + 797) * bytesPerPixel;
      vlog.debug("setFramebuffer: sample old pixel (797,859) = (%d,%d,%d,%d)",
                 sample[0], sample[1], sample[2], sample[3]);
    }
    
    fb->imageRect(rect, data, stride);
    
    // Sample the same pixel after copy in new framebuffer
    if (rect.width() >= 800 && rect.height() >= 860) {
      int newStride;
      const uint8_t* newData = fb->getBuffer(core::Rect(797, 859, 798, 860), &newStride);
      vlog.debug("setFramebuffer: sample new pixel (797,859) = (%d,%d,%d,%d)",
                 newData[0], newData[1], newData[2], newData[3]);
    }

    // Black out any new areas

    if (fb->width() > framebuffer->width()) {
      rect.setXYWH(framebuffer->width(), 0,
                   fb->width() - framebuffer->width(),
                   fb->height());
      vlog.debug("setFramebuffer: blacking out right strip [%d,%d-%d,%d]",
                 rect.tl.x, rect.tl.y, rect.br.x, rect.br.y);
      fb->fillRect(rect, black);
    }

    if (fb->height() > framebuffer->height()) {
      rect.setXYWH(0, framebuffer->height(),
                   framebuffer->width(),
                   fb->height() - framebuffer->height());
      vlog.debug("setFramebuffer: blacking out bottom strip [%d,%d-%d,%d]",
                 rect.tl.x, rect.tl.y, rect.br.x, rect.br.y);
      fb->fillRect(rect, black);
    }
  }

  delete framebuffer;
  framebuffer = fb;
}

void CConnection::logDecodeStats()
{
  decoder.logStats();
}

void CConnection::dumpCacheDebugState(const char* outputDir)
{
  decoder.dumpCacheDebugState(outputDir);
}

void CConnection::initialiseProtocol()
{
  state_ = RFBSTATE_PROTOCOL_VERSION;
}

bool CConnection::processMsg()
{
  switch (state_) {

  case RFBSTATE_PROTOCOL_VERSION: return processVersionMsg();        break;
  case RFBSTATE_SECURITY_TYPES:   return processSecurityTypesMsg();  break;
  case RFBSTATE_SECURITY:         return processSecurityMsg();       break;
  case RFBSTATE_SECURITY_RESULT:  return processSecurityResultMsg(); break;
  case RFBSTATE_SECURITY_REASON:  return processSecurityReasonMsg(); break;
  case RFBSTATE_INITIALISATION:   return processInitMsg();           break;
  case RFBSTATE_NORMAL:           return reader_->readMsg();         break;
  case RFBSTATE_CLOSING:
    throw std::logic_error("CConnection::processMsg: Called while closing");
  case RFBSTATE_UNINITIALISED:
    throw std::logic_error("CConnection::processMsg: Not initialised yet?");
  default:
    throw std::logic_error("CConnection::processMsg: Invalid state");
  }
}

bool CConnection::processVersionMsg()
{
  char verStr[27]; // FIXME: gcc has some bug in format-overflow
  int majorVersion;
  int minorVersion;

  vlog.debug("Reading protocol version");

  if (!is->hasData(12))
    return false;

  is->readBytes((uint8_t*)verStr, 12);
  verStr[12] = '\0';

  if (sscanf(verStr, "RFB %03d.%03d\n",
             &majorVersion, &minorVersion) != 2) {
    state_ = RFBSTATE_INVALID;
    throw protocol_error("Reading version failed, not an RFB server?");
  }

  server.setVersion(majorVersion, minorVersion);

  vlog.info("Server supports RFB protocol version %d.%d",
            server.majorVersion, server.minorVersion);

  // The only official RFB protocol versions are currently 3.3, 3.7 and 3.8
  if (server.beforeVersion(3,3)) {
    vlog.error("Server gave unsupported RFB protocol version %d.%d",
               server.majorVersion, server.minorVersion);
    state_ = RFBSTATE_INVALID;
    throw protocol_error(
      core::format("Server gave unsupported RFB protocol version %d.%d",
                   server.majorVersion, server.minorVersion));
  } else if (server.beforeVersion(3,7)) {
    server.setVersion(3,3);
  } else if (server.afterVersion(3,8)) {
    server.setVersion(3,8);
  }

  sprintf(verStr, "RFB %03d.%03d\n",
          server.majorVersion, server.minorVersion);
  os->writeBytes((const uint8_t*)verStr, 12);
  os->flush();

  state_ = RFBSTATE_SECURITY_TYPES;

  vlog.info("Using RFB protocol version %d.%d",
            server.majorVersion, server.minorVersion);

  return true;
}


bool CConnection::processSecurityTypesMsg()
{
  vlog.debug("Processing security types message");

  int secType = secTypeInvalid;

  std::list<uint8_t> secTypes;
  secTypes = security.GetEnabledSecTypes();

  if (server.isVersion(3,3)) {

    // legacy 3.3 server may only offer "vnc authentication" or "none"

    if (!is->hasData(4))
      return false;

    secType = is->readU32();
    if (secType == secTypeInvalid) {
      state_ = RFBSTATE_SECURITY_REASON;
      return true;
    } else if (secType == secTypeNone || secType == secTypeVncAuth) {
      if (std::find(secTypes.begin(), secTypes.end(),
                    secType) == secTypes.end())
        secType = secTypeInvalid;
    } else {
      vlog.error("Unknown 3.3 security type %d", secType);
      throw protocol_error("Unknown 3.3 security type");
    }

  } else {

    // >=3.7 server will offer us a list

    if (!is->hasData(1))
      return false;

    is->setRestorePoint();

    int nServerSecTypes = is->readU8();

    if (!is->hasDataOrRestore(nServerSecTypes))
      return false;
    is->clearRestorePoint();

    if (nServerSecTypes == 0) {
      state_ = RFBSTATE_SECURITY_REASON;
      return true;
    }

    for (int i = 0; i < nServerSecTypes; i++) {
      uint8_t serverSecType = is->readU8();
      vlog.debug("Server offers security type %s(%d)",
                 secTypeName(serverSecType), serverSecType);

      /*
       * Use the first type sent by server which matches client's type.
       * It means server's order specifies priority.
       */
      if (secType == secTypeInvalid) {
        if (std::find(secTypes.begin(), secTypes.end(),
                      serverSecType) != secTypes.end())
          secType = serverSecType;
      }
    }

    // Inform the server of our decision
    if (secType != secTypeInvalid) {
      os->writeU8(secType);
      os->flush();
      vlog.info("Choosing security type %s(%d)",secTypeName(secType),secType);
    }
  }

  if (secType == secTypeInvalid) {
    state_ = RFBSTATE_INVALID;
    vlog.error("No matching security types");
    throw protocol_error("No matching security types");
  }

  state_ = RFBSTATE_SECURITY;
  csecurity = security.GetCSecurity(this, secType);

  return true;
}

bool CConnection::processSecurityMsg()
{
  vlog.debug("Processing security message");
  if (!csecurity->processMsg())
    return false;

  state_ = RFBSTATE_SECURITY_RESULT;

  return true;
}

bool CConnection::processSecurityResultMsg()
{
  vlog.debug("Processing security result message");
  int result;

  if (server.beforeVersion(3,8) && csecurity->getType() == secTypeNone) {
    result = secResultOK;
  } else {
    if (!is->hasData(4))
      return false;
    result = is->readU32();
  }

  switch (result) {
  case secResultOK:
    securityCompleted();
    return true;
  case secResultFailed:
    vlog.debug("Auth failed");
    break;
  case secResultTooMany:
    vlog.debug("Auth failed: Too many tries");
    break;
  default:
    throw protocol_error("Unknown security result from server");
  }

  if (server.beforeVersion(3,8)) {
    state_ = RFBSTATE_INVALID;
    throw auth_error("Authentication failed");
  }

  state_ = RFBSTATE_SECURITY_REASON;
  return true;
}

bool CConnection::processSecurityReasonMsg()
{
  vlog.debug("Processing security reason message");

  if (!is->hasData(4))
    return false;

  is->setRestorePoint();

  uint32_t len = is->readU32();
  if (!is->hasDataOrRestore(len))
    return false;
  is->clearRestorePoint();

  std::vector<char> reason(len + 1);
  is->readBytes((uint8_t*)reason.data(), len);
  reason[len] = '\0';

  state_ = RFBSTATE_INVALID;
  throw auth_error(reason.data());
}

bool CConnection::processInitMsg()
{
  vlog.debug("Reading server initialisation");
  return reader_->readServerInit();
}

void CConnection::securityCompleted()
{
  state_ = RFBSTATE_INITIALISATION;
  reader_ = new CMsgReader(this, is);
  writer_ = new CMsgWriter(&server, os);
  vlog.debug("Authentication success!");
  writer_->writeClientInit(shared);

  // After authentication is complete and the CMsgWriter exists, advertise any
  // hashes loaded from the client-side PersistentCache so the server can
  // immediately exploit a warm cache.
  decoder.advertisePersistentCacheHashes();
}

void CConnection::close()
{
  state_ = RFBSTATE_CLOSING;

  /*
   * We're already shutting down, so just log any pending decoder
   * problems
   */
  try {
    decoder.flush();
  } catch (std::exception& e) {
    vlog.error("%s", e.what());
  }

  setFramebuffer(nullptr);
  delete csecurity;
  csecurity = nullptr;
  delete reader_;
  reader_ = nullptr;
  delete writer_;
  writer_ = nullptr;
}

void CConnection::setDesktopSize(int w, int h)
{
  decoder.flush();

  server.setDimensions(w, h);

  if (continuousUpdates)
    writer()->writeEnableContinuousUpdates(true, 0, 0,
                                           server.width(),
                                           server.height());

  resizeFramebuffer();
  assert(framebuffer != nullptr);
  assert(framebuffer->width() == server.width());
  assert(framebuffer->height() == server.height());
}

void CConnection::setExtendedDesktopSize(unsigned reason,
                                         unsigned result,
                                         int w, int h,
                                         const ScreenSet& layout)
{
  decoder.flush();

  server.supportsSetDesktopSize = true;

  if ((reason != reasonClient) || (result == resultSuccess))
    server.setDimensions(w, h, layout);

  if ((reason == reasonClient) && (result != resultSuccess)) {
    vlog.error("SetDesktopSize failed: %d", result);
    return;
  }

  if (continuousUpdates)
    writer()->writeEnableContinuousUpdates(true, 0, 0,
                                           server.width(),
                                           server.height());

  resizeFramebuffer();
  assert(framebuffer != nullptr);
  assert(framebuffer->width() == server.width());
  assert(framebuffer->height() == server.height());
}

void CConnection::setCursor(int width, int height,
                            const core::Point& hotspot,
                            const uint8_t* data)
{
  Cursor cursor(width, height, hotspot, data);
  server.setCursor(cursor);
}

void CConnection::setCursorPos(const core::Point& /*pos*/)
{
}

void CConnection::setName(const char* name)
{
  server.setName(name);
}

void CConnection::fence(uint32_t flags, unsigned len, const uint8_t data[])
{
  server.supportsFence = true;

  if (flags & fenceFlagRequest) {
    // FIXME: We handle everything synchronously, and we assume anything
    //        using us also does so, which means we automatically handle
    //        these flags
    flags = flags & (fenceFlagBlockBefore | fenceFlagBlockAfter);

    writer()->writeFence(flags, len, data);
    return;
  }
}

void CConnection::endOfContinuousUpdates()
{
  server.supportsContinuousUpdates = true;

  // We've gotten the marker for a format change, so make the pending
  // one active
  if (pendingPFChange) {
    server.setPF(pendingPF);
    pendingPFChange = false;

    // We might have another change pending
    if (formatChange)
      requestNewUpdate();
  }
}

void CConnection::supportsQEMUKeyEvent()
{
  server.supportsQEMUKeyEvent = true;
}

void CConnection::supportsExtendedMouseButtons()
{
  server.supportsExtendedMouseButtons = true;
}

void CConnection::serverInit(int width, int height,
                             const PixelFormat& pf,
                             const char* name)
{
  server.setDimensions(width, height);
  server.setPF(pf);
  server.setName(name);

  state_ = RFBSTATE_NORMAL;
  vlog.debug("Initialisation done");

  initDone();
  assert(framebuffer != nullptr);
  assert(framebuffer->width() == server.width());
  assert(framebuffer->height() == server.height());

  // We want to make sure we call SetEncodings at least once
  encodingChange = true;

  requestNewUpdate();

  // This initial update request is a bit of a corner case, so we need
  // to help out setting the correct format here.
  if (pendingPFChange) {
    server.setPF(pendingPF);
    pendingPFChange = false;
  }
}

bool CConnection::readAndDecodeRect(const core::Rect& r, int encoding,
                                    ModifiablePixelBuffer* pb,
                                    const ServerParams* serverOverride)
{
  if (!decoder.decodeRect(r, encoding, pb, serverOverride))
    return false;
  decoder.flush();
  return true;
}

void CConnection::framebufferUpdateStart()
{
  assert(framebuffer != nullptr);

  // Note: This might not be true if continuous updates are supported
  pendingUpdate = false;

  requestNewUpdate();
}

void CConnection::framebufferUpdateEnd()
{
  decoder.flush();

  // After all rects in this update have been applied, optionally hash
  // a fixed framebuffer region to compare ContentCache vs. non-cache runs.
  if (isFBHashDebugEnabled() && framebuffer != nullptr) {
    // Problem region observed in black-box tests: [100,100-586,443]
    core::Rect problemRegion(100, 100, 586, 443);
    core::Rect fbRect = framebuffer->getRect();
    core::Rect hashRect = problemRegion.intersect(fbRect);
    if (!hashRect.is_empty()) {
      std::vector<uint8_t> hash = ContentHash::computeRect(framebuffer, hashRect);
      // Format first 8 bytes as a 64-bit value for compact logging
      uint64_t hash64 = 0;
      size_t n = std::min<size_t>(8, hash.size());
      for (size_t i = 0; i < n; ++i) {
        hash64 = (hash64 << 8) | hash[i];
      }
      vlog.info("FBDBG HASH region=[%d,%d-%d,%d] hash64=%016llx",
                hashRect.tl.x, hashRect.tl.y,
                hashRect.br.x, hashRect.br.y,
                (unsigned long long)hash64);
    }
  }

  // A format change has been scheduled and we are now past the update
  // with the old format. Time to active the new one.
  if (pendingPFChange && !continuousUpdates) {
    server.setPF(pendingPF);
    pendingPFChange = false;
  }

  if (firstUpdate) {
    if (server.supportsContinuousUpdates) {
      vlog.info("Enabling continuous updates");
      continuousUpdates = true;
      writer()->writeEnableContinuousUpdates(true, 0, 0,
                                             server.width(),
                                             server.height());
    }

    firstUpdate = false;
  }
}

bool CConnection::dataRect(const core::Rect& r, int encoding,
                           const ServerParams* serverOverride)
{
  // Optional client-side tracing of update ordering in the problematic region
  if (isClientCCDebugEnabled()) {
    core::Rect problemRegion(100, 100, 586, 443); // [100,100-586,443]
    if (!problemRegion.intersect(r).is_empty()) {
      vlog.info("CCDBG CLIENT dataRect: rect=[%d,%d-%d,%d] enc=%d", 
                r.tl.x, r.tl.y, r.br.x, r.br.y, encoding);
    }
  }
  return decoder.decodeRect(r, encoding, framebuffer, serverOverride);
}

void CConnection::setColourMapEntries(int /*firstColour*/,
                                      int /*nColours*/,
                                      uint16_t* /*rgbs*/)
{
  vlog.error("Invalid SetColourMapEntries from server!");
}

void CConnection::serverCutText(const char* str)
{
  hasLocalClipboard = false;

  serverClipboard = str;
  hasRemoteClipboard = true;

  handleClipboardAnnounce(true);
}

void CConnection::setLEDState(unsigned int state)
{
  server.setLEDState(state);
}

void CConnection::handleClipboardCaps(uint32_t flags,
                                      const uint32_t* lengths)
{
  int i;
  uint32_t sizes[] = { 0 };

  vlog.debug("Got server clipboard capabilities:");
  for (i = 0;i < 16;i++) {
    if (flags & (1 << i)) {
      const char *type;

      switch (1 << i) {
        case clipboardUTF8:
          type = "Plain text";
          break;
        case clipboardRTF:
          type = "Rich text";
          break;
        case clipboardHTML:
          type = "HTML";
          break;
        case clipboardDIB:
          type = "Images";
          break;
        case clipboardFiles:
          type = "Files";
          break;
        default:
          vlog.debug("    Unknown format 0x%x", 1 << i);
          continue;
      }

      if (lengths[i] == 0)
        vlog.debug("    %s (only notify)", type);
      else {
        vlog.debug("    %s (automatically send up to %s)",
                   type, core::iecPrefix(lengths[i], "B").c_str());
      }
    }
  }

  server.setClipboardCaps(flags, lengths);

  writer()->writeClipboardCaps(rfb::clipboardUTF8 |
                               rfb::clipboardRequest |
                               rfb::clipboardPeek |
                               rfb::clipboardNotify |
                               rfb::clipboardProvide,
                               sizes);
}

void CConnection::handleClipboardRequest(uint32_t flags)
{
  if (!(flags & rfb::clipboardUTF8)) {
    vlog.debug("Ignoring clipboard request for unsupported formats 0x%x", flags);
    return;
  }
  if (!hasLocalClipboard) {
    vlog.debug("Ignoring unexpected clipboard request");
    return;
  }
  handleClipboardRequest();
}

void CConnection::handleClipboardPeek()
{
  if (server.clipboardFlags() & rfb::clipboardNotify)
    writer()->writeClipboardNotify(hasLocalClipboard ? rfb::clipboardUTF8 : 0);
}

void CConnection::handleClipboardNotify(uint32_t flags)
{
  hasRemoteClipboard = false;

  if (flags & rfb::clipboardUTF8) {
    hasLocalClipboard = false;
    handleClipboardAnnounce(true);
  } else {
    handleClipboardAnnounce(false);
  }
}

void CConnection::handleClipboardProvide(uint32_t flags,
                                         const size_t* lengths,
                                         const uint8_t* const* data)
{
  if (!(flags & rfb::clipboardUTF8)) {
    vlog.debug("Ignoring clipboard provide with unsupported formats 0x%x", flags);
    return;
  }

  // FIXME: This conversion magic should be in CMsgReader
  if (!core::isValidUTF8((const char*)data[0], lengths[0])) {
    vlog.error("Invalid UTF-8 sequence in clipboard - ignoring");
    return;
  }
  serverClipboard = core::convertLF((const char*)data[0], lengths[0]);
  hasRemoteClipboard = true;

  // FIXME: Should probably verify that this data was actually requested
  handleClipboardData(serverClipboard.c_str());
}

void CConnection::initDone()
{
}

void CConnection::resizeFramebuffer()
{
  assert(false);
}

void CConnection::handleClipboardRequest()
{
}

void CConnection::handleClipboardAnnounce(bool /*available*/)
{
}

void CConnection::handleClipboardData(const char* /*data*/)
{
}

void CConnection::requestClipboard()
{
  if (hasRemoteClipboard) {
    handleClipboardData(serverClipboard.c_str());
    return;
  }

  if (server.clipboardFlags() & rfb::clipboardRequest)
    writer()->writeClipboardRequest(rfb::clipboardUTF8);
}

void CConnection::announceClipboard(bool available)
{
  hasLocalClipboard = available;
  unsolicitedClipboardAttempt = false;

  // Attempt an unsolicited transfer?
  if (available &&
      (server.clipboardSize(rfb::clipboardUTF8) > 0) &&
      (server.clipboardFlags() & rfb::clipboardProvide)) {
    vlog.debug("Attempting unsolicited clipboard transfer...");
    unsolicitedClipboardAttempt = true;
    handleClipboardRequest();
    return;
  }

  if (server.clipboardFlags() & rfb::clipboardNotify) {
    writer()->writeClipboardNotify(available ? rfb::clipboardUTF8 : 0);
    return;
  }

  if (available)
    handleClipboardRequest();
}

void CConnection::sendClipboardData(const char* data)
{
  if (server.clipboardFlags() & rfb::clipboardProvide) {
    // FIXME: This conversion magic should be in CMsgWriter
    std::string filtered(core::convertCRLF(data));
    size_t sizes[1] = { filtered.size() + 1 };
    const uint8_t* datas[1] = { (const uint8_t*)filtered.c_str() };

    if (unsolicitedClipboardAttempt) {
      unsolicitedClipboardAttempt = false;
      if (sizes[0] > server.clipboardSize(rfb::clipboardUTF8)) {
        vlog.debug("Clipboard was too large for unsolicited clipboard transfer");
        if (server.clipboardFlags() & rfb::clipboardNotify)
          writer()->writeClipboardNotify(rfb::clipboardUTF8);
        return;
      }
    }

    writer()->writeClipboardProvide(rfb::clipboardUTF8, sizes, datas);
  } else {
    writer()->writeClientCutText(data);
  }
}

void CConnection::sendKeyPress(int systemKeyCode,
                               uint32_t keyCode, uint32_t keySym)
{
  // For the first few years, there wasn't a good consensus on what the
  // Windows keys should be mapped to for X11. So we need to help out a
  // bit and map all variants to the same key...
  switch (keySym) {
  case XK_Hyper_L:
    keySym = XK_Super_L;
    break;
  case XK_Hyper_R:
    keySym = XK_Super_R;
    break;
  // There has been several variants for Shift-Tab over the years.
  // RFB states that we should always send a normal tab.
  case XK_ISO_Left_Tab:
    keySym = XK_Tab;
    break;
  }

#ifdef __APPLE__
  // Alt on OS X behaves more like AltGr on other systems, and to get
  // sane behaviour we should translate things in that manner for the
  // remote VNC server. However that means we lose the ability to use
  // Alt as a shortcut modifier. Do what RealVNC does and hijack the
  // left command key as an Alt replacement.
  switch (keySym) {
  case XK_Super_L:
    keySym = XK_Alt_L;
    break;
  case XK_Super_R:
    keySym = XK_Super_L;
    break;
  case XK_Alt_L:
    keySym = XK_Mode_switch;
    break;
  case XK_Alt_R:
    keySym = XK_ISO_Level3_Shift;
    break;
  }
#endif

  // Because of the way keyboards work, we cannot expect to have the same
  // symbol on release as when pressed. This breaks the VNC protocol however,
  // so we need to keep track of what keysym a key _code_ generated on press
  // and send the same on release.
  downKeys[systemKeyCode].keyCode = keyCode;
  downKeys[systemKeyCode].keySym = keySym;

  vlog.debug("Key pressed: %d => 0x%02x / XK_%s (0x%04x)",
             systemKeyCode, keyCode, KeySymName(keySym), keySym);

  writer()->writeKeyEvent(keySym, keyCode, true);
}

void CConnection::sendKeyRelease(int systemKeyCode)
{
  DownMap::iterator iter;

  iter = downKeys.find(systemKeyCode);
  if (iter == downKeys.end()) {
    // These occur somewhat frequently so let's not spam them unless
    // logging is turned up.
    vlog.debug("Unexpected release of key code %d", systemKeyCode);
    return;
  }

  vlog.debug("Key released: %d => 0x%02x / XK_%s (0x%04x)",
             systemKeyCode, iter->second.keyCode,
             KeySymName(iter->second.keySym), iter->second.keySym);

  writer()->writeKeyEvent(iter->second.keySym,
                          iter->second.keyCode, false);

  downKeys.erase(iter);
}

void CConnection::releaseAllKeys()
{
  while (!downKeys.empty())
    sendKeyRelease(downKeys.begin()->first);
}

void CConnection::refreshFramebuffer()
{
  vlog.info("refreshFramebuffer: forceNonincremental=true, continuousUpdates=%d, pendingUpdate=%d",
            continuousUpdates, pendingUpdate);
  forceNonincremental = true;

  // Always request an update immediately. The previous logic only called
  // requestNewUpdate() for continuous updates, but this left the refresh
  // pending until the next update cycle, which might never happen if the
  // screen is static. For non-continuous mode, setting pendingUpdate=false
  // first ensures we can send a new request.
  if (!continuousUpdates) {
    vlog.info("refreshFramebuffer: clearing pendingUpdate for non-continuous mode");
    pendingUpdate = false;
  }
  vlog.info("refreshFramebuffer: calling requestNewUpdate()");
  requestNewUpdate();
  vlog.info("refreshFramebuffer: done");
}

void CConnection::setPreferredEncoding(int encoding)
{
  if (preferredEncoding == encoding)
    return;

  preferredEncoding = encoding;
  encodingChange = true;
}

int CConnection::getPreferredEncoding()
{
  return preferredEncoding;
}

void CConnection::setCompressLevel(int level)
{
  if (compressLevel == level)
    return;

  compressLevel = level;
  encodingChange = true;
}

int CConnection::getCompressLevel()
{
  return compressLevel;
}

void CConnection::setQualityLevel(int level)
{
  if (qualityLevel == level)
    return;

  qualityLevel = level;
  encodingChange = true;
}

int CConnection::getQualityLevel()
{
  return qualityLevel;
}

void CConnection::setPF(const PixelFormat& pf)
{
  if (server.pf() == pf && !formatChange)
    return;

  nextPF = pf;
  formatChange = true;
}

bool CConnection::isSecure() const
{
  // SecurityClient does not expose a runtime "secure" state in this build;
  // keep the API but conservatively report false.
  return false;
}

bool CConnection::isContentCacheNegotiated() const
{
  return negotiatedCacheProtocol == CacheProtocolContent;
}

bool CConnection::isPersistentCacheNegotiated() const
{
  return negotiatedCacheProtocol == CacheProtocolPersistent;
}

void CConnection::disablePersistentCacheForSession()
{
  // If PersistentCache was not enabled for this connection, nothing to do.
  if (!supportsPersistentCache && negotiatedCacheProtocol != CacheProtocolPersistent)
    return;

  // Stop advertising PersistentCache in future SetEncodings so the server
  // will stop sending PersistentCachedRect references. This prevents further
  // visual corruption once the client has detected hash mismatches.
  if (supportsPersistentCache) {
    supportsPersistentCache = false;
    supportsNativeFormatCache_ = false;
    encodingChange = true;
  }

  if (negotiatedCacheProtocol == CacheProtocolPersistent) {
    negotiatedCacheProtocol = CacheProtocolNone;
    negotiatedCacheLogged = false;
    vlog.info("Cache protocol: disabling PersistentCache for this session due to client-side hash mismatches; will re-issue SetEncodings without -321");
  }
}

// requestNewUpdate() requests an update from the server, having set the
// format and encoding appropriately.
void CConnection::requestNewUpdate()
{
  vlog.debug("requestNewUpdate: forceNonincremental=%d, continuousUpdates=%d, pendingUpdate=%d",
             forceNonincremental, continuousUpdates, pendingUpdate);
  if (formatChange && !pendingPFChange) {
    /* Catch incorrect requestNewUpdate calls */
    assert(!pendingUpdate || continuousUpdates);

    // We have to make sure we switch the internal format at a safe
    // time. For continuous updates we temporarily disable updates and
    // look for a EndOfContinuousUpdates message to see when to switch.
    // For classical updates we just got a new update right before this
    // function was called, so we need to make sure we finish that
    // update before we can switch.

    pendingPFChange = true;
    pendingPF = nextPF;

    if (continuousUpdates)
      writer()->writeEnableContinuousUpdates(false, 0, 0, 0, 0);

    writer()->writeSetPixelFormat(pendingPF);

    if (continuousUpdates)
      writer()->writeEnableContinuousUpdates(true, 0, 0,
                                             server.width(),
                                             server.height());

    formatChange = false;
  }

  if (encodingChange) {
    updateEncodings();
    encodingChange = false;
  }

  if (forceNonincremental || !continuousUpdates) {
    pendingUpdate = true;
    writer()->writeFramebufferUpdateRequest({0, 0,
                                             server.width(),
                                             server.height()},
                                            !forceNonincremental);
  }

  forceNonincremental = false;
}

// Ask for encodings based on which decoders are supported.  Assumes higher
// encoding numbers are more desirable.

void CConnection::updateEncodings()
{
  std::list<uint32_t> encodings;

  if (supportsLocalCursor) {
    encodings.push_back(pseudoEncodingCursorWithAlpha);
    encodings.push_back(pseudoEncodingVMwareCursor);
    encodings.push_back(pseudoEncodingCursor);
    encodings.push_back(pseudoEncodingXCursor);
  }
  if (supportsCursorPosition) {
    encodings.push_back(pseudoEncodingVMwareCursorPosition);
  }
  if (supportsDesktopResize) {
    encodings.push_back(pseudoEncodingDesktopSize);
    encodings.push_back(pseudoEncodingExtendedDesktopSize);
  }
  if (supportsLEDState) {
    encodings.push_back(pseudoEncodingLEDState);
    encodings.push_back(pseudoEncodingVMwareLEDState);
  }

  encodings.push_back(pseudoEncodingDesktopName);
  encodings.push_back(pseudoEncodingLastRect);
  encodings.push_back(pseudoEncodingExtendedClipboard);
  encodings.push_back(pseudoEncodingContinuousUpdates);
  encodings.push_back(pseudoEncodingFence);
  encodings.push_back(pseudoEncodingQEMUKeyEvent);
  encodings.push_back(pseudoEncodingExtendedMouseButtons);
  
  // Cache protocol extension: we now advertise only the PersistentCache
  // pseudo-encoding (-321). The "ContentCache" command-line/configuration
  // option is treated as an alias that enables the same protocol but with
  // an ephemeral (memory-only) policy at the viewer.
  if (supportsPersistentCache) {
    // CRITICAL: Load disk cache BEFORE advertising capability so we can
    // immediately send HashList with our known hashes. If we wait until
    // receiving the first PersistentCachedRectInit, that rect is already
    // a guaranteed miss (server didn't know we had the hash).
    decoder.triggerPersistentCacheLoad();
    encodings.push_back(pseudoEncodingPersistentCache);
    if (supportsNativeFormatCache_) {
      encodings.push_back(pseudoEncodingNativeFormatCache);
      vlog.info("Cache protocol: advertising NativeFormatCache (-327)");
    }
    vlog.info("Cache protocol: advertising PersistentCache (-321)");
  }

  if (Decoder::supported(preferredEncoding)) {
    encodings.push_back(preferredEncoding);
  }

  encodings.push_back(encodingCopyRect);

  for (int i = encodingMax; i >= 0; i--) {
    if ((i != preferredEncoding) && Decoder::supported(i))
      encodings.push_back(i);
  }

  if (compressLevel >= 0 && compressLevel <= 9)
      encodings.push_back(pseudoEncodingCompressLevel0 + compressLevel);
  if (qualityLevel >= 0 && qualityLevel <= 9)
      encodings.push_back(pseudoEncodingQualityLevel0 + qualityLevel);

  writer()->writeSetEncodings(encodings);
}

void CConnection::handleCachedRect(const core::Rect& r, const CacheKey& key)
{
  // On first use, record and log negotiated cache protocol
  if (negotiatedCacheProtocol == CacheProtocolNone) {
    negotiatedCacheProtocol = CacheProtocolContent;
    if (!negotiatedCacheLogged) {
      vlog.info("Cache protocol: negotiated ContentCache (-320)");
      negotiatedCacheLogged = true;
    }
  }

  // Forward to decoder manager to handle cache lookup and blit using the
  // unified cache engine (ContentCache now aliases PersistentCache policy).
  decoder.handleCachedRect(r, key, framebuffer);
}

void CConnection::storeCachedRect(const core::Rect& r, const CacheKey& key)
{
  // Forward to decoder manager to store decoded content with cache ID
  decoder.storeCachedRect(r, key, framebuffer);
}

void CConnection::handlePersistentCachedRect(const core::Rect& r, const CacheKey& key)
{
  // On first use, record and log negotiated cache protocol
  if (negotiatedCacheProtocol == CacheProtocolNone) {
    negotiatedCacheProtocol = CacheProtocolPersistent;
    if (!negotiatedCacheLogged) {
      vlog.info("Cache protocol: negotiated PersistentCache (-321)");
      negotiatedCacheLogged = true;
    }
    // Trigger deferred disk load now that we know server supports PersistentCache.
    // This must happen BEFORE processing the first rect so cached entries are available.
    decoder.triggerPersistentCacheLoad();
  }

  // Forward to decoder manager to handle cache lookup and blit
  decoder.handlePersistentCachedRect(r, key, framebuffer);
}

void CConnection::storePersistentCachedRect(const core::Rect& r, const CacheKey& key, int encoding)
{
  // On first use, record and log negotiated cache protocol and trigger disk load.
  // PersistentCachedRectInit may be the first message if server is sending new content,
  // so we must trigger disk load here too (not just in handlePersistentCachedRect).
  if (negotiatedCacheProtocol == CacheProtocolNone) {
    negotiatedCacheProtocol = CacheProtocolPersistent;
    if (!negotiatedCacheLogged) {
      vlog.info("Cache protocol: negotiated PersistentCache (-321)");
      negotiatedCacheLogged = true;
    }
    // Trigger deferred disk load BEFORE storing, so subsequent lookups benefit
    // from existing disk cache entries.
    decoder.triggerPersistentCacheLoad();
  }
  
  // Forward to decoder manager to store decoded content with ID and
  // inner encoding so it can distinguish lossless vs lossy payloads for
  // persistence policy.
  decoder.storePersistentCachedRect(r, key, encoding, framebuffer);
}

void CConnection::seedCachedRect(const core::Rect& r, const CacheKey& key)
{
  // Server tells us to take existing framebuffer pixels at rect R and
  // associate them with cache ID. This is used for whole-rectangle caching
  // where subrect data was already sent via normal encoding.
  //
  // We read pixels from our current framebuffer and store them in the cache.
  // Logged to debug file via DecodeManager
  
  // Forward to decoder manager to seed cache using existing framebuffer pixels.
  // Use encodingRaw as the "encoding" since we're reading raw pixels from framebuffer.
  decoder.seedCachedRect(r, key, framebuffer);
}
