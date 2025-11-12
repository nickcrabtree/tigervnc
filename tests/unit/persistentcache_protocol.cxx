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

// Mock handler to capture eviction messages
class MockSMsgHandler : public SMsgHandler {
public:
  std::vector<std::vector<uint8_t>> receivedEvictions;
  
  // Override PersistentCache eviction to capture data
  void handlePersistentCacheEviction(const std::vector<std::vector<uint8_t>>& hashes) override {
    receivedEvictions = hashes;
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
  void handlePersistentCacheQuery(const std::vector<std::vector<uint8_t>>&) override {}
  void handlePersistentHashList(uint32_t, uint16_t, uint16_t, const std::vector<std::vector<uint8_t>>&) override {}
};

// ============================================================================
// PersistentCacheEviction Message Tests
// ============================================================================

TEST(PersistentCacheProtocol, EvictionRoundTripBasic)
{
  // Create test hashes
  std::vector<std::vector<uint8_t>> hashes;
  hashes.push_back({0x01, 0x02, 0x03, 0x04});
  hashes.push_back({0xAA, 0xBB, 0xCC, 0xDD, 0xEE});
  
  // Write to stream
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  // Read back
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  // Skip message type byte (already read in readMsg())
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  // Manually invoke reader method
  EXPECT_TRUE(reader.readMsg());
  
  // Verify received hashes
  ASSERT_EQ(handler.receivedEvictions.size(), 2);
  EXPECT_EQ(handler.receivedEvictions[0], hashes[0]);
  EXPECT_EQ(handler.receivedEvictions[1], hashes[1]);
}

TEST(PersistentCacheProtocol, EvictionEmptyList)
{
  std::vector<std::vector<uint8_t>> hashes;  // Empty
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  // Should write message with count=0
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);  // padding
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 0);
}

TEST(PersistentCacheProtocol, EvictionMaxHashes)
{
  // Test with maximum allowed hashes (1000)
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 1000; i++) {
    std::vector<uint8_t> hash;
    hash.push_back(i & 0xFF);
    hash.push_back((i >> 8) & 0xFF);
    hashes.push_back(hash);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  EXPECT_TRUE(reader.readMsg());
  EXPECT_EQ(handler.receivedEvictions.size(), 1000);
}

TEST(PersistentCacheProtocol, EvictionVariableHashLengths)
{
  std::vector<std::vector<uint8_t>> hashes;
  
  // MD5 (16 bytes)
  hashes.push_back(std::vector<uint8_t>(16, 0xAA));
  
  // SHA-256 (32 bytes)
  hashes.push_back(std::vector<uint8_t>(32, 0xBB));
  
  // SHA-512 (64 bytes)
  hashes.push_back(std::vector<uint8_t>(64, 0xCC));
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  inStream.skip(1);  // Skip msg type
  EXPECT_TRUE(reader.readMsg());
  
  ASSERT_EQ(handler.receivedEvictions.size(), 3);
  EXPECT_EQ(handler.receivedEvictions[0].size(), 16);
  EXPECT_EQ(handler.receivedEvictions[1].size(), 32);
  EXPECT_EQ(handler.receivedEvictions[2].size(), 64);
}

TEST(PersistentCacheProtocol, EvictionMaxHashLength)
{
  std::vector<std::vector<uint8_t>> hashes;
  
  // Maximum hash length (64 bytes)
  hashes.push_back(std::vector<uint8_t>(64, 0xFF));
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockSMsgHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  inStream.skip(1);
  EXPECT_TRUE(reader.readMsg());
  
  ASSERT_EQ(handler.receivedEvictions.size(), 1);
  EXPECT_EQ(handler.receivedEvictions[0].size(), 64);
}

// ============================================================================
// Batched Eviction Tests
// ============================================================================

TEST(PersistentCacheProtocol, EvictionBatchedLargeSet)
{
  // Test batching with 350 hashes (should create 4 messages: 100+100+100+50)
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 350; i++) {
    std::vector<uint8_t> hash(32);  // SHA-256
    hash[0] = i & 0xFF;
    hash[1] = (i >> 8) & 0xFF;
    hashes.push_back(hash);
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(hashes);
  
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
    
    // Skip hash data
    for (uint16_t i = 0; i < count; i++) {
      uint8_t hashLen = inStream.readU8();
      inStream.skip(hashLen);
    }
    
    totalReceived += count;
  }
  
  EXPECT_EQ(totalReceived, 350);
}

TEST(PersistentCacheProtocol, EvictionBatchedSingle)
{
  // Small set (< 100) should be single message
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 50; i++) {
    hashes.push_back({uint8_t(i)});
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(hashes);
  
  // Should be single message
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 50);
  
  // Should be no more data
  for (int i = 0; i < 50; i++) {
    uint8_t hashLen = inStream.readU8();
    inStream.skip(hashLen);
  }
  
  EXPECT_EQ(inStream.avail(), 0);
}

// ============================================================================
// Wire Format Validation
// ============================================================================

TEST(PersistentCacheProtocol, WireFormatExact)
{
  std::vector<std::vector<uint8_t>> hashes;
  hashes.push_back({0x12, 0x34, 0x56});
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  const uint8_t* data = outStream.data();
  size_t length = outStream.length();
  
  // Expected: msgType(1) + pad(1) + count(2) + hashLen(1) + hash(3) = 8 bytes
  ASSERT_EQ(length, 8);
  
  EXPECT_EQ(data[0], msgTypePersistentCacheEviction);  // msgType
  EXPECT_EQ(data[1], 0);                               // padding
  EXPECT_EQ(data[2], 0);                               // count high byte
  EXPECT_EQ(data[3], 1);                               // count low byte (1 hash)
  EXPECT_EQ(data[4], 3);                               // hashLen
  EXPECT_EQ(data[5], 0x12);                            // hash[0]
  EXPECT_EQ(data[6], 0x34);                            // hash[1]
  EXPECT_EQ(data[7], 0x56);                            // hash[2]
}

TEST(PersistentCacheProtocol, WireFormatMultiple)
{
  std::vector<std::vector<uint8_t>> hashes;
  hashes.push_back({0xAA});
  hashes.push_back({0xBB, 0xCC});
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  const uint8_t* data = outStream.data();
  
  EXPECT_EQ(data[0], msgTypePersistentCacheEviction);
  EXPECT_EQ(data[1], 0);     // padding
  EXPECT_EQ(data[3], 2);     // count = 2
  
  // First hash
  EXPECT_EQ(data[4], 1);     // hashLen = 1
  EXPECT_EQ(data[5], 0xAA);  // hash[0]
  
  // Second hash
  EXPECT_EQ(data[6], 2);     // hashLen = 2
  EXPECT_EQ(data[7], 0xBB);  // hash[0]
  EXPECT_EQ(data[8], 0xCC);  // hash[1]
}

// ============================================================================
// Edge Cases
// ============================================================================

TEST(PersistentCacheProtocol, EvictionZeroLengthHash)
{
  std::vector<std::vector<uint8_t>> hashes;
  hashes.push_back({});  // Empty hash (invalid but should handle gracefully)
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEviction(hashes);
  
  const uint8_t* data = outStream.data();
  
  // Should write hashLen=0
  EXPECT_EQ(data[4], 0);
}

TEST(PersistentCacheProtocol, EvictionExactBatchBoundary)
{
  // Exactly 100 hashes - should be single batch
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 100; i++) {
    hashes.push_back({uint8_t(i)});
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(hashes);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  
  uint8_t msgType = inStream.readU8();
  EXPECT_EQ(msgType, msgTypePersistentCacheEviction);
  
  inStream.skip(1);
  uint16_t count = inStream.readU16();
  EXPECT_EQ(count, 100);
  
  // Skip all hashes
  for (int i = 0; i < 100; i++) {
    uint8_t hashLen = inStream.readU8();
    inStream.skip(hashLen);
  }
  
  // Should be exactly one message
  EXPECT_EQ(inStream.avail(), 0);
}

TEST(PersistentCacheProtocol, EvictionExactBatchBoundaryPlusOne)
{
  // 101 hashes - should be 2 batches (100 + 1)
  std::vector<std::vector<uint8_t>> hashes;
  for (int i = 0; i < 101; i++) {
    hashes.push_back({uint8_t(i)});
  }
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  writer.writePersistentCacheEvictionBatched(hashes);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  
  // First batch: 100
  uint8_t msgType1 = inStream.readU8();
  EXPECT_EQ(msgType1, msgTypePersistentCacheEviction);
  inStream.skip(1);
  uint16_t count1 = inStream.readU16();
  EXPECT_EQ(count1, 100);
  
  for (int i = 0; i < 100; i++) {
    uint8_t hashLen = inStream.readU8();
    inStream.skip(hashLen);
  }
  
  // Second batch: 1
  uint8_t msgType2 = inStream.readU8();
  EXPECT_EQ(msgType2, msgTypePersistentCacheEviction);
  inStream.skip(1);
  uint16_t count2 = inStream.readU16();
  EXPECT_EQ(count2, 1);
}
