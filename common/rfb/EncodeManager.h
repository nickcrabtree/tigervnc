/* Copyright (C) 2000-2003 Constantin Kaplinsky.  All Rights Reserved.
 * Copyright (C) 2011 D. R. Commander.  All Rights Reserved.
 * Copyright 2014-2022 Pierre Ossman for Cendio AB
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
#ifndef __RFB_ENCODEMANAGER_H__
#define __RFB_ENCODEMANAGER_H__

#include <vector>
#include <unordered_set>
#include <queue>

#include <stdint.h>

#include <core/Region.h>
#include <core/Timer.h>
#include <rfb/PixelBuffer.h>
#include <rfb/Palette.h>
#include <rfb/cache/ServerHashSet.h>

namespace rfb {

  class SConnection;
  class Encoder;
  class UpdateInfo;
  class PixelBuffer;
  class RenderedCursor;

  // Encoder classes and types shared between implementation and stats
  enum EncoderClass {
    encoderRaw,
    encoderRRE,
    encoderHextile,
    encoderTight,
    encoderTightJPEG,
    encoderZRLE,
    encoderClassMax,
  };

  enum EncoderType {
    encoderSolid,
    encoderBitmap,
    encoderBitmapRLE,
    encoderIndexed,
    encoderIndexedRLE,
    encoderFullColour,
    encoderTypeMax,
  };

  struct RectInfo {
    int rleRuns;
    Palette palette;
  };

  class EncodeManager : public core::Timer::Callback {
  public:
    EncodeManager(SConnection* conn);
    ~EncodeManager();

    void logStats();

    // Hack to let ConnParams calculate the client's preferred encoding
    static bool supported(int encoding);

    bool needsLosslessRefresh(const core::Region& req);
    int getNextLosslessRefresh(const core::Region& req);

    void pruneLosslessRefresh(const core::Region& limits);

    void forceRefresh(const core::Region& req);

    void writeUpdate(const UpdateInfo& ui, const PixelBuffer* pb,
                     const RenderedCursor* renderedCursor);

    void writeLosslessRefresh(const core::Region& req,
                              const PixelBuffer* pb,
                              const RenderedCursor* renderedCursor,
                              size_t maxUpdateSize);

    // PersistentCache protocol support - public interface (64-bit IDs)
    void addClientKnownHash(uint64_t cacheId);
    void removeClientKnownHash(uint64_t cacheId);
    bool clientKnowsHash(uint64_t cacheId) const;
    void setUsePersistentCache(bool enable) { usePersistentCache = enable; }

  protected:
    void handleTimeout(core::Timer* t) override;

    void doUpdate(bool allowLossy, const core::Region& changed,
                  const core::Region& copied,
                  const core::Point& copy_delta,
                  const PixelBuffer* pb,
                  const RenderedCursor* renderedCursor);
    void prepareEncoders(bool allowLossy);

    core::Region getLosslessRefresh(const core::Region& req,
                                    size_t maxUpdateSize);

    int computeNumRects(const core::Region& changed);

    Encoder* startRect(const core::Rect& rect, int type);
    void endRect();

    void writeCopyRects(const core::Region& copied,
                        const core::Point& delta);
    void writeSolidRects(core::Region* changed, const PixelBuffer* pb);
    void findSolidRect(const core::Rect& rect, core::Region* changed,
                       const PixelBuffer* pb);
    void writeRects(const core::Region& changed, const PixelBuffer* pb);

    void writeSubRect(const core::Rect& rect, const PixelBuffer* pb);

    // Shared encoder selection for both normal rects and cache INIT
    // paths (ContentCache and PersistentCache). Populates ppb, info
    // and type for the given rect.
    void selectEncoderForRect(const core::Rect& rect,
                              const PixelBuffer* pb,
                              PixelBuffer*& ppb,
                              struct RectInfo* info,
                              EncoderType& type);

    // Unified cache protocol support (PersistentCache-style, 64-bit IDs)
    bool tryPersistentCacheLookup(const core::Rect& rect, const PixelBuffer* pb);

    bool checkSolidTile(const core::Rect& r, const uint8_t* colourValue,
                        const PixelBuffer *pb);
    void extendSolidAreaByBlock(const core::Rect& r,
                                const uint8_t* colourValue,
                                const PixelBuffer* pb, core::Rect* er);
    void extendSolidAreaByPixel(const core::Rect& r,
                                const core::Rect& sr,
                                const uint8_t* colourValue,
                                const PixelBuffer* pb, core::Rect* er);

    PixelBuffer* preparePixelBuffer(const core::Rect& rect,
                                    const PixelBuffer* pb, bool convert);

    bool analyseRect(const PixelBuffer *pb,
                     struct RectInfo *info, int maxColours);

  protected:
    // Templated, optimised methods
    template<class T>
    inline bool checkSolidTile(int width, int height,
                               const T* buffer, int stride,
                               const T colourValue);
    template<class T>
    inline bool analyseRect(int width, int height,
                            const T* buffer, int stride,
                            struct RectInfo *info, int maxColours);

  protected:
    SConnection *conn;

    std::vector<Encoder*> encoders;
    std::vector<int> activeEncoders;

    core::Region lossyRegion;
    core::Region recentlyChangedRegion;
    core::Region pendingRefreshRegion;

    core::Timer recentChangeTimer;
    core::Timer cacheStatsTimer;

    struct EncoderStats {
      unsigned rects;
      unsigned long long bytes;
      unsigned long long pixels;
      unsigned long long equivalent;
    };
    typedef std::vector< std::vector<struct EncoderStats> > StatsVector;

    unsigned updates;
    EncoderStats copyStats;
    StatsVector stats;
    int activeType;
    int beforeLength;

    class OffsetPixelBuffer : public FullFramePixelBuffer {
    public:
      OffsetPixelBuffer() {}
      virtual ~OffsetPixelBuffer() {}

      void update(const PixelFormat& pf, int width, int height,
                  const uint8_t* data_, int stride);

    private:
      uint8_t* getBufferRW(const core::Rect& r, int* stride) override;
    };

    OffsetPixelBuffer offsetPixelBuffer;
    ManagedPixelBuffer convertedPixelBuffer;

    // Last framebuffer size we saw; used to detect resolution changes.
    core::Rect lastFramebufferRect;

    // PersistentCache protocol state
    bool usePersistentCache;
    // Set of 64-bit content IDs known to be present on the client. This
    // mirrors the ContentCache cacheId tracking logic, but for
    // cross-session PersistentCache entries.
    ServerHashSet<uint64_t, std::hash<uint64_t>> clientKnownIds_;
    std::queue<std::pair<uint64_t, core::Rect>> pendingPersistentQueries_;

    struct PersistentCacheStats {
      unsigned cacheHits;
      unsigned cacheMisses;
      unsigned cacheLookups;
      unsigned long long bytesSaved;
    };
    PersistentCacheStats persistentCacheStats;
  };

}

#endif
