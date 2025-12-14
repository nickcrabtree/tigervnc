/* Copyright (C) 2025 TigerVNC Team
 * 
 * Unit tests for PersistentCacheHashReport protocol message.
 * 
 * Tests the new dual-hash reporting protocol where viewer sends both
 * canonical and actual hash to server for quality determination.
 */

#include <gtest/gtest.h>

#include <rfb/CMsgWriter.h>
#include <rfb/SMsgReader.h>
#include <rfb/SMsgHandler.h>
#include <rfb/ServerParams.h>
#include <rfb/msgTypes.h>
#include <rdr/MemOutStream.h>
#include <rdr/MemInStream.h>

using namespace rfb;

// Mock handler to capture hash report messages
class MockHashReportHandler : public SMsgHandler {
public:
  uint64_t receivedCanonical = 0;
  uint64_t receivedActual = 0;
  bool reportReceived = false;
  
  // Override hash report handler
  void handlePersistentCacheHashReport(uint64_t canonical, uint64_t actual) override {
    receivedCanonical = canonical;
    receivedActual = actual;
    reportReceived = true;
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
  void handlePersistentCacheEviction(const std::vector<uint64_t>&) override {}
};

TEST(HashReportProtocol, LosslessReport)
{
  // Test case: Viewer has lossless pixels (canonical == actual)
  uint64_t canonical = 0x1234567890ABCDEFULL;
  uint64_t actual = 0x1234567890ABCDEFULL;  // Same = lossless
  
  // Write to stream
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);
  
  // Read back
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockHashReportHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  ASSERT_TRUE(reader.readMsg());
  ASSERT_TRUE(handler.reportReceived);
  EXPECT_EQ(handler.receivedCanonical, canonical);
  EXPECT_EQ(handler.receivedActual, actual);
  
  // Verify lossless (canonical == actual)
  bool isLossless = (handler.receivedCanonical == handler.receivedActual);
  EXPECT_TRUE(isLossless);
}

TEST(HashReportProtocol, LossyReport)
{
  // Test case: Viewer has lossy pixels (canonical != actual)
  uint64_t canonical = 0x1111111111111111ULL;
  uint64_t actual = 0x2222222222222222ULL;  // Different = lossy
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockHashReportHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  ASSERT_TRUE(reader.readMsg());
  ASSERT_TRUE(handler.reportReceived);
  EXPECT_EQ(handler.receivedCanonical, canonical);
  EXPECT_EQ(handler.receivedActual, actual);
  
  // Verify lossy (canonical != actual)
  bool isLossless = (handler.receivedCanonical == handler.receivedActual);
  EXPECT_FALSE(isLossless);
}

TEST(HashReportProtocol, WireFormat)
{
  // Verify exact wire format: type(1) + canonical(8) + actual(8) = 17 bytes
  uint64_t canonical = 0xAAAABBBBCCCCDDDDULL;
  uint64_t actual = 0x1122334455667788ULL;
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);
  
  const uint8_t* data = outStream.data();
  size_t length = outStream.length();
  
  // Expected: msgType(1) + canonical(8) + actual(8) = 17 bytes
  ASSERT_EQ(length, 17);
  
  // Verify message type
  EXPECT_EQ(data[0], msgTypePersistentCacheHashReport);
  
  // Verify canonical hash (big-endian, two U32s)
  uint64_t canonicalHi = ((uint64_t)data[1] << 24) | ((uint64_t)data[2] << 16) |
                         ((uint64_t)data[3] << 8) | (uint64_t)data[4];
  uint64_t canonicalLo = ((uint64_t)data[5] << 24) | ((uint64_t)data[6] << 16) |
                         ((uint64_t)data[7] << 8) | (uint64_t)data[8];
  uint64_t reconstructedCanonical = (canonicalHi << 32) | canonicalLo;
  EXPECT_EQ(reconstructedCanonical, canonical);
  
  // Verify actual hash (big-endian, two U32s)
  uint64_t actualHi = ((uint64_t)data[9] << 24) | ((uint64_t)data[10] << 16) |
                      ((uint64_t)data[11] << 8) | (uint64_t)data[12];
  uint64_t actualLo = ((uint64_t)data[13] << 24) | ((uint64_t)data[14] << 16) |
                      ((uint64_t)data[15] << 8) | (uint64_t)data[16];
  uint64_t reconstructedActual = (actualHi << 32) | actualLo;
  EXPECT_EQ(reconstructedActual, actual);
}

TEST(HashReportProtocol, MultipleReports)
{
  // Test sending multiple hash reports in sequence
  std::vector<std::pair<uint64_t, uint64_t>> reports = {
    {0x1000000000000001ULL, 0x1000000000000001ULL},  // Lossless
    {0x2000000000000002ULL, 0x2000000000000099ULL},  // Lossy
    {0x3000000000000003ULL, 0x3000000000000003ULL},  // Lossless
  };
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  
  for (const auto& report : reports) {
    writer.writePersistentCacheHashReport(report.first, report.second);
  }
  
  // Read back all reports
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockHashReportHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  for (size_t i = 0; i < reports.size(); i++) {
    handler.reportReceived = false;
    ASSERT_TRUE(reader.readMsg()) << "Failed to read report " << i;
    ASSERT_TRUE(handler.reportReceived) << "Report " << i << " not received";
    EXPECT_EQ(handler.receivedCanonical, reports[i].first) << "Report " << i << " canonical mismatch";
    EXPECT_EQ(handler.receivedActual, reports[i].second) << "Report " << i << " actual mismatch";
  }
}

TEST(HashReportProtocol, ZeroHashes)
{
  // Edge case: both hashes are zero
  uint64_t canonical = 0;
  uint64_t actual = 0;
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockHashReportHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  ASSERT_TRUE(reader.readMsg());
  ASSERT_TRUE(handler.reportReceived);
  EXPECT_EQ(handler.receivedCanonical, 0ULL);
  EXPECT_EQ(handler.receivedActual, 0ULL);
}

TEST(HashReportProtocol, MaxHashes)
{
  // Edge case: maximum uint64_t values
  uint64_t canonical = 0xFFFFFFFFFFFFFFFFULL;
  uint64_t actual = 0xFFFFFFFFFFFFFFFEULL;
  
  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);
  
  rdr::MemInStream inStream(outStream.data(), outStream.length());
  MockHashReportHandler handler;
  SMsgReader reader(&handler, &inStream);
  
  ASSERT_TRUE(reader.readMsg());
  ASSERT_TRUE(handler.reportReceived);
  EXPECT_EQ(handler.receivedCanonical, canonical);
  EXPECT_EQ(handler.receivedActual, actual);
}
