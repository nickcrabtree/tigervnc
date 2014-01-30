/* Copyright (C) 2002-2005 RealVNC Ltd.  All Rights Reserved.
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

// -=- PixelBuffer.cxx
//
// The PixelBuffer class encapsulates the PixelFormat and dimensions
// of a block of pixel data.

#include <rfb/Exception.h>
#include <rfb/LogWriter.h>
#include <rfb/PixelBuffer.h>

using namespace rfb;
using namespace rdr;

static LogWriter vlog("PixelBuffer");


// -=- Generic pixel buffer class

PixelBuffer::PixelBuffer(const PixelFormat& pf, int w, int h)
  : format(pf), width_(w), height_(h) {}
PixelBuffer::PixelBuffer() : width_(0), height_(0) {}

PixelBuffer::~PixelBuffer() {}


void
PixelBuffer::getImage(void* imageBuf, const Rect& r, int outStride) {
  int inStride;
  const U8* data = getBuffer(r, &inStride);
  // We assume that the specified rectangle is pre-clipped to the buffer
  int bytesPerPixel = format.bpp/8;
  int inBytesPerRow = inStride * bytesPerPixel;
  if (!outStride) outStride = r.width();
  int outBytesPerRow = outStride * bytesPerPixel;
  int bytesPerMemCpy = r.width() * bytesPerPixel;
  U8* imageBufPos = (U8*)imageBuf;
  const U8* end = data + (inBytesPerRow * r.height());
  while (data < end) {
    memcpy(imageBufPos, data, bytesPerMemCpy);
    imageBufPos += outBytesPerRow;
    data += inBytesPerRow;
  }
}


FullFramePixelBuffer::FullFramePixelBuffer(const PixelFormat& pf, int w, int h,
                                           rdr::U8* data_)
  : PixelBuffer(pf, w, h), data(data_)
{
}

FullFramePixelBuffer::FullFramePixelBuffer() : data(0) {}

FullFramePixelBuffer::~FullFramePixelBuffer() {}


int FullFramePixelBuffer::getStride() const { return width(); }

rdr::U8* FullFramePixelBuffer::getBufferRW(const Rect& r, int* stride)
{
  *stride = getStride();
  return &data[(r.tl.x + (r.tl.y * *stride)) * format.bpp/8];
}


void FullFramePixelBuffer::fillRect(const Rect& r, Pixel pix) {
  int stride;
  U8 *buf, pixbuf[4];
  int w, h, b;

  buf = getBufferRW(r, &stride);
  w = r.width();
  h = r.height();
  b = format.bpp/8;

  format.bufferFromPixel(pixbuf, pix);

  while (h--) {
    int w_ = w;
    while (w_--) {
      memcpy(buf, pixbuf, b);
      buf += b;
    }
    buf += (stride - w) * b;
  }
}

void FullFramePixelBuffer::imageRect(const Rect& r, const void* pixels, int srcStride) {
  int bytesPerPixel = getPF().bpp/8;
  int destStride;
  U8* dest = getBufferRW(r, &destStride);
  int bytesPerDestRow = bytesPerPixel * destStride;
  if (!srcStride) srcStride = r.width();
  int bytesPerSrcRow = bytesPerPixel * srcStride;
  int bytesPerFill = bytesPerPixel * r.width();
  const U8* src = (const U8*)pixels;
  U8* end = dest + (bytesPerDestRow * r.height());
  while (dest < end) {
    memcpy(dest, src, bytesPerFill);
    dest += bytesPerDestRow;
    src += bytesPerSrcRow;
  }
}

void FullFramePixelBuffer::maskRect(const Rect& r, const void* pixels, const void* mask_) {
  Rect cr = getRect().intersect(r);
  if (cr.is_empty()) return;
  int stride;
  U8* data = getBufferRW(cr, &stride);
  U8* mask = (U8*) mask_;
  int w = cr.width();
  int h = cr.height();
  int bpp = getPF().bpp;
  int pixelStride = r.width();
  int maskStride = (r.width() + 7) / 8;

  Point offset = Point(cr.tl.x-r.tl.x, cr.tl.y-r.tl.y);
  mask += offset.y * maskStride;
  for (int y = 0; y < h; y++) {
    int cy = offset.y + y;
    for (int x = 0; x < w; x++) {
      int cx = offset.x + x;
      U8* byte = mask + (cx / 8);
      int bit = 7 - cx % 8;
      if ((*byte) & (1 << bit)) {
        switch (bpp) {
        case 8:
          ((U8*)data)[y * stride + x] = ((U8*)pixels)[cy * pixelStride + cx];
          break;
        case 16:
          ((U16*)data)[y * stride + x] = ((U16*)pixels)[cy * pixelStride + cx];
          break;
        case 32:
          ((U32*)data)[y * stride + x] = ((U32*)pixels)[cy * pixelStride + cx];
          break;
        }
      }
    }
    mask += maskStride;
  }
}

void FullFramePixelBuffer::maskRect(const Rect& r, Pixel pixel, const void* mask_) {
  Rect cr = getRect().intersect(r);
  if (cr.is_empty()) return;
  int stride;
  U8* data = getBufferRW(cr, &stride);
  U8* mask = (U8*) mask_;
  int w = cr.width();
  int h = cr.height();
  int bpp = getPF().bpp;
  int maskStride = (r.width() + 7) / 8;

  Point offset = Point(cr.tl.x-r.tl.x, cr.tl.y-r.tl.y);
  mask += offset.y * maskStride;
  for (int y = 0; y < h; y++) {
    for (int x = 0; x < w; x++) {
      int cx = offset.x + x;
      U8* byte = mask + (cx / 8);
      int bit = 7 - cx % 8;
      if ((*byte) & (1 << bit)) {
        switch (bpp) {
        case 8:
          ((U8*)data)[y * stride + x] = pixel;
          break;
        case 16:
          ((U16*)data)[y * stride + x] = pixel;
          break;
        case 32:
          ((U32*)data)[y * stride + x] = pixel;
          break;
        }
      }
    }
    mask += maskStride;
  }
}

void FullFramePixelBuffer::copyRect(const Rect &rect, const Point &move_by_delta) {
  int stride;
  U8* data;
  unsigned int bytesPerPixel, bytesPerRow, bytesPerMemCpy;
  Rect drect, srect = rect.translate(move_by_delta.negate());

  drect = rect;
  if (!drect.enclosed_by(getRect())) {
    vlog.error("Destination rect %dx%d at %d,%d exceeds framebuffer %dx%d",
               drect.width(), drect.height(), drect.tl.x, drect.tl.y, width_, height_);
    drect = drect.intersect(getRect());
  }

  if (drect.is_empty())
    return;

  srect = drect.translate(move_by_delta.negate());
  if (!srect.enclosed_by(getRect())) {
    vlog.error("Source rect %dx%d at %d,%d exceeds framebuffer %dx%d",
               srect.width(), srect.height(), srect.tl.x, srect.tl.y, width_, height_);
    srect = srect.intersect(getRect());
    // Need to readjust the destination now that the area has changed
    drect = srect.translate(move_by_delta);
  }

  if (srect.is_empty())
    return;

  data = getBufferRW(getRect(), &stride);
  bytesPerPixel = getPF().bpp/8;
  bytesPerRow = stride * bytesPerPixel;
  bytesPerMemCpy = drect.width() * bytesPerPixel;
  if (move_by_delta.y <= 0) {
    U8* dest = data + drect.tl.x*bytesPerPixel + drect.tl.y*bytesPerRow;
    U8* src = data + srect.tl.x*bytesPerPixel + srect.tl.y*bytesPerRow;
    for (int i=drect.tl.y; i<drect.br.y; i++) {
      memmove(dest, src, bytesPerMemCpy);
      dest += bytesPerRow;
      src += bytesPerRow;
    }
  } else {
    U8* dest = data + drect.tl.x*bytesPerPixel + (drect.br.y-1)*bytesPerRow;
    U8* src = data + srect.tl.x*bytesPerPixel + (srect.br.y-1)*bytesPerRow;
    for (int i=drect.tl.y; i<drect.br.y; i++) {
      memmove(dest, src, bytesPerMemCpy);
      dest -= bytesPerRow;
      src -= bytesPerRow;
    }
  }
}


// -=- Managed pixel buffer class
// Automatically allocates enough space for the specified format & area

ManagedPixelBuffer::ManagedPixelBuffer()
  : datasize(0)
{
  checkDataSize();
};

ManagedPixelBuffer::ManagedPixelBuffer(const PixelFormat& pf, int w, int h)
  : FullFramePixelBuffer(pf, w, h, 0), datasize(0)
{
  checkDataSize();
};

ManagedPixelBuffer::~ManagedPixelBuffer() {
  if (data) delete [] data;
};


void
ManagedPixelBuffer::setPF(const PixelFormat &pf) {
  format = pf; checkDataSize();
};
void
ManagedPixelBuffer::setSize(int w, int h) {
  width_ = w; height_ = h; checkDataSize();
};


inline void
ManagedPixelBuffer::checkDataSize() {
  unsigned long new_datasize = width_ * height_ * (format.bpp/8);
  if (datasize < new_datasize) {
    vlog.debug("reallocating managed buffer (%dx%d)", width_, height_);
    if (data) {
      delete [] data;
      datasize = 0; data = 0;
    }
    if (new_datasize) {
      data = new U8[new_datasize];
      if (!data)
        throw Exception("rfb::ManagedPixelBuffer unable to allocate buffer");
      datasize = new_datasize;
    }
  }
};
