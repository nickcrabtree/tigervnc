/* Copyright (C) 2002-2005 RealVNC Ltd.  All Rights Reserved.
 * Copyright 2009-2019 Pierre Ossman for Cendio AB
 * Copyright (C) 2011 D. R. Commander.  All Rights Reserved.
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
//
// CMsgHandler - class to handle incoming messages on the client side.
//

#ifndef __RFB_CMSGHANDLER_H__
#define __RFB_CMSGHANDLER_H__

#include <stdint.h>
#include <vector>

#include <rfb/ServerParams.h>

namespace core {
  struct Point;
  struct Rect;
}

namespace rfb {

  class ModifiablePixelBuffer;
  struct ScreenSet;

  class CMsgHandler {
  public:
    // The following methods are called as corresponding messages are
    // read. A derived class must override these methods.

    virtual void setDesktopSize(int w, int h) = 0;
    virtual void setExtendedDesktopSize(unsigned reason, unsigned result,
                                        int w, int h,
                                        const ScreenSet& layout) = 0;
    virtual void setCursor(int width, int height, const
                           core::Point& hotspot,
                           const uint8_t* data) = 0;
    virtual void setCursorPos(const core::Point& pos) = 0;
    virtual void setName(const char* name) = 0;
    virtual void fence(uint32_t flags, unsigned len,
                       const uint8_t data[]) = 0;
    virtual void endOfContinuousUpdates() = 0;
    virtual void supportsQEMUKeyEvent() = 0;
    virtual void supportsExtendedMouseButtons() = 0;
    virtual void serverInit(int width, int height,
                            const PixelFormat& pf,
                            const char* name) = 0;

    virtual bool readAndDecodeRect(const core::Rect& r, int encoding,
                                   ModifiablePixelBuffer* pb,
                                   const ServerParams* serverOverride = nullptr) = 0;

    virtual void framebufferUpdateStart() = 0;
    virtual void framebufferUpdateEnd() = 0;
    virtual bool dataRect(const core::Rect& r, int encoding,
                          const ServerParams* serverOverride = nullptr) = 0;

    virtual void setColourMapEntries(int firstColour, int nColours,
				     uint16_t* rgbs) = 0;
    virtual void bell() = 0;
    virtual void serverCutText(const char* str) = 0;

    virtual void setLEDState(unsigned int state) = 0;

    virtual void handleClipboardCaps(uint32_t flags,
                                     const uint32_t* lengths) = 0;
    virtual void handleClipboardRequest(uint32_t flags) = 0;
    virtual void handleClipboardPeek() = 0;
    virtual void handleClipboardNotify(uint32_t flags) = 0;
    virtual void handleClipboardProvide(uint32_t flags,
                                        const size_t* lengths,
                                        const uint8_t* const* data) = 0;

    // Cache protocol extension handlers (ContentCache - session-only)
    virtual void handleCachedRect(const core::Rect& r, uint64_t cacheId) = 0;
    virtual void storeCachedRect(const core::Rect& r, uint64_t cacheId) = 0;
    
    // PersistentCache protocol extension handlers (cross-session).
    // Use the same 64-bit contentHash/cacheId identity as ContentCache so
    // both protocols share ContentKey-based keying. PersistentCache differs
    // only in that entries persist across sessions and are backed by disk.
    virtual void handlePersistentCachedRect(const core::Rect& r,
                                            uint64_t cacheId) = 0;
    // encoding is the inner payload encoding used for this INIT (e.g.
    // encodingRaw, encodingZRLE, encodingTight). This allows the client
    // implementation to treat lossy payloads differently for persistence.
    virtual void storePersistentCachedRect(const core::Rect& r,
                                           uint64_t cacheId,
                                           int encoding) = 0;
    
    // Cache seed: server tells client to take existing framebuffer pixels
    // at rect R and associate them with cache ID. Used for whole-rectangle
    // caching where subrect data was already sent via normal encoding.
    virtual void seedCachedRect(const core::Rect& r, uint64_t cacheId) = 0;
    // Whether this connection advertised the native-format cache extension
    // (PersistentCachedRectInit v2). Default false; override in clients that
    // send pseudoEncodingNativeFormatCache in SetEncodings.
    virtual bool supportsNativeFormatCache() const { return false; }

    ServerParams server;
  };
}
#endif
