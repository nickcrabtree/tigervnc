/* Copyright (C) 2025 TigerVNC Team
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

// Unit tests for CacheCoordinator multi-viewer coordination

#ifdef HAVE_CONFIG_H
#include <config.h>
#endif

#include <gtest/gtest.h>
#include <rfb/cache/CacheCoordinator.h>
#include <rfb/cache/CoordinatorProtocol.h>

#include <thread>
#include <chrono>
#include <cstdlib>
#include <cstdio>
#include <unistd.h>
#include <sys/stat.h>

using namespace rfb::cache;

class CacheCoordinatorTest : public ::testing::Test {
protected:
  void SetUp() override {
    // Create a unique test directory for each test
    char tmpl[] = "/tmp/cachecoord_test_XXXXXX";
    testDir_ = mkdtemp(tmpl);
    ASSERT_FALSE(testDir_.empty());
  }
  
  void TearDown() override {
    // Clean up test directory
    if (!testDir_.empty()) {
      std::string cmd = "rm -rf " + testDir_;
      int ret = system(cmd.c_str());
      (void)ret;
    }
  }
  
  std::string testDir_;
};

// Test that WireIndexEntry has correct size
TEST_F(CacheCoordinatorTest, WireIndexEntrySize) {
  // Should be 66 bytes (16 + 2 + 4 + 4 + 2 + 2 + 2 + 8 + 8 + 1 + 1 + 16)
  EXPECT_EQ(sizeof(WireIndexEntry), 66u);
}

// Test message serialization/deserialization
TEST_F(CacheCoordinatorTest, MessageSerialization) {
  CoordMessage msg(CoordMsgType::HELLO);
  
  HelloPayload payload;
  payload.protocolVersion = COORDINATOR_PROTOCOL_VERSION;
  payload.pid = getpid();
  memset(payload.reserved, 0, sizeof(payload.reserved));
  msg.appendStruct(payload);
  
  // Serialize
  std::vector<uint8_t> data = msg.serialize();
  ASSERT_GT(data.size(), 4u);  // At least header
  
  // Parse back
  CoordMessage parsed;
  int consumed = CoordMessage::parse(data.data(), data.size(), parsed);
  EXPECT_EQ(consumed, static_cast<int>(data.size()));
  EXPECT_EQ(parsed.type(), CoordMsgType::HELLO);
  
  // Verify payload
  HelloPayload parsedPayload;
  EXPECT_TRUE(parsed.readStruct(0, parsedPayload));
  EXPECT_EQ(parsedPayload.protocolVersion, COORDINATOR_PROTOCOL_VERSION);
  EXPECT_EQ(parsedPayload.pid, static_cast<uint32_t>(getpid()));
}

// Test that first viewer becomes master
TEST_F(CacheCoordinatorTest, FirstViewerBecomesMaster) {
  int indexUpdates = 0;
  int writeRequests = 0;
  
  auto indexCb = [&](const std::vector<WireIndexEntry>&) {
    indexUpdates++;
  };
  auto writeCb = [&](const WireIndexEntry&, const std::vector<uint8_t>&,
                     WireIndexEntry&) -> bool {
    writeRequests++;
    return true;
  };
  
  auto coord = CacheCoordinator::create(testDir_, indexCb, writeCb);
  ASSERT_NE(coord, nullptr);
  EXPECT_EQ(coord->role(), CacheCoordinator::Role::Master);
  
  EXPECT_TRUE(coord->start());
  EXPECT_TRUE(coord->isRunning());
  
  coord->stop();
}

// Test that second viewer becomes slave
TEST_F(CacheCoordinatorTest, SecondViewerBecomesSlave) {
  int indexUpdates = 0;
  int writeRequests = 0;
  
  auto indexCb = [&](const std::vector<WireIndexEntry>&) {
    indexUpdates++;
  };
  auto writeCb = [&](const WireIndexEntry&, const std::vector<uint8_t>&,
                     WireIndexEntry&) -> bool {
    writeRequests++;
    return true;
  };
  
  // Start master
  auto master = CacheCoordinator::create(testDir_, indexCb, writeCb);
  ASSERT_NE(master, nullptr);
  EXPECT_EQ(master->role(), CacheCoordinator::Role::Master);
  EXPECT_TRUE(master->start());
  
  // Give master time to start listening
  std::this_thread::sleep_for(std::chrono::milliseconds(100));
  
  // Start slave
  auto slave = CacheCoordinator::create(testDir_, indexCb, writeCb);
  ASSERT_NE(slave, nullptr);
  EXPECT_EQ(slave->role(), CacheCoordinator::Role::Slave);
  EXPECT_TRUE(slave->start());
  
  // Give slave time to connect
  std::this_thread::sleep_for(std::chrono::milliseconds(100));
  
  // Verify master sees the connection
  auto masterStats = master->getStats();
  EXPECT_EQ(masterStats.connectedSlaves, 1u);
  
  slave->stop();
  master->stop();
}

// Test path helper functions
TEST_F(CacheCoordinatorTest, PathHelpers) {
  std::string sockPath = getCoordinatorSocketPath(testDir_);
  std::string lockPath = getCoordinatorLockPath(testDir_);
  std::string pidPath = getCoordinatorPidPath(testDir_);
  
  EXPECT_EQ(sockPath, testDir_ + "/coordinator.sock");
  EXPECT_EQ(lockPath, testDir_ + "/coordinator.lock");
  EXPECT_EQ(pidPath, testDir_ + "/coordinator.pid");
}

// Test incomplete message parsing
TEST_F(CacheCoordinatorTest, IncompleteMessageParsing) {
  CoordMessage msg(CoordMsgType::PING);
  std::vector<uint8_t> data = msg.serialize();
  
  // Try parsing with only partial data
  CoordMessage partial;
  for (size_t i = 1; i < data.size(); i++) {
    int consumed = CoordMessage::parse(data.data(), i, partial);
    EXPECT_EQ(consumed, 0);  // Should return 0 for incomplete
  }
  
  // Full data should parse
  int consumed = CoordMessage::parse(data.data(), data.size(), partial);
  EXPECT_EQ(consumed, static_cast<int>(data.size()));
}

// Test standalone mode (when coordination fails)
TEST_F(CacheCoordinatorTest, StandaloneMode) {
  // Create coordinator with callbacks
  auto coord = CacheCoordinator::create(testDir_, nullptr, nullptr);
  ASSERT_NE(coord, nullptr);
  
  // Should work even with null callbacks
  EXPECT_TRUE(coord->start());
  coord->stop();
}

int main(int argc, char **argv) {
  testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}
