/* Copyright (C) 2002-2005 RealVNC Ltd.  All Rights Reserved.
 * Copyright 2009-2019 Pierre Ossman for Cendio AB
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
// VNCSConnectionST is our derived class of SConnection for VNCServerST - there
// is one for each connected client.  We think of VNCSConnectionST as part of
// the VNCServerST implementation, so its methods are allowed full access to
// members of VNCServerST.
//

#ifndef __RFB_VNCSCONNECTIONST_H__
#define __RFB_VNCSCONNECTIONST_H__

#include <map>
#include <unordered_map>
#include <unordered_set>

#include <core/Timer.h>

#include <rfb/Congestion.h>
#include <rfb/EncodeManager.h>
#include <rfb/SConnection.h>

namespace rfb {
  class VNCServerST;

  class VNCSConnectionST : private SConnection,
                           public core::Timer::Callback {
  public:
    VNCSConnectionST(VNCServerST* server_, network::Socket* s, bool reverse,
                     AccessRights ar);
    virtual ~VNCSConnectionST();

    // SConnection methods

    bool accessCheck(AccessRights ar) const override;
    void close(const char* reason) override;

    using SConnection::authenticated;

    // Methods called from VNCServerST.  None of these methods ever knowingly
    // throw an exception.

    // init() must be called to initialise the protocol.  If it fails it
    // returns false, and close() will have been called.
    bool init();

    // processMessages() processes incoming messages from the client, invoking
    // various callbacks as a result.  It continues to process messages until
    // reading might block.  shutdown() will be called on the connection's
    // Socket if an error occurs, via the close() call.
    void processMessages();

    // flushSocket() pushes any unwritten data on to the network.
    void flushSocket();

    // Called when the underlying pixelbuffer is resized or replaced.
    void pixelBufferChange();

    // Wrappers to make these methods "safe" for VNCServerST.
    void writeFramebufferUpdateOrClose();
    void screenLayoutChangeOrClose(uint16_t reason);
    void setCursorOrClose();
    void bellOrClose();
    void setDesktopNameOrClose(const char *name);
    void setLEDStateOrClose(unsigned int state);
    void approveConnectionOrClose(bool accept, const char* reason);
    void requestClipboardOrClose();
    void announceClipboardOrClose(bool available);
    void sendClipboardDataOrClose(const char* data);
    void desktopReadyOrClose();

    // The following methods never throw exceptions

    // getComparerState() returns if this client would like the framebuffer
    // comparer to be enabled.
    bool getComparerState();

    // renderedCursorChange() is called whenever the server-side rendered
    // cursor changes shape or position.  It ensures that the next update will
    // clean up the old rendered cursor and if necessary draw the new rendered
    // cursor.
    void renderedCursorChange();

    // cursorPositionChange() is called whenever the cursor has changed position by
    // the server.  If the client supports being informed about these changes then
    // it will arrange for the new cursor position to be sent to the client.
    void cursorPositionChange();

    // needRenderedCursor() returns true if this client needs the server-side
    // rendered cursor.  This may be because it does not support local cursor
    // or because the current cursor position has not been set by this client.
    bool needRenderedCursor();

    network::Socket* getSock() { return sock; }

    // Change tracking

    void add_changed(const core::Region& region) { updates.add_changed(region); }
    void add_copied(const core::Region& dest, const core::Point& delta) {
      updates.add_copied(dest, delta);
    }

    const char* getPeerEndpoint() const {return peerEndpoint.c_str();}

  private:
    // SConnection callbacks

    // These methods are invoked as callbacks from processMsg(
    void authSuccess() override;
    void queryConnection(const char* userName) override;
    void clientReady(bool shared) override;
    void setEncodings(int nEncodings, const int32_t* encodings) override;
    void setPixelFormat(const PixelFormat& pf) override;
    void pointerEvent(const core::Point& pos,
                      uint16_t buttonMask) override;
    void keyEvent(uint32_t keysym, uint32_t keycode,
                  bool down) override;
    void framebufferUpdateRequest(const core::Rect& r,
                                  bool incremental) override;
    void setDesktopSize(int fb_width, int fb_height,
                        const ScreenSet& layout) override;
    void fence(uint32_t flags, unsigned len,
               const uint8_t data[]) override;
    void enableContinuousUpdates(bool enable,
                                 int x, int y, int w, int h) override;
    void handleClipboardRequest() override;
    void handleClipboardAnnounce(bool available) override;
    void handleClipboardData(const char* data) override;
    void supportsLocalCursor() override;
    void supportsFence() override;
    void supportsContinuousUpdates() override;
    void supportsLEDState() override;
    void handleRequestCachedData(uint64_t cacheId) override;
    void handleCacheEviction(const std::vector<uint64_t>& cacheIds) override;
    void onCachedRectRef(uint64_t cacheId, const core::Rect& r) override;
    void handlePersistentCacheQuery(const std::vector<uint64_t>& cacheIds) override;
    void handlePersistentHashList(uint32_t sequenceId, uint16_t totalChunks,
                                  uint16_t chunkIndex,
                                  const std::vector<uint64_t>& cacheIds) override;
    void handlePersistentCacheEviction(const std::vector<uint64_t>& cacheIds) override;
    void handlePersistentCacheHashReport(uint64_t canonicalId, uint64_t lossyId) override;
    void handleDebugDumpRequest(uint32_t timestamp) override;

    // PersistentCache request helpers used by encoder
    bool clientRequestedPersistent(uint64_t id) const override {
      return clientRequestedPersistentIds_.find(id) != clientRequestedPersistentIds_.end();
    }
    void clearClientPersistentRequest(uint64_t id) override {
      auto it = clientRequestedPersistentIds_.find(id);
      if (it != clientRequestedPersistentIds_.end())
        clientRequestedPersistentIds_.erase(it);
    }

    // Unified cache ID tracking for both ContentCache and PersistentCache.
    // All cache identities are 64-bit content IDs; ContentCache is now an
    // ephemeral policy of the same engine. We therefore treat "persistent"
    // and "content" IDs as aliases over a single ID space.
    bool knowsPersistentId(uint64_t id) const override {
      return (knownPersistentIds_.find(id) != knownPersistentIds_.end()) ||
             (knownCacheIds_.find(id) != knownCacheIds_.end());
    }
    void markPersistentIdKnown(uint64_t id) override {
      knownPersistentIds_.insert(id);
      knownCacheIds_.insert(id);
    }
    
    // Lossy hash cache management
    void cacheLossyHash(uint64_t canonical, uint64_t lossy) override {
      lossyHashCache_[canonical] = lossy;
    }
    bool hasLossyHash(uint64_t canonical, uint64_t& lossy) const override {
      auto it = lossyHashCache_.find(canonical);
      if (it != lossyHashCache_.end()) {
        lossy = it->second;
        return true;
      }
      return false;
    }
    
    // Viewer confirmation tracking
    bool viewerHasConfirmed(uint64_t id) const {
      return viewerConfirmedCache_.find(id) != viewerConfirmedCache_.end();
    }
    void markPending(uint64_t id) {
      viewerPendingConfirmation_.insert(id);
    }
    void confirmPendingIds() {
      // Move all pending to confirmed (frame update succeeded)
      for (uint64_t id : viewerPendingConfirmation_) {
        viewerConfirmedCache_.insert(id);
      }
      viewerPendingConfirmation_.clear();
    }
    void removePendingId(uint64_t id) {
      // Client sent RequestCachedData - didn't have this ID
      viewerPendingConfirmation_.erase(id);
      viewerConfirmedCache_.erase(id);
    }

  private:
    std::unordered_set<uint64_t> clientRequestedPersistentIds_;
    
    // Session-scoped tracking of persistent IDs known by client
    // (from initial inventory OR sent via PersistentCachedRectInit this session)
    std::unordered_set<uint64_t> knownPersistentIds_;
    
    // Lossy hash cache: canonical hash (lossless) -> lossy hash (post-decode)
    // Used when encoding with lossy compression (e.g. JPEG) to map from
    // server's lossless content hash to the hash the client will compute
    // after decoding the lossy data.
    std::unordered_map<uint64_t, uint64_t> lossyHashCache_;
    
    // Viewer confirmed cache: IDs that viewer has explicitly confirmed having
    // (by not sending RequestCachedData after receiving a reference)
    std::unordered_set<uint64_t> viewerConfirmedCache_;
    
    // Viewer pending confirmation: IDs sent to viewer, awaiting confirmation
    // Moved to viewerConfirmedCache_ after successful frame update
    std::unordered_set<uint64_t> viewerPendingConfirmation_;

    // Record that we just referenced a CachedRect with this ID for this rect
    void recordCachedRectRef(uint64_t cacheId, const core::Rect& r);

    // Drain pending cache init requests (EncodeManager will send them)
    void drainPendingCachedInits(std::vector<std::pair<uint64_t, core::Rect>>& out) override { out.swap(pendingCacheInit_); }
    // Legacy ContentCache helpers now delegate to the unified persistent-ID
    // tracking so both protocols share a single notion of "known" IDs.
    bool knowsCacheId(uint64_t id) const override { return knowsPersistentId(id); }
    void queueCachedInit(uint64_t cacheId, const core::Rect& r) override { pendingCacheInit_.emplace_back(cacheId, r); }
    void markCacheIdKnown(uint64_t id) override { markPersistentIdKnown(id); }

    // Timer callbacks
    void handleTimeout(core::Timer* t) override;

    // Internal methods

    bool isShiftPressed();

    // Congestion control
    void writeRTTPing();
    bool isCongested();

    // writeFramebufferUpdate() attempts to write a framebuffer update to the
    // client.

    void writeFramebufferUpdate();
    void writeNoDataUpdate();
    void writeDataUpdate();
    void writeLosslessRefresh();

    void screenLayoutChange(uint16_t reason);
    void setCursor();
    void setCursorPos();
    void setDesktopName(const char *name);
    void setLEDState(unsigned int state);
    void desktopReady() override;

  private:
    network::Socket* sock;
    std::string peerEndpoint;
    bool reverseConnection;

    bool inProcessMessages;

    bool pendingSyncFence, syncFence;
    uint32_t fenceFlags;
    unsigned fenceDataLen;
    uint8_t *fenceData;

    Congestion congestion;
    core::Timer congestionTimer;
    core::Timer losslessTimer;

    VNCServerST* server;
    SimpleUpdateTracker updates;
    core::Region requested;
    bool updateRenderedCursor, removeRenderedCursor;
    core::Region damagedCursorRegion;
    bool continuousUpdates;
    core::Region cuRegion;
    EncodeManager encodeManager;

    // Track last referenced rectangle per cacheId for targeted refresh on miss
    std::unordered_map<uint64_t, core::Rect> lastCachedRectRef_;
    std::vector<std::pair<uint64_t, core::Rect>> pendingCacheInit_;
    std::unordered_set<uint64_t> knownCacheIds_;
    
    // Update counter for periodic cache statistics logging
    unsigned updateCount_;

    std::map<uint32_t, uint32_t> pressedKeys;

    core::Timer idleTimer;

    time_t pointerEventTime;
    core::Point pointerEventPos;
    bool clientHasCursor;

    // Session timing for aggregate per-client bandwidth statistics
    struct timeval sessionStartTime_;

    std::string closeReason;
  };
}
#endif
