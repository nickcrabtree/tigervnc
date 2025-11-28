/* Copyright (C) 2026 TigerVNC Team
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

#include <gtest/gtest.h>

#include <rfb/CMsgWriter.h>
#include <rfb/SMsgReader.h>
#include <rfb/SMsgHandler.h>
#include <rfb/ServerParams.h>
#include <rfb/msgTypes.h>
#include <rdr/MemOutStream.h>
#include <rdr/MemInStream.h>

using namespace rfb;

// Mock handler to capture eviction messages (now using 64-bit IDs)
class MockSMsgHandler : public SMsgHandler {
public:
  std::vector<uint64_t> receivedEvictions;
  
  // Override PersistentCache eviction to capture data
  void handlePersistentCacheEviction(const std::vector<uint64_t>& ids) override {
    receivedEvictions = ids;
  }
  
  // Stub all pure virtual methods from SMsgHandler
  void clientInit(bool) override {}
  void setPixelFormat(const PixelFormat&) override {}
  void setEncodings(int, const int32_t*) override {}
  void framebufferUpdateRequest(const core::Rect&, bool) override {}
  void setDesktopSize(int, int, const ScreenSet&) override {}
  void fence(uint32_t, unsigned, const uint8_t*) override {}
  void enableContinuousUpdates(bool, int, int, int, int) override {}
  void keyEvent(uint32_t, uint32_t, bool) override {}
  void pointerEvent(const core::Point&, uint16_t) override {}
  void clientCutText(const char*) override {}
  void handleClipboardCaps(uint32_t, const uint32_t*) override {}
  void handleClipboardRequest(uint32_t) override {}
  void handleClipboardPeek() override {}
  void handleClipboardNotify(uint32_t) override {}
  void handleClipboardProvide(uint32_t, const size_t*, const uint8_t* const*) override {}
  void handleRequestCachedData(uint64_t) override {}
  void handleCacheEviction(const std::vector<uint64_t>&) override {}
  void handlePersistentCacheQuery(const std::vector<uint64_t>&) override {}
  void handlePersistentHashList(uint32_t, uint16_t, uint16_t, const std::vector<uint64_t>&) override {}
};

// ============================================================================
// PersistentCacheEviction Message Tests
// ============================================================================

TEST(PersistentCacheProtocol, EvictionRoundTripBasic)
{
  // Create test 64-bit IDs
  std::vector<uint64_t> ids;
  ids.push_back(0x0102030405060708ULL);
  ids.push_back(0xAABBCCDDEEFF0011ULL);
  
  // Write to stream
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(ids);
  
  // Read back
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  // Skip message type byte (already read in readMsg())
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  // Manually invoke reader method
  EXPECT_TRUE(reader.readMsg());
  
  // Verify received IDs
  ASSERT_EQ(handler.receivedEvictions.size(), 2);
  EXPECT_EQ(handler.receivedEvictions[0], ids[0]);
  EXPECT_EQ(handler.receivedEvictions[1], ids[1]);
}

TEST(PersistentCacheProtocol, EvictionEmptyList)
{
  std::vector<uint64_t> ids;  // Empty
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(ids);
  
  // Should write message with count=0
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);  // padding
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 0);
}

TEST(PersistentCacheProtocol, EvictionMaxIds)
{
  // Test with maximum allowed IDs (1000)
  std::vector<uint64_t> ids;
  for (int i = 0; i < 1000; i++) {
    ids.push_back((uint64_t)i + 1);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(ids);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  EXPECT_TRUE(reader.readMsg());
  EXPECT_EQ(handler.receivedEvictions.size(), 1000);
}

// Variable hash-length tests are obsolete now that the protocol uses
// fixed 64-bit IDs on the wire. The following tests have been removed
// in favour of ID-based wire format checks.

// TEST(PersistentCacheProtocol, EvictionVariableHashLengths)
// {
//   std::vector<std::vector<uint8_t>> hashes;
//   // MD5 (16 bytes)
//   hashes.push_back(std::vector<uint8_t>(16, 0xAA));
//   // SHA-256 (32 bytes)
//   hashes.push_back(std::vector<uint8_t>(32, 0xBB));
//   // SHA-512 (64 bytes)
//   hashes.push_back(std::vector<uint8_t>(64, 0xCC));
//   rdr::MemOutStream outStream;
//   ServerParams serverParams;
//   CMsgWriter writer(&serverParams, &outStream);
//   writer.writePersistentCacheEviction(hashes);
//   rdr::MemInStream inStream(outStream.data(), outStream.length());
//   MockSMsgHandler handler;
//   SMsgReader reader(&handler, &inStream);
//   inStream.skip(1);  // Skip msg type
//   EXPECT_TRUE(reader.readMsg());
//   ASSERT_EQ(handler.receivedEvictions.size(), 3);
//   EXPECT_EQ(handler.receivedEvictions[0].size(), 16);
//   EXPECT_EQ(handler.receivedEvictions[1].size(), 32);
//   EXPECT_EQ(handler.receivedEvictions[2].size(), 64);
// }

// TEST(PersistentCacheProtocol, EvictionMaxHashLength)
// {
//   std::vector<std::vector<uint8_t>> hashes;
//   // Maximum hash length (64 bytes)
//   hashes.push_back(std::vector<uint8_t>(64, 0xFF));
//   rdr::MemOutStream outStream;
//   ServerParams serverParams;
//   CMsgWriter writer(&serverParams, &outStream);
//   writer.writePersistentCacheEviction(hashes);
//   rdr::MemInStream inStream(outStream.data(), outStream.length());
//   MockSMsgHandler handler;
//   SMsgReader reader(&handler, &inStream);
//   inStream.skip(1);
//   EXPECT_TRUE(reader.readMsg());
//   ASSERT_EQ(handler.receivedEvictions.size(), 1);
//   EXPECT_EQ(handler.receivedEvictions[0].size(), 64);
// }

// ============================================================================
// Batched Eviction Tests
// ============================================================================

TEST(PersistentCacheProtocol, EvictionBatchedLargeSet)
{
  // Test batching with 350 IDs (should create 4 messages: 100+100+100+50)
  std::vector<uint64_t> ids;
  for (int i = 0; i < 350; i++) {
    ids.push_back((uint64_t)i + 1);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(ids);
  
  // Should have written 4 separate messages
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  
  int totalReceived = 0;
  while (inStream.avail() > 0) {
    uint8_t msgType = inStream.readU8();
    EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
    
    inStream.skip(1);  // padding
    uint16_t count = inStream.readU16();
    
    // Each batch should have at most 100
    EXPECT_LE(count, 100);
    
    // Skip ID data (each is two U32 values)
    for (uint16_t i = 0; i < count; i++) {
      inStream.skip(8);
    }
    
    totalReceived += count;
  }
  
  EXPECT_EQ(totalReceived, 350);
}

TEST(PersistentCacheProtocol, EvictionBatchedSingle)
{
  // Small set (< 100) should be single message
  std::vector<uint64_t> ids;
  for (int i = 0; i < 50; i++) {
    ids.push_back((uint64_t)i + 1);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(ids);
  
  // Should be single message
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 50);
  
  // Should be no more data; skip 8 bytes per ID
  for (int i = 0; i < 50; i++) {
    inStream.skip(8);
  }
  
  EXPECT_EQ(inStream.avail(), 0);
}

// ============================================================================
// Wire Format Validation
// ============================================================================

TEST(PersistentCacheProtocol, WireFormatExact)
{
  std::vector<uint64_t> ids;
  ids.push_back(0x1234567890ABCDEFULL);
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(ids);
  
  const uint8_t* data = outStream.data();
  size_t length = outStream.length();
  
  // Expected: msgType(1) + pad(1) + count(2) + ID(8) = 12 bytes
  ASSERT_EQ(length, 12);
  
  EXPECT_EQ(data[0], msgTypePersistentCacheEviction);  // msgType
  EXPECT_EQ(data[1], 0);                               // padding
  EXPECT_EQ(data[2], 0);                               // count high byte
  EXPECT_EQ(data[3], 1);                               // count low byte (1 ID)
  // Next 8 bytes should be the 64-bit ID in big-endian (two U32s)
  uint64_t hi = ((uint64_t)data[4] << 24) | ((uint64_t)data[5] << 16) |
                ((uint64_t)data[6] << 8) | (uint64_t)data[7];
  uint64_t lo = ((uint64_t)data[8] << 24) | ((uint64_t)data[9] << 16) |
                ((uint64_t)data[10] << 8) | (uint64_t)data[11];
  uint64_t reconstructed = (hi << 32) | lo;
  EXPECT_EQ(reconstructed, ids[0]);
}

TEST(PersistentCacheProtocol, WireFormatMultiple)
{
  std::vector<uint64_t> ids;
  ids.push_back(0x00000000000000AAULL);
  ids.push_back(0x000000000000BBCCULL);
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(ids);
  
  const uint8_t* data = outStream.data();
  
  EXPECT_EQ(data[0], msgTypePersistentCacheEviction);
  EXPECT_EQ(data[1], 0);     // padding
  EXPECT_EQ(data[3], 2);     // count = 2
  
  // First ID high/low
  uint64_t hi1 = ((uint64_t)data[4] << 24) | ((uint64_t)data[5] << 16) |
                 ((uint64_t)data[6] << 8) | (uint64_t)data[7];
  uint64_t lo1 = ((uint64_t)data[8] << 24) | ((uint64_t)data[9] << 16) |
                 ((uint64_t)data[10] << 8) | (uint64_t)data[11];
  uint64_t id1 = (hi1 << 32) | lo1;
  
  // Second ID high/low
  uint64_t hi2 = ((uint64_t)data[12] << 24) | ((uint64_t)data[13] << 16) |
                 ((uint64_t)data[14] << 8) | (uint64_t)data[15];
  uint64_t lo2 = ((uint64_t)data[16] << 24) | ((uint64_t)data[17] << 16) |
                 ((uint64_t)data[18] << 8) | (uint64_t)data[19];
  uint64_t id2 = (hi2 << 32) | lo2;
  
  EXPECT_EQ(id1, ids[0]);
  EXPECT_EQ(id2, ids[1]);
}

// ============================================================================
// Edge Cases
// ============================================================================

// Zero-length hash edge case is obsolete with fixed 64-bit IDs.
// TEST(PersistentCacheProtocol, EvictionZeroLengthHash)
// {
//   ...
// }

TEST(PersistentCacheProtocol, EvictionExactBatchBoundary)
{
  // Exactly 100 IDs - should be single batch
  std::vector<uint64_t> ids;
  for (int i = 0; i < 100; i++) {
    ids.push_back((uint64_t)i + 1);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(ids);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 100);
  
  // Skip all IDs (8 bytes each)
  for (int i = 0; i < 100; i++) {
    inStream.skip(8);
  }
  
  // Should be exactly one message
  EXPECT_EQ(inStream.avail(), 0);
}

TEST(PersistentCacheProtocol, EvictionExactBatchBoundaryPlusOne)
{
  // 101 IDs - should be 2 batches (100 + 1)
  std::vector<uint64_t> ids;
  for (int i = 0; i < 101; i++) {
    ids.push_back((uint64_t)i + 1);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(ids);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  
  // First batch: 100
  uint8_t msgType1 = inStream.readU8();
  EXPECT_EQ(msgType1, msgTypePersistentCacheEviction);
  inStream.skip(1);
  uint16_t count1 = inStream.readU16();
  EXPECT_EQ(count1, 100);
  
  for (int i = 0; i < 100; i++) {
    inStream.skip(8);
  }
  
  // Second batch: 1
  uint8_t msgType2 = inStream.readU8();
  EXPECT_EQ(msgType2, msgTypePersistentCacheEviction);
  inStream.skip(1);
  uint16_t count2 = inStream.readU16();
  EXPECT_EQ(count2, 1);
}
