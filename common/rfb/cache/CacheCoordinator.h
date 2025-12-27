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

#ifndef __RFB_CACHE_COORDINATOR_H__
#define __RFB_CACHE_COORDINATOR_H__

#include <rfb/cache/CoordinatorProtocol.h>

#include <string>
#include <memory>
#include <functional>
#include <vector>
#include <unordered_map>
#include <thread>
#include <mutex>
#include <atomic>
#include <condition_variable>

#include <rfb/cache/CoordinatorProtocol.h>

namespace rfb {
namespace cache {

// Forward declaration
class CacheCoordinator;

// =============================================================================
// CacheCoordinator - Multi-Viewer Cache Coordination
// =============================================================================
//
// Enables multiple VNC viewer processes to share a single PersistentCache
// directory. The first viewer becomes "master" and owns exclusive write access;
// subsequent viewers connect as "slaves" and send write requests to the master.
//
// Usage:
//   auto coord = CacheCoordinator::create(cacheDir, writeCallback);
//   if (coord && coord->start()) {
//     // Use coord->role() to determine master/slave
//     // Use coord->requestWrite() for slaves
//     // Master receives writes via callback
//   }

class CacheCoordinator {
public:
  // Role of this viewer in the coordination scheme
  enum class Role {
    Uninitialized,  // Not yet determined
    Master,         // First viewer; owns write access
    Slave,          // Subsequent viewer; sends writes to master
    Standalone      // Coordination disabled or failed
  };
  
  // Callback types
  
  // Called when a new index entry is written (either locally for master,
  // or received from master for slaves). The callback should update the
  // local indexMap_ and related state.
  using IndexUpdateCallback = std::function<void(const std::vector<WireIndexEntry>&)>;
  
  // Called by master when a slave requests a write. Master should write to
  // shard and return the completed entry with shardId/offset filled in.
  // Returns true on success, false on failure.
  using WriteRequestCallback = std::function<bool(const WireIndexEntry& entry,
                                                   const std::vector<uint8_t>& payload,
                                                   WireIndexEntry& resultEntry)>;
  
  // Factory method - determines role and creates appropriate coordinator
  // Returns nullptr if coordination is disabled or not supported (e.g., Windows)
  static std::unique_ptr<CacheCoordinator> create(
      const std::string& cacheDir,
      IndexUpdateCallback indexUpdateCb,
      WriteRequestCallback writeRequestCb);
  
  virtual ~CacheCoordinator() = default;
  
  // Get the current role
  virtual Role role() const = 0;
  
  // Start the coordinator (bind socket for master, connect for slave)
  // Returns true on success
  virtual bool start() = 0;
  
  // Stop the coordinator gracefully
  virtual void stop() = 0;
  
  // Check if coordinator is running
  virtual bool isRunning() const = 0;
  
  // ==========================================================================
  // Write Coordination (for slaves)
  // ==========================================================================
  
  // Request the master to write a cache entry.
  // - entry: metadata for the entry (payloadOffset/shardId may be 0)
  // - payload: pixel data to write
  // - resultEntry: on success, filled with completed entry from master
  // Returns true if master successfully wrote the entry
  //
  // For master: this is a no-op (returns false, use direct write instead)
  virtual bool requestWrite(const WireIndexEntry& entry,
                            const std::vector<uint8_t>& payload,
                            WireIndexEntry& resultEntry) = 0;
  
  // ==========================================================================
  // Index Synchronization
  // ==========================================================================
  
  // Broadcast new index entries to all slaves (master only)
  virtual void broadcastIndexUpdate(const std::vector<WireIndexEntry>& entries) = 0;
  
  // Query master if a hash exists (slave only, optional optimization)
  // Returns true if found and fills entry
  virtual bool queryIndex(const uint8_t* hash, uint16_t width, uint16_t height,
                          WireIndexEntry& entry) = 0;
  
  // ==========================================================================
  // Statistics
  // ==========================================================================
  
  struct Stats {
    size_t connectedSlaves;     // Number of connected slaves (master only)
    size_t writeRequestsSent;   // Write requests sent to master (slave only)
    size_t writeRequestsRecv;   // Write requests received from slaves (master only)
    size_t indexUpdatesSent;    // Index updates broadcast (master only)
    size_t indexUpdatesRecv;    // Index updates received (slave only)
    uint64_t bytesWrittenForSlaves;  // Bytes written on behalf of slaves (master)
  };
  
  virtual Stats getStats() const = 0;
  
  // Get cache directory
  const std::string& cacheDir() const { return cacheDir_; }

protected:
  explicit CacheCoordinator(const std::string& cacheDir,
                            IndexUpdateCallback indexUpdateCb,
                            WriteRequestCallback writeRequestCb)
    : cacheDir_(cacheDir),
      indexUpdateCallback_(std::move(indexUpdateCb)),
      writeRequestCallback_(std::move(writeRequestCb)) {}
  
  std::string cacheDir_;
  IndexUpdateCallback indexUpdateCallback_;
  WriteRequestCallback writeRequestCallback_;
};

// =============================================================================
// MasterCoordinator - Server-side coordinator
// =============================================================================

class MasterCoordinator : public CacheCoordinator {
public:
  MasterCoordinator(const std::string& cacheDir,
                    IndexUpdateCallback indexUpdateCb,
                    WriteRequestCallback writeRequestCb);
  ~MasterCoordinator() override;
  
  Role role() const override { return Role::Master; }
  bool start() override;
  void stop() override;
  bool isRunning() const override { return running_; }
  
  bool requestWrite(const WireIndexEntry& entry,
                    const std::vector<uint8_t>& payload,
                    WireIndexEntry& resultEntry) override;
  
  void broadcastIndexUpdate(const std::vector<WireIndexEntry>& entries) override;
  
  bool queryIndex(const uint8_t* hash, uint16_t width, uint16_t height,
                  WireIndexEntry& entry) override;
  
  Stats getStats() const override;
  
private:
  void serverThread();
  void handleClient(int clientFd);
  bool sendMessage(int fd, const CoordMessage& msg);
  bool sendWelcome(int clientFd);
  void handleWriteRequest(int clientFd, const CoordMessage& msg);
  void handleSlaveExit(int clientFd);
  void removeClient(int clientFd);
  
  int lockFd_;
  int listenFd_;
  std::atomic<bool> running_;
  std::atomic<bool> stopRequested_;
  std::unique_ptr<std::thread> serverThread_;
  
  mutable std::mutex clientsMutex_;
  std::vector<int> clientFds_;
  
  mutable std::mutex statsMutex_;
  Stats stats_;
  
  // Buffer for receiving messages from each client
  std::unordered_map<int, std::vector<uint8_t>> clientBuffers_;
};

// =============================================================================
// SlaveCoordinator - Client-side coordinator
// =============================================================================

class SlaveCoordinator : public CacheCoordinator {
public:
  SlaveCoordinator(const std::string& cacheDir,
                   IndexUpdateCallback indexUpdateCb,
                   WriteRequestCallback writeRequestCb);
  ~SlaveCoordinator() override;
  
  Role role() const override { return Role::Slave; }
  bool start() override;
  void stop() override;
  bool isRunning() const override { return running_; }
  
  bool requestWrite(const WireIndexEntry& entry,
                    const std::vector<uint8_t>& payload,
                    WireIndexEntry& resultEntry) override;
  
  void broadcastIndexUpdate(const std::vector<WireIndexEntry>& entries) override;
  
  bool queryIndex(const uint8_t* hash, uint16_t width, uint16_t height,
                  WireIndexEntry& entry) override;
  
  Stats getStats() const override;
  
private:
  void readerThread();
  bool sendMessage(const CoordMessage& msg);
  bool connectToMaster();
  void handleMessage(const CoordMessage& msg);
  void handleWelcome(const CoordMessage& msg);
  void handleWriteAck(const CoordMessage& msg);
  void handleIndexUpdate(const CoordMessage& msg);
  void handleMasterExit();
  bool attemptElection();
  
  int socketFd_;
  std::atomic<bool> running_;
  std::atomic<bool> stopRequested_;
  std::unique_ptr<std::thread> readerThread_;
  
  mutable std::mutex socketMutex_;
  std::vector<uint8_t> recvBuffer_;
  
  // For synchronous write requests
  std::mutex writeMutex_;
  std::condition_variable writeCond_;
  bool writeAckReceived_;
  WireIndexEntry lastWriteAck_;
  bool lastWriteSuccess_;
  
  mutable std::mutex statsMutex_;
  Stats stats_;
};

// =============================================================================
// StandaloneCoordinator - No-op coordinator for single-viewer mode
// =============================================================================

class StandaloneCoordinator : public CacheCoordinator {
public:
  StandaloneCoordinator(const std::string& cacheDir,
                        IndexUpdateCallback indexUpdateCb,
                        WriteRequestCallback writeRequestCb)
    : CacheCoordinator(cacheDir, std::move(indexUpdateCb), std::move(writeRequestCb)) {}
  
  Role role() const override { return Role::Standalone; }
  bool start() override { return true; }
  void stop() override {}
  bool isRunning() const override { return true; }
  
  bool requestWrite(const WireIndexEntry&, const std::vector<uint8_t>&,
                    WireIndexEntry&) override { return false; }
  
  void broadcastIndexUpdate(const std::vector<WireIndexEntry>&) override {}
  
  bool queryIndex(const uint8_t*, uint16_t, uint16_t, WireIndexEntry&) override {
    return false;
  }
  
  Stats getStats() const override { return Stats{}; }
};

}  // namespace cache
}  // namespace rfb

#endif // __RFB_CACHE_COORDINATOR_H__
