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
#include <rfb/CacheKey.h>
#include <rdr/MemOutStream.h>
#include <rdr/MemInStream.h>

using namespace rfb;

static CacheKey makeKey(uint8_t seed)
{
  CacheKey key;
  for (size_t i = 0; i < key.bytes.size(); i++) {
    key.bytes[i] = static_cast<uint8_t>(seed + i);
  }
  return key;
}

static CacheKey makeKeyFill(uint8_t value)
{
  CacheKey key;
  key.bytes.fill(value);
  return key;
}

// Mock handler to capture hash report messages
class MockHashReportHandler : public SMsgHandler {
public:
  CacheKey receivedCanonical;
  CacheKey receivedActual;
  bool reportReceived = false;

  // Override hash report handler
  void handlePersistentCacheHashReport(const CacheKey& canonical, const CacheKey& actual) override {
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
  CacheKey canonical = makeKey(0x10);
  CacheKey actual = canonical;  // Same = lossless

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
  CacheKey canonical = makeKey(0x20);
  CacheKey actual = makeKey(0x40);  // Different = lossy

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
  // Verify exact wire format: type(1) + canonical(16) + actual(16) = 33 bytes
  CacheKey canonical = makeKey(0xAA);
  CacheKey actual = makeKey(0x11);

  rdr::MemOutStream outStream;
  ServerParams serverParams;
  CMsgWriter writer(&serverParams, &outStream);
  writer.writePersistentCacheHashReport(canonical, actual);

  const uint8_t* data = outStream.data();
  size_t length = outStream.length();

  // Expected: msgType(1) + canonical(16) + actual(16) = 33 bytes
  ASSERT_EQ(length, 33);

  // Verify message type
  EXPECT_EQ(data[0], msgTypePersistentCacheHashReport);

  // Verify canonical bytes
  for (size_t i = 0; i < canonical.bytes.size(); i++) {
    EXPECT_EQ(data[1 + i], canonical.bytes[i]);
  }

  // Verify actual bytes
  for (size_t i = 0; i < actual.bytes.size(); i++) {
    EXPECT_EQ(data[17 + i], actual.bytes[i]);
  }
}

TEST(HashReportProtocol, MultipleReports)
{
  // Test sending multiple hash reports in sequence
  std::vector<std::pair<CacheKey, CacheKey>> reports = {
    {makeKey(0x01), makeKey(0x01)},  // Lossless
    {makeKey(0x10), makeKey(0x20)},  // Lossy
    {makeKey(0x30), makeKey(0x30)},  // Lossless
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
  CacheKey canonical;
  CacheKey actual;

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

TEST(HashReportProtocol, MaxHashes)
{
  // Edge case: all-0xFF vs all-0xFE bytes
  CacheKey canonical = makeKeyFill(0xFF);
  CacheKey actual = makeKeyFill(0xFE);

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
